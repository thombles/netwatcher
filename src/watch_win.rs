use std::ffi::c_void;
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
use crate::Error;
use crate::List;
use crate::Update;

struct WatchState {
    cursor: crate::UpdateCursor,
    /// User's callback
    cb: Box<dyn FnMut(Update) + Send + 'static>,
}

pub(crate) struct WatchHandle {
    hnd: HANDLE,
    _state: Pin<Box<Mutex<WatchState>>>,
}

struct QueuedWatchState {
    current_list: List,
    queue: SharedAsyncCallbackQueue,
}

type QueuedWatchRegistration = (
    HANDLE,
    SharedAsyncCallbackQueue,
    Pin<Box<Mutex<QueuedWatchState>>>,
);

pub(crate) struct AsyncWatch {
    hnd: HANDLE,
    queue: SharedAsyncCallbackQueue,
    cursor: crate::UpdateCursor,
    _state: Pin<Box<Mutex<QueuedWatchState>>>,
}

pub(crate) struct BlockingWatch {
    hnd: HANDLE,
    queue: SharedAsyncCallbackQueue,
    cursor: crate::UpdateCursor,
    _state: Pin<Box<Mutex<QueuedWatchState>>>,
}

impl Drop for AsyncWatch {
    fn drop(&mut self) {
        unsafe {
            let _ = CancelMibChangeNotify2(self.hnd);
        }
    }
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

impl Drop for BlockingWatch {
    fn drop(&mut self) {
        unsafe {
            let _ = CancelMibChangeNotify2(self.hnd);
        }
    }
}

impl BlockingWatch {
    pub(crate) fn updated(&mut self) -> Update {
        loop {
            let new_list = wait_next_list(&self.queue);
            if let Some(update) = self.cursor.advance(new_list) {
                return update;
            }
        }
    }
}

impl Drop for WatchHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = CancelMibChangeNotify2(self.hnd);
        }
    }
}

pub(crate) fn watch_interfaces_with_callback<F: FnMut(Update) + Send + 'static>(
    callback: F,
) -> Result<WatchHandle, Error> {
    let state = Box::pin(Mutex::new(WatchState {
        cursor: crate::UpdateCursor::default(),
        cb: Box::new(callback),
    }));
    let state_ctx = &*state.as_ref() as *const _ as *const c_void;

    let mut hnd = HANDLE::default();
    let res = unsafe {
        NotifyUnicastIpAddressChange(AF_UNSPEC, Some(notif), Some(state_ctx), false, &mut hnd)
    };
    match res {
        NO_ERROR => {
            // Trigger an initial update
            handle_notif(&mut state.lock().unwrap(), crate::list::list_interfaces()?);
            // Then return the handle
            Ok(WatchHandle { hnd, _state: state })
        }
        ERROR_INVALID_HANDLE => Err(Error::InvalidHandle),
        ERROR_INVALID_PARAMETER => Err(Error::InvalidParameter),
        ERROR_NOT_ENOUGH_MEMORY => Err(Error::NotEnoughMemory),
        _ => Err(Error::UnexpectedWindowsResult(res.0)),
    }
}

#[allow(clippy::extra_unused_type_parameters)]
pub(crate) fn watch_interfaces_async<A: crate::async_adapter::AsyncFdAdapter>(
) -> Result<AsyncWatch, Error> {
    let (hnd, queue, state) = register_queued_watcher()?;
    Ok(AsyncWatch {
        hnd,
        queue,
        cursor: crate::UpdateCursor::default(),
        _state: state,
    })
}

pub(crate) fn watch_interfaces_blocking() -> Result<BlockingWatch, Error> {
    let (hnd, queue, state) = register_queued_watcher()?;
    Ok(BlockingWatch {
        hnd,
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
        NO_ERROR => Ok((hnd, queue, state)),
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
    let state_ptr = ctx as *const Mutex<WatchState>;
    unsafe {
        let state_guard = &mut *state_ptr
            .as_ref()
            .expect("callback ctx should never be null")
            .lock()
            .unwrap();
        let Ok(new_list) = crate::list::list_interfaces() else {
            return;
        };
        handle_notif(state_guard, new_list);
    }
}

unsafe extern "system" fn queued_notif(
    ctx: *const c_void,
    _row: *const MIB_UNICASTIPADDRESS_ROW,
    _notification_type: MIB_NOTIFICATION_TYPE,
) {
    let state_ptr = ctx as *const Mutex<QueuedWatchState>;
    unsafe {
        let state_guard = &mut *state_ptr
            .as_ref()
            .expect("callback ctx should never be null")
            .lock()
            .unwrap();
        let Ok(new_list) = crate::list::list_interfaces() else {
            return;
        };
        handle_queued_notif(state_guard, new_list);
    }
}

fn handle_notif(state: &mut WatchState, new_list: List) {
    let Some(update) = state.cursor.advance(new_list) else {
        return;
    };
    (state.cb)(update);
}

fn handle_queued_notif(state: &mut QueuedWatchState, new_list: List) {
    if new_list == state.current_list {
        return;
    }
    state.current_list = new_list.clone();
    push_async_list(&state.queue, new_list);
}
