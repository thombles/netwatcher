use crate::Update;

pub struct WatchHandle;

pub fn watch_interfaces<F: FnMut(Update)>(callback: F) -> WatchHandle {
    // stop current worker thread
    // post this into a thread that will use it
    drop(callback);
    WatchHandle
}
