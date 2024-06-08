use crate::Update;

pub struct WatchHandle;

pub fn watch_interfaces<F: FnMut(Update)>(callback: F) -> WatchHandle {
    drop(callback);
    WatchHandle
}
