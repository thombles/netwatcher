use netwatcher::{list_interfaces, IpRecord};
#[cfg(any(target_os = "windows", target_os = "linux", target_vendor = "apple"))]
use serial_test::serial;
use std::net::{IpAddr, Ipv4Addr};

#[cfg(any(target_os = "windows", target_os = "linux", target_vendor = "apple"))]
use netwatcher::{watch_interfaces_blocking, watch_interfaces_with_callback, Update, WatchHandle};
#[cfg(any(target_os = "windows", target_os = "linux", target_vendor = "apple"))]
use std::sync::{Arc, Condvar, Mutex};
#[cfg(any(target_os = "windows", target_os = "linux", target_vendor = "apple"))]
use std::time::Duration;

#[cfg(any(target_os = "windows", target_os = "linux", target_vendor = "apple"))]
mod helpers;

#[cfg(any(target_os = "windows", target_os = "linux", target_vendor = "apple"))]
fn setup_callback_handler() -> (
    impl Fn(usize) + 'static,
    Arc<Mutex<Vec<Update>>>,
    WatchHandle,
) {
    let updates = Arc::new(Mutex::new(Vec::<Update>::new()));
    let updates_1 = updates.clone();
    let updates_2 = updates.clone();

    let callback_received = Arc::new(Condvar::new());
    let callback_received_1 = callback_received.clone();

    let handle = watch_interfaces_with_callback(move |update| {
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

#[cfg(any(target_os = "windows", target_os = "linux", target_vendor = "apple"))]
fn assert_has_ip(
    updates: &Arc<Mutex<Vec<Update>>>,
    update_index: usize,
    ip_record: &IpRecord,
    should_have: bool,
) {
    let updates_guard = updates.lock().unwrap();
    let update = &updates_guard[update_index];
    helpers::assert_update_has_ip(update, ip_record, should_have);
}

#[cfg(any(target_os = "windows", target_os = "linux", target_vendor = "apple"))]
fn assert_is_initial(updates: &Arc<Mutex<Vec<Update>>>, update_index: usize, expected: bool) {
    let updates_guard = updates.lock().unwrap();
    assert_eq!(updates_guard[update_index].is_initial, expected);
}

#[test]
fn test_list_interfaces_has_loopback() {
    let interfaces = list_interfaces().expect("failed to list network interfaces");
    assert!(!interfaces.is_empty(), "no network interfaces found");

    let expected_loopback = IpRecord {
        ip: IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
        prefix_len: 8,
    };
    let loopback_found = interfaces
        .values()
        .any(|interface| interface.ips.contains(&expected_loopback));

    assert!(loopback_found, "address 127.0.0.1/8 not found");
}

#[test]
#[ignore] // needs to run in administrator/root context
#[cfg(any(target_os = "windows", target_os = "linux", target_vendor = "apple"))]
#[serial(loopback)]
fn test_watch_interfaces_loopback_changes() {
    use helpers::sys::*;

    let loopback_interface = discover_loopback_interface();
    println!("discovered loopback interface: '{loopback_interface}'");

    let expected_original = IpRecord {
        ip: IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
        prefix_len: 8,
    };
    let expected_added = IpRecord {
        ip: IpAddr::V4(Ipv4Addr::new(127, 0, 0, 10)),
        prefix_len: 8,
    };

    let (wait_for_callback, updates, _handle) = setup_callback_handler();

    // Wait for initial callback and verify initial state
    wait_for_callback(1);
    assert_is_initial(&updates, 0, true);
    assert_has_ip(&updates, 0, &expected_original, true);
    assert_has_ip(&updates, 0, &expected_added, false);

    // Add test IP and verify both addresses are present
    add_ip_to_interface(&loopback_interface, "127.0.0.10");
    wait_for_callback(2);
    assert_is_initial(&updates, 1, false);
    assert_has_ip(&updates, 1, &expected_original, true);
    assert_has_ip(&updates, 1, &expected_added, true);

    // Remove test IP and verify only original remains
    remove_ip_from_interface(&loopback_interface, "127.0.0.10");
    wait_for_callback(3);
    assert_is_initial(&updates, 2, false);
    assert_has_ip(&updates, 2, &expected_original, true);
    assert_has_ip(&updates, 2, &expected_added, false);
}

#[test]
#[ignore] // needs to run in administrator/root context
#[cfg(any(target_os = "windows", target_os = "linux", target_vendor = "apple"))]
#[serial(loopback)]
fn test_watch_interfaces_blocking_loopback_changes() {
    use helpers::assert_update_has_ip;
    use helpers::sys::*;

    let loopback_interface = discover_loopback_interface();
    println!("discovered loopback interface: '{loopback_interface}'");

    let expected_original = IpRecord {
        ip: IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
        prefix_len: 8,
    };
    let expected_added = IpRecord {
        ip: IpAddr::V4(Ipv4Addr::new(127, 0, 0, 10)),
        prefix_len: 8,
    };

    let mut watch = watch_interfaces_blocking().expect("failed to create blocking watcher");

    let initial = watch.updated();
    assert!(initial.is_initial);
    assert_update_has_ip(&initial, &expected_original, true);
    assert_update_has_ip(&initial, &expected_added, false);

    add_ip_to_interface(&loopback_interface, "127.0.0.10");
    let added = watch.updated();
    assert!(!added.is_initial);
    assert_update_has_ip(&added, &expected_original, true);
    assert_update_has_ip(&added, &expected_added, true);

    remove_ip_from_interface(&loopback_interface, "127.0.0.10");
    let removed = watch.updated();
    assert!(!removed.is_initial);
    assert_update_has_ip(&removed, &expected_original, true);
    assert_update_has_ip(&removed, &expected_added, false);
}
