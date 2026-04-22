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
    empty_async_callback_queue, next_async_list, push_async_list, AsyncCallbackQueue,
};
use crate::Error;
use crate::List;
use crate::Update;

struct WatchState {
    /// The last result that we captured, for diffing
    prev_list: List,
    /// User's callback
    cb: Box<dyn FnMut(Update) + Send + 'static>,
}

pub(crate) struct WatchHandle {
    hnd: HANDLE,
    _state: Pin<Box<Mutex<WatchState>>>,
}

struct AsyncWatchState {
    current_list: List,
    queue: AsyncCallbackQueue,
}

pub(crate) struct AsyncWatch {
    hnd: HANDLE,
    queue: AsyncCallbackQueue,
    prev_list: List,
    _state: Pin<Box<Mutex<AsyncWatchState>>>,
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
            if new_list == self.prev_list {
                continue;
            }
            let update = new_list.update_from(&self.prev_list);
            self.prev_list = new_list;
            return update;
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

pub(crate) fn watch_interfaces<F: FnMut(Update) + Send + 'static>(
    callback: F,
) -> Result<WatchHandle, Error> {
    let state = Box::pin(Mutex::new(WatchState {
        prev_list: List::default(),
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
pub(crate) fn watch_interfaces_async<A: crate::AsyncFdAdapter>() -> Result<AsyncWatch, Error> {
    let current_list = crate::list::list_interfaces()?;
    let queue = empty_async_callback_queue();
    push_async_list(&queue, current_list.clone());
    let state = Box::pin(Mutex::new(AsyncWatchState {
        current_list,
        queue: queue.clone(),
    }));
    let state_ctx = &*state.as_ref() as *const _ as *const c_void;

    let mut hnd = HANDLE::default();
    let res = unsafe {
        NotifyUnicastIpAddressChange(
            AF_UNSPEC,
            Some(async_notif),
            Some(state_ctx),
            false,
            &mut hnd,
        )
    };
    match res {
        NO_ERROR => Ok(AsyncWatch {
            hnd,
            queue,
            prev_list: List::default(),
            _state: state,
        }),
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

unsafe extern "system" fn async_notif(
    ctx: *const c_void,
    _row: *const MIB_UNICASTIPADDRESS_ROW,
    _notification_type: MIB_NOTIFICATION_TYPE,
) {
    let state_ptr = ctx as *const Mutex<AsyncWatchState>;
    unsafe {
        let state_guard = &mut *state_ptr
            .as_ref()
            .expect("callback ctx should never be null")
            .lock()
            .unwrap();
        let Ok(new_list) = crate::list::list_interfaces() else {
            return;
        };
        handle_async_notif(state_guard, new_list);
    }
}

fn handle_notif(state: &mut WatchState, new_list: List) {
    if new_list == state.prev_list {
        return;
    }
    let update = new_list.update_from(&state.prev_list);
    (state.cb)(update);
    state.prev_list = new_list;
}

fn handle_async_notif(state: &mut AsyncWatchState, new_list: List) {
    if new_list == state.current_list {
        return;
    }
    state.current_list = new_list.clone();
    push_async_list(&state.queue, new_list);
}
