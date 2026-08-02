use std::ffi::c_void;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::pin::Pin;
use std::sync::Mutex;

use windows::Win32::Foundation::ERROR_INVALID_HANDLE;
use windows::Win32::Foundation::ERROR_INVALID_PARAMETER;
use windows::Win32::Foundation::ERROR_NOT_ENOUGH_MEMORY;
use windows::Win32::Foundation::NO_ERROR;
use windows::Win32::Foundation::WIN32_ERROR;
use windows::Win32::NetworkManagement::IpHelper::CancelMibChangeNotify2;
use windows::Win32::NetworkManagement::IpHelper::MIB_IPINTERFACE_ROW;
use windows::Win32::NetworkManagement::IpHelper::MIB_NOTIFICATION_TYPE;
use windows::Win32::NetworkManagement::IpHelper::MIB_UNICASTIPADDRESS_ROW;
use windows::Win32::NetworkManagement::IpHelper::{
    NotifyIpInterfaceChange, NotifyUnicastIpAddressChange, PIPINTERFACE_CHANGE_CALLBACK,
    PUNICAST_IPADDRESS_CHANGE_CALLBACK,
};
use windows::Win32::{Foundation::HANDLE, Networking::WinSock::AF_UNSPEC};

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
    fn cancel(&self) -> Result<(), Error> {
        let result = unsafe { CancelMibChangeNotify2(self.0) };
        match result {
            NO_ERROR => Ok(()),
            _ => Err(Error::UnexpectedWindowsResult(result.0)),
        }
    }
}

// Owns the pinned callback state alongside the registered notification handles.
//
// The state is only dropped once every registered handle has been successfully
// cancelled. If any cancellation fails the OS may still retain the registration
// and invoke the callback later, so the state is deliberately leaked to keep the
// context pointer valid rather than allowing a use-after-free. The state is
// disabled first so any surviving callbacks are suppressed instead of running
// user code on the leaked state.
struct NotificationRegistration<T: WatchStateLike> {
    unicast: Option<NotificationHandle>,
    interface: Option<NotificationHandle>,
    state: Option<Pin<Box<Mutex<T>>>>,
}

trait WatchStateLike {
    fn disable(&mut self);
}

impl<T: WatchStateLike> NotificationRegistration<T> {
    fn new(state: Pin<Box<Mutex<T>>>) -> Self {
        NotificationRegistration {
            unicast: None,
            interface: None,
            state: Some(state),
        }
    }

    fn state(&self) -> &Mutex<T> {
        self.state
            .as_ref()
            .expect("state must be present")
            .as_ref()
            .get_ref()
    }

    fn register_unicast(
        &mut self,
        callback: PUNICAST_IPADDRESS_CHANGE_CALLBACK,
    ) -> Result<(), Error> {
        let ctx = self.state() as *const _ as *const c_void;
        let mut handle = HANDLE::default();
        let res = unsafe {
            NotifyUnicastIpAddressChange(AF_UNSPEC, callback, Some(ctx), false, &mut handle)
        };
        self.unicast = Some(registration_result(res, handle)?);
        Ok(())
    }

    fn register_interface(&mut self, callback: PIPINTERFACE_CHANGE_CALLBACK) -> Result<(), Error> {
        let ctx = self.state() as *const _ as *const c_void;
        let mut handle = HANDLE::default();
        let res =
            unsafe { NotifyIpInterfaceChange(AF_UNSPEC, callback, Some(ctx), false, &mut handle) };
        self.interface = Some(registration_result(res, handle)?);
        Ok(())
    }
}

impl<T: WatchStateLike> Drop for NotificationRegistration<T> {
    fn drop(&mut self) {
        let unicast_ok = match &self.unicast {
            Some(h) => h.cancel().is_ok(),
            None => true,
        };
        let interface_ok = match &self.interface {
            Some(h) => h.cancel().is_ok(),
            None => true,
        };
        if !unicast_ok || !interface_ok {
            if let Some(state) = self.state.take() {
                state.as_ref().get_ref().lock().unwrap().disable();
                std::mem::forget(state);
            }
        }
    }
}

struct WatchState {
    cursor: crate::UpdateCursor,
    callback: Callback,
    initialising: bool,
    disabled: bool,
}

impl WatchStateLike for WatchState {
    fn disable(&mut self) {
        self.disabled = true;
    }
}

pub(crate) struct WatchHandle {
    _registration: NotificationRegistration<WatchState>,
}

struct QueuedWatchState {
    current_list: List,
    queue: SharedAsyncCallbackQueue,
    initialising: bool,
    disabled: bool,
}

impl WatchStateLike for QueuedWatchState {
    fn disable(&mut self) {
        self.disabled = true;
    }
}

type QueuedWatchRegistration = (
    NotificationRegistration<QueuedWatchState>,
    SharedAsyncCallbackQueue,
);

pub(crate) struct AsyncWatch {
    _registration: NotificationRegistration<QueuedWatchState>,
    queue: SharedAsyncCallbackQueue,
    cursor: crate::UpdateCursor,
}

pub(crate) struct BlockingWatch {
    _registration: NotificationRegistration<QueuedWatchState>,
    queue: SharedAsyncCallbackQueue,
    cursor: crate::UpdateCursor,
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
        disabled: false,
    }));
    let registration = register_notifications(state, Some(unicast_notif), Some(interface_notif))?;

    let mut state_guard = registration.state().lock().unwrap();
    let initial_list = crate::list::list_interfaces()?;
    handle_initial_notif(&mut state_guard, initial_list);
    drop(state_guard);

    Ok(WatchHandle {
        _registration: registration,
    })
}

#[allow(clippy::extra_unused_type_parameters)]
pub(crate) fn watch_interfaces_async<A: crate::async_adapter::AsyncFdAdapter>(
) -> Result<AsyncWatch, Error> {
    let (registration, queue) = register_queued_watcher()?;
    Ok(AsyncWatch {
        _registration: registration,
        queue,
        cursor: crate::UpdateCursor::default(),
    })
}

pub(crate) fn watch_interfaces_blocking() -> Result<BlockingWatch, Error> {
    let (registration, queue) = register_queued_watcher()?;
    Ok(BlockingWatch {
        _registration: registration,
        queue,
        cursor: crate::UpdateCursor::default(),
    })
}

fn register_queued_watcher() -> Result<QueuedWatchRegistration, Error> {
    let queue = shared_async_callback_queue();
    let state = Box::pin(Mutex::new(QueuedWatchState {
        current_list: List::default(),
        queue: queue.clone(),
        initialising: true,
        disabled: false,
    }));
    let registration = register_notifications(
        state,
        Some(queued_unicast_notif),
        Some(queued_interface_notif),
    )?;

    let mut state_guard = registration.state().lock().unwrap();
    let initial_list = crate::list::list_interfaces()?;
    handle_initial_queued_notif(&mut state_guard, initial_list);
    drop(state_guard);

    Ok((registration, queue))
}

fn register_notifications<T: WatchStateLike>(
    state: Pin<Box<Mutex<T>>>,
    unicast_callback: PUNICAST_IPADDRESS_CHANGE_CALLBACK,
    interface_callback: PIPINTERFACE_CHANGE_CALLBACK,
) -> Result<NotificationRegistration<T>, Error> {
    let mut registration = NotificationRegistration::new(state);
    registration.register_unicast(unicast_callback)?;
    registration.register_interface(interface_callback)?;
    Ok(registration)
}

fn registration_result(result: WIN32_ERROR, handle: HANDLE) -> Result<NotificationHandle, Error> {
    match result {
        NO_ERROR => Ok(NotificationHandle(handle)),
        ERROR_INVALID_HANDLE => Err(Error::InvalidHandle),
        ERROR_INVALID_PARAMETER => Err(Error::InvalidParameter),
        ERROR_NOT_ENOUGH_MEMORY => Err(Error::NotEnoughMemory),
        _ => Err(Error::UnexpectedWindowsResult(result.0)),
    }
}

unsafe extern "system" fn unicast_notif(
    ctx: *const c_void,
    _row: *const MIB_UNICASTIPADDRESS_ROW,
    _notification_type: MIB_NOTIFICATION_TYPE,
) {
    dispatch_notif(ctx);
}

unsafe extern "system" fn interface_notif(
    ctx: *const c_void,
    _row: *const MIB_IPINTERFACE_ROW,
    _notification_type: MIB_NOTIFICATION_TYPE,
) {
    dispatch_notif(ctx);
}

fn dispatch_notif(ctx: *const c_void) {
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

unsafe extern "system" fn queued_unicast_notif(
    ctx: *const c_void,
    _row: *const MIB_UNICASTIPADDRESS_ROW,
    _notification_type: MIB_NOTIFICATION_TYPE,
) {
    dispatch_queued_notif(ctx);
}

unsafe extern "system" fn queued_interface_notif(
    ctx: *const c_void,
    _row: *const MIB_IPINTERFACE_ROW,
    _notification_type: MIB_NOTIFICATION_TYPE,
) {
    dispatch_queued_notif(ctx);
}

fn dispatch_queued_notif(ctx: *const c_void) {
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
    if state.initialising || state.disabled {
        return;
    }
    let Some(update) = state.cursor.advance(new_list) else {
        return;
    };
    state.callback.call_from_notification(update);
}

fn handle_queued_notif(state: &mut QueuedWatchState, new_list: List) {
    if state.initialising || state.disabled {
        return;
    }
    if new_list == state.current_list {
        return;
    }
    state.current_list = new_list.clone();
    push_async_list(&state.queue, new_list);
}

fn handle_initial_queued_notif(state: &mut QueuedWatchState, new_list: List) {
    state.current_list = new_list.clone();
    push_async_list(&state.queue, new_list);
    state.initialising = false;
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
            disabled: false,
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
