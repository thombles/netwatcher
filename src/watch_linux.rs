use crate::Error;
use crate::Update;

pub(crate) struct WatchHandle;

pub(crate) fn watch_interfaces<F: FnMut(Update) + 'static>(
    callback: F,
) -> Result<WatchHandle, Error> {
    // stop current worker thread
    // post this into a thread that will use it
    drop(callback);
    Ok(WatchHandle)
}
