use std::ffi::c_void;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::pin::Pin;
use std::sync::Mutex;

use windows::Win32::Foundation::ERROR_INVALID_HANDLE;
use windows::Win32::Foundation::ERROR_INVALID_PARAMETER;
use windows::Win32::Foundation::ERROR_NOT_ENOUGH_MEMORY;
use windows::Win32::Foundation::NO_ERROR;
use windows::Win32::NetworkManagement::IpHelper::CancelMibChangeNotify2;
use windows::Win32::NetworkManagement::IpHelper::MIB_NOTIFICATION_TYPE;
use windows::Win32::NetworkManagement::IpHelper::MIB_UNICASTIPADDRESS_ROW;
use windows::Win32::{
    Foundation::HANDLE, NetworkManagement::IpHelper::NotifyUnicastIpAddressChange,
    Networking::WinSock::AF_UNSPEC,
};

use crate::async_callback::{
    next_async_list, push_async_list, shared_async_callback_queue, wait_next_list,
    SharedAsyncCallbackQueue,
};
use crate::callback::Callback;
use crate::Error;
use crate::List;
use crate::Update;

struct NotificationHandle(HANDLE);

// SAFETY: The cancellation is intended to be used from another thread.
unsafe impl Send for NotificationHandle {}

impl NotificationHandle {
    fn cancel(&self) {
        unsafe {
            let _ = CancelMibChangeNotify2(self.0);
        }
    }
}

impl Drop for NotificationHandle {
    fn drop(&mut self) {
        self.cancel();
    }
}

struct WatchState {
    cursor: crate::UpdateCursor,
    callback: Callback,
    initialising: bool,
}

pub(crate) struct WatchHandle {
    _hnd: NotificationHandle,
    _state: Pin<Box<Mutex<WatchState>>>,
}

struct QueuedWatchState {
    current_list: List,
    queue: SharedAsyncCallbackQueue,
}

type QueuedWatchRegistration = (
    NotificationHandle,
    SharedAsyncCallbackQueue,
    Pin<Box<Mutex<QueuedWatchState>>>,
);

pub(crate) struct AsyncWatch {
    _hnd: NotificationHandle,
    queue: SharedAsyncCallbackQueue,
    cursor: crate::UpdateCursor,
    _state: Pin<Box<Mutex<QueuedWatchState>>>,
}

pub(crate) struct BlockingWatch {
    _hnd: NotificationHandle,
    queue: SharedAsyncCallbackQueue,
    cursor: crate::UpdateCursor,
    _state: Pin<Box<Mutex<QueuedWatchState>>>,
}

impl AsyncWatch {
    pub(crate) async fn changed(&mut self) -> Update {
        loop {
            let new_list = next_async_list(&self.queue).await;
            if let Some(update) = self.cursor.advance(new_list) {
                return update;
            }
        }
    }
}

impl BlockingWatch {
    pub(crate) fn changed(&mut self) -> Update {
        loop {
            let new_list = wait_next_list(&self.queue);
            if let Some(update) = self.cursor.advance(new_list) {
                return update;
            }
        }
    }
}

pub(crate) fn watch_interfaces_with_callback<F: FnMut(Update) + Send + 'static>(
    callback: F,
) -> Result<WatchHandle, Error> {
    let state = Box::pin(Mutex::new(WatchState {
        cursor: crate::UpdateCursor::default(),
        callback: Callback::new(Box::new(callback)),
        initialising: true,
    }));
    let state_ctx = &*state.as_ref() as *const _ as *const c_void;

    let mut hnd = HANDLE::default();
    let res = unsafe {
        NotifyUnicastIpAddressChange(AF_UNSPEC, Some(notif), Some(state_ctx), false, &mut hnd)
    };
    let hnd = match res {
        NO_ERROR => NotificationHandle(hnd),
        ERROR_INVALID_HANDLE => return Err(Error::InvalidHandle),
        ERROR_INVALID_PARAMETER => return Err(Error::InvalidParameter),
        ERROR_NOT_ENOUGH_MEMORY => return Err(Error::NotEnoughMemory),
        _ => return Err(Error::UnexpectedWindowsResult(res.0)),
    };

    let mut state_guard = state.lock().unwrap();
    let initial_list = crate::list::list_interfaces()?;
    handle_initial_notif(&mut state_guard, initial_list);
    drop(state_guard);

    Ok(WatchHandle {
        _hnd: hnd,
        _state: state,
    })
}

#[allow(clippy::extra_unused_type_parameters)]
pub(crate) fn watch_interfaces_async<A: crate::async_adapter::AsyncFdAdapter>(
) -> Result<AsyncWatch, Error> {
    let (hnd, queue, state) = register_queued_watcher()?;
    Ok(AsyncWatch {
        _hnd: hnd,
        queue,
        cursor: crate::UpdateCursor::default(),
        _state: state,
    })
}

pub(crate) fn watch_interfaces_blocking() -> Result<BlockingWatch, Error> {
    let (hnd, queue, state) = register_queued_watcher()?;
    Ok(BlockingWatch {
        _hnd: hnd,
        queue,
        cursor: crate::UpdateCursor::default(),
        _state: state,
    })
}

fn register_queued_watcher() -> Result<QueuedWatchRegistration, Error> {
    let current_list = crate::list::list_interfaces()?;
    let queue = shared_async_callback_queue();
    push_async_list(&queue, current_list.clone());
    let state = Box::pin(Mutex::new(QueuedWatchState {
        current_list,
        queue: queue.clone(),
    }));
    let state_ctx = &*state.as_ref() as *const _ as *const c_void;

    let mut hnd = HANDLE::default();
    let res = unsafe {
        NotifyUnicastIpAddressChange(
            AF_UNSPEC,
            Some(queued_notif),
            Some(state_ctx),
            false,
            &mut hnd,
        )
    };
    match res {
        NO_ERROR => Ok((NotificationHandle(hnd), queue, state)),
        ERROR_INVALID_HANDLE => Err(Error::InvalidHandle),
        ERROR_INVALID_PARAMETER => Err(Error::InvalidParameter),
        ERROR_NOT_ENOUGH_MEMORY => Err(Error::NotEnoughMemory),
        _ => Err(Error::UnexpectedWindowsResult(res.0)),
    }
}

unsafe extern "system" fn notif(
    ctx: *const c_void,
    _row: *const MIB_UNICASTIPADDRESS_ROW,
    _notification_type: MIB_NOTIFICATION_TYPE,
) {
    let _ = catch_unwind(AssertUnwindSafe(|| unsafe {
        let state_ptr = ctx as *const Mutex<WatchState>;
        let state_guard = &mut *state_ptr
            .as_ref()
            .expect("callback ctx should never be null")
            .lock()
            .unwrap();
        let Ok(new_list) = crate::list::list_interfaces() else {
            return;
        };
        handle_notif(state_guard, new_list);
    }));
}

unsafe extern "system" fn queued_notif(
    ctx: *const c_void,
    _row: *const MIB_UNICASTIPADDRESS_ROW,
    _notification_type: MIB_NOTIFICATION_TYPE,
) {
    let _ = catch_unwind(AssertUnwindSafe(|| unsafe {
        let state_ptr = ctx as *const Mutex<QueuedWatchState>;
        let state_guard = &mut *state_ptr
            .as_ref()
            .expect("callback ctx should never be null")
            .lock()
            .unwrap();
        let Ok(new_list) = crate::list::list_interfaces() else {
            return;
        };
        handle_queued_notif(state_guard, new_list);
    }));
}

fn handle_initial_notif(state: &mut WatchState, new_list: List) {
    let update = state
        .cursor
        .advance(new_list)
        .expect("initial update should always advance the cursor");
    state.callback.call_initial(update);
    state.initialising = false;
}

fn handle_notif(state: &mut WatchState, new_list: List) {
    if state.initialising {
        return;
    }
    let Some(update) = state.cursor.advance(new_list) else {
        return;
    };
    state.callback.call_from_notification(update);
}

fn handle_queued_notif(state: &mut QueuedWatchState, new_list: List) {
    if new_list == state.current_list {
        return;
    }
    state.current_list = new_list.clone();
    push_async_list(&state.queue, new_list);
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    use super::*;
    use crate::Interface;

    fn list(name: &str) -> List {
        List(HashMap::from([(
            1,
            Interface {
                index: 1,
                name: name.to_owned(),
                hw_addr: String::new(),
                ips: Vec::new(),
            },
        )]))
    }

    #[test]
    fn notifications_are_ignored_while_initialising_and_panics_quarantine_the_callback() {
        let failed_calls = Arc::new(AtomicUsize::new(0));
        let failed_calls_for_callback = failed_calls.clone();
        let state = Mutex::new(WatchState {
            cursor: crate::UpdateCursor::default(),
            callback: Callback::new(Box::new(move |update| {
                failed_calls_for_callback.fetch_add(1, Ordering::Relaxed);
                if !update.is_initial {
                    panic!("notification callback failed");
                }
            })),
            initialising: true,
        });

        handle_notif(&mut state.lock().unwrap(), list("before initialisation"));
        assert_eq!(failed_calls.load(Ordering::Relaxed), 0);

        handle_initial_notif(&mut state.lock().unwrap(), list("initial"));
        handle_notif(&mut state.lock().unwrap(), list("changed"));

        {
            let state = state.lock().expect("watch state should not be poisoned");
            assert!(!state.initialising);
            assert!(state.callback.has_failed());
        }

        handle_notif(&mut state.lock().unwrap(), list("changed again"));
        assert_eq!(failed_calls.load(Ordering::Relaxed), 2);
    }
}
