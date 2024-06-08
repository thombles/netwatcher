use crate::Update;

// The "objc2" project aims to provide bindings for all frameworks but Network.framework
// isn't ready yet so let's kick it old-school

struct nw_path_monitor;
type nw_path_monitor_t = *mut nw_path_monitor;
struct nw_path;
type nw_path_t = *mut nw_path;
struct dispatch_queue;
type dispatch_queue_t = *mut dispatch_queue;
const QOS_CLASS_BACKGROUND: usize = 0x09;

#[link(name = "Network", kind = "framework")]
extern "C" {
    fn nw_path_monitor_create() -> nw_path_monitor_t;
    fn nw_path_monitor_set_update_handler(
        monitor: nw_path_monitor_t,
        update_handler: &Block<dyn Fn(nw_path_t)>,
    );
    fn nw_path_monitor_set_queue(monitor: nw_path_monitor_t, queue: dispatch_queue_t);
    fn nw_path_monitor_start(monitor: nw_path_monitor_t);
    fn nw_path_monitor_cancel(monitor: nw_path_monitor_t);

    fn dispatch_get_global_queue(identifier: usize, flag: usize) -> dispatch_queue_t;
}

#[cfg(test)]
mod test {
    use super::list_interfaces;

    #[test]
    fn list() {
        let ifaces = list_interfaces().unwrap();
        println!("{:?}", ifaces);
    }
}

pub(crate) struct WatchHandle;

pub(crate) fn watch_interfaces<F: FnMut(Update) + 'static>(
    callback: F,
) -> Result<WatchHandle, Error> {
    // stop current worker thread
    // post this into a thread that will use it
    drop(callback);
    Ok(WatchHandle)
}
