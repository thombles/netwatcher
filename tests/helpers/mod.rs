use netwatcher::{watch_interfaces, IpRecord, Update};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

#[cfg(target_os = "windows")]
#[path = "sys_windows.rs"]
pub mod sys;

#[cfg(target_os = "linux")]
#[path = "sys_linux.rs"]
pub mod sys;

#[cfg(target_vendor = "apple")]
#[path = "sys_mac.rs"]
pub mod sys;

pub fn setup_callback_handler() -> (
    impl Fn(usize) + 'static,
    Arc<Mutex<Vec<Update>>>,
    netwatcher::WatchHandle,
) {
    let updates = Arc::new(Mutex::new(Vec::<Update>::new()));
    let updates_1 = updates.clone();
    let updates_2 = updates.clone();

    let callback_received = Arc::new(Condvar::new());
    let callback_received_1 = callback_received.clone();

    let handle = watch_interfaces(move |update| {
        let mut updates_guard = updates_1.lock().unwrap();
        updates_guard.push(update);
        let count = updates_guard.len();
        println!(
            "callback #{}: received update with {} interfaces",
            count,
            updates_guard.last().unwrap().interfaces.len()
        );
        drop(updates_guard);
        callback_received_1.notify_one();
    })
    .expect("failed to create watcher");

    let wait_for_callback = move |expected_count: usize| {
        let mut updates_guard = updates.lock().unwrap();
        while updates_guard.len() < expected_count {
            let result = callback_received
                .wait_timeout(updates_guard, Duration::from_secs(10))
                .unwrap();
            updates_guard = result.0;
            if result.1.timed_out() {
                panic!("timeout waiting for callback #{expected_count}");
            }
        }
    };

    (wait_for_callback, updates_2, handle)
}

pub fn assert_has_ip(
    updates: &Arc<Mutex<Vec<Update>>>,
    update_index: usize,
    ip_record: &IpRecord,
    should_have: bool,
) {
    let updates_guard = updates.lock().unwrap();
    let update = &updates_guard[update_index];
    let has_ip = update
        .interfaces
        .values()
        .any(|interface| interface.ips.contains(ip_record));

    if should_have {
        assert!(
            has_ip,
            "should have {}/{}",
            ip_record.ip, ip_record.prefix_len
        );
    } else {
        assert!(
            !has_ip,
            "should not have {}/{}",
            ip_record.ip, ip_record.prefix_len
        );
    }
}
