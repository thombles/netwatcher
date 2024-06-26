use std::sync::Mutex;

use block2::{Block, RcBlock};
use objc2::Encoding;

use crate::{Error, List, Update};

// The "objc2" project aims to provide bindings for all frameworks but Network.framework
// isn't ready yet so let's kick it old-school

#[repr(C)]
struct NwPathMonitor([u8; 0]);
type NwPathMonitorT = *mut NwPathMonitor;
#[repr(C)]
struct NwPath([u8; 0]);
#[repr(C)]
struct DispatchQueue([u8; 0]);
type DispatchQueueT = *mut DispatchQueue;
const QOS_CLASS_BACKGROUND: usize = 0x09;

unsafe impl objc2::Encode for NwPath {
    const ENCODING: Encoding = usize::ENCODING;
}

#[link(name = "Network", kind = "framework")]
extern "C" {
    fn nw_path_monitor_create() -> NwPathMonitorT;
    fn nw_path_monitor_set_update_handler(
        monitor: NwPathMonitorT,
        update_handler: &Block<dyn Fn(NwPath)>,
    );
    fn nw_path_monitor_set_queue(monitor: NwPathMonitorT, queue: DispatchQueueT);
    fn nw_path_monitor_start(monitor: NwPathMonitorT);
    fn nw_path_monitor_cancel(monitor: NwPathMonitorT);

    fn dispatch_get_global_queue(identifier: usize, flag: usize) -> DispatchQueueT;
}

pub(crate) struct WatchHandle {
    path_monitor: NwPathMonitorT,
}

impl Drop for WatchHandle {
    fn drop(&mut self) {
        unsafe {
            nw_path_monitor_cancel(self.path_monitor);
        }
    }
}

struct CallbackState {
    prev_list: List,
    callback: Box<dyn FnMut(Update) + Send + 'static>,
}

pub(crate) fn watch_interfaces<F: FnMut(Update) + Send + 'static>(
    callback: F,
) -> Result<WatchHandle, Error> {
    let state = CallbackState {
        prev_list: List::default(),
        callback: Box::new(callback),
    };
    // Blocks are Fn, not FnMut
    let state = Mutex::new(state);
    let block = RcBlock::new(move |_: NwPath| {
        let mut state = state.lock().unwrap();
        let Ok(new_list) = crate::list::list_interfaces() else {
            return;
        };
        if new_list == state.prev_list {
            return;
        }
        let update = Update {
            interfaces: new_list.0.clone(),
            diff: new_list.diff_from(&state.prev_list),
        };
        (state.callback)(update);
        state.prev_list = new_list;
    });
    let path_monitor: NwPathMonitorT;
    unsafe {
        let queue = dispatch_get_global_queue(QOS_CLASS_BACKGROUND, 0);
        path_monitor = nw_path_monitor_create();
        nw_path_monitor_set_update_handler(path_monitor, &block);
        nw_path_monitor_set_queue(path_monitor, queue);
        nw_path_monitor_start(path_monitor);
    }
    Ok(WatchHandle { path_monitor })
}
