use netwatcher::{list_interfaces, IpRecord};
#[cfg(any(windows, all(unix, not(target_os = "android"))))]
use serial_test::serial;
#[cfg(any(windows, all(unix, not(target_os = "android"))))]
use std::net::Ipv6Addr;
use std::net::{IpAddr, Ipv4Addr};

#[cfg(any(windows, all(unix, not(target_os = "android"))))]
use netwatcher::{watch_interfaces_blocking, watch_interfaces_with_callback, Update, WatchHandle};
#[cfg(any(windows, all(unix, not(target_os = "android"))))]
use std::sync::{Arc, Condvar, Mutex};
#[cfg(any(windows, all(unix, not(target_os = "android"))))]
use std::time::Duration;

#[cfg(any(windows, all(unix, not(target_os = "android"))))]
mod helpers;

#[cfg(any(windows, all(unix, not(target_os = "android"))))]
#[path = "helpers/ipv6.rs"]
mod ipv6_helpers;

#[cfg(windows)]
#[path = "helpers/windows_interface.rs"]
mod windows_interface;

#[cfg(any(windows, all(unix, not(target_os = "android"))))]
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

#[cfg(any(windows, all(unix, not(target_os = "android"))))]
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

#[cfg(any(windows, all(unix, not(target_os = "android"))))]
fn assert_is_initial(updates: &Arc<Mutex<Vec<Update>>>, update_index: usize, expected: bool) {
    let updates_guard = updates.lock().unwrap();
    assert_eq!(updates_guard[update_index].is_initial, expected);
}

#[cfg(windows)]
fn wait_for_matching_update(
    receiver: &std::sync::mpsc::Receiver<Update>,
    description: &str,
    matches: impl Fn(&Update) -> bool,
) -> Update {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        let update = receiver
            .recv_timeout(remaining)
            .unwrap_or_else(|_| panic!("timeout waiting for {description}"));
        if matches(&update) {
            return update;
        }
    }
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
#[ignore] // installs a temporary network adapter and requires administrator context
#[cfg(windows)]
#[serial(loopback)]
fn test_watch_interfaces_interface_added_and_removed() {
    use windows_interface::TestInterface;

    let test_interface = TestInterface::install_disabled();
    let interface_name = test_interface.name().to_owned();
    let mut blocking_watch =
        watch_interfaces_blocking().expect("failed to create blocking watcher");
    let blocking_initial = blocking_watch.changed();
    assert!(blocking_initial.is_initial);
    assert!(
        blocking_initial
            .interfaces
            .values()
            .all(|interface| interface.name != interface_name),
        "disabled test interface unexpectedly appeared in blocking initial snapshot"
    );
    let blocking_interface_name = interface_name.clone();
    let (blocking_sender, blocking_receiver) = std::sync::mpsc::channel();
    let blocking_thread = std::thread::spawn(move || {
        let added = loop {
            let update = blocking_watch.changed();
            if update
                .diff
                .added
                .values()
                .any(|interface| interface.name == blocking_interface_name)
            {
                break update;
            }
        };
        let interface_index = added
            .diff
            .added
            .values()
            .find(|interface| interface.name == blocking_interface_name)
            .expect("matching blocking added interface should exist")
            .index;
        if blocking_sender.send(added).is_err() {
            return;
        }

        let removed = loop {
            let update = blocking_watch.changed();
            if update.diff.removed.contains_key(&interface_index) {
                break update;
            }
        };
        let _ = blocking_sender.send(removed);
    });

    let (sender, receiver) = std::sync::mpsc::channel();
    let handle = watch_interfaces_with_callback(move |update| {
        let _ = sender.send(update);
    })
    .expect("failed to create callback watcher");

    let initial = receiver
        .recv_timeout(Duration::from_secs(10))
        .expect("timeout waiting for initial update");
    assert!(initial.is_initial);
    assert!(
        initial
            .interfaces
            .values()
            .all(|interface| interface.name != interface_name),
        "disabled test interface unexpectedly appeared in initial snapshot"
    );

    test_interface.enable();
    let added = wait_for_matching_update(&receiver, "test interface to be added", |update| {
        update
            .diff
            .added
            .values()
            .any(|interface| interface.name == interface_name)
    });
    let added_interface = added
        .diff
        .added
        .values()
        .find(|interface| interface.name == interface_name)
        .expect("matching added interface should exist");
    assert!(
        added_interface.ips.is_empty(),
        "test interface unexpectedly acquired an address: {:?}",
        added_interface.ips
    );
    let interface_index = added_interface.index;
    let blocking_added = blocking_receiver
        .recv_timeout(Duration::from_secs(10))
        .expect("timeout waiting for blocking test interface addition");
    assert!(
        blocking_added.diff.added[&interface_index].ips.is_empty(),
        "blocking test interface unexpectedly acquired an address: {:?}",
        blocking_added.diff.added[&interface_index].ips
    );

    test_interface.disable();
    let removed = wait_for_matching_update(&receiver, "test interface to be removed", |update| {
        update.diff.removed.contains_key(&interface_index)
    });
    assert_eq!(removed.diff.removed[&interface_index].name, interface_name);
    let blocking_removed = blocking_receiver
        .recv_timeout(Duration::from_secs(10))
        .expect("timeout waiting for blocking test interface removal");
    assert_eq!(
        blocking_removed.diff.removed[&interface_index].name,
        interface_name
    );

    drop(handle);
    blocking_thread
        .join()
        .expect("blocking watcher thread should not panic");
}

#[test]
#[ignore] // needs to run in administrator/root context
#[cfg(any(windows, all(unix, not(target_os = "android"))))]
#[serial(loopback)]
fn test_watch_interfaces_callback_loopback_changes() {
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
#[cfg(any(windows, all(unix, not(target_os = "android"))))]
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

    let initial = watch.changed();
    assert!(initial.is_initial);
    assert_update_has_ip(&initial, &expected_original, true);
    assert_update_has_ip(&initial, &expected_added, false);

    add_ip_to_interface(&loopback_interface, "127.0.0.10");
    let added = watch.changed();
    assert!(!added.is_initial);
    assert_update_has_ip(&added, &expected_original, true);
    assert_update_has_ip(&added, &expected_added, true);

    remove_ip_from_interface(&loopback_interface, "127.0.0.10");
    let removed = watch.changed();
    assert!(!removed.is_initial);
    assert_update_has_ip(&removed, &expected_original, true);
    assert_update_has_ip(&removed, &expected_added, false);
}

#[test]
#[ignore] // needs to run in administrator/root context
#[cfg(any(windows, all(unix, not(target_os = "android"))))]
#[serial(loopback)]
fn test_watch_interfaces_callback_loopback_ipv6_changes() {
    use helpers::sys::discover_loopback_interface;

    let loopback_interface = discover_loopback_interface();
    println!("discovered loopback interface: '{loopback_interface}'");

    let expected_original = IpRecord {
        ip: IpAddr::V6(Ipv6Addr::LOCALHOST),
        prefix_len: 128,
    };
    let expected_added = IpRecord {
        ip: IpAddr::V6("2001:db8::2".parse().unwrap()),
        prefix_len: 128,
    };

    let (wait_for_callback, updates, _handle) = setup_callback_handler();

    // Wait for initial callback and verify initial state
    wait_for_callback(1);
    assert_is_initial(&updates, 0, true);
    assert_has_ip(&updates, 0, &expected_original, true);
    assert_has_ip(&updates, 0, &expected_added, false);

    // Add test IPv6 alias and verify both addresses are present
    ipv6_helpers::add_ipv6_to_interface(&loopback_interface, "2001:db8::2");
    wait_for_callback(2);
    assert_is_initial(&updates, 1, false);
    assert_has_ip(&updates, 1, &expected_original, true);
    assert_has_ip(&updates, 1, &expected_added, true);

    // Remove test IPv6 alias and verify only original remains
    ipv6_helpers::remove_ipv6_from_interface(&loopback_interface, "2001:db8::2");
    wait_for_callback(3);
    assert_is_initial(&updates, 2, false);
    assert_has_ip(&updates, 2, &expected_original, true);
    assert_has_ip(&updates, 2, &expected_added, false);
}

#[test]
#[ignore] // needs to run in administrator/root context
#[cfg(any(windows, all(unix, not(target_os = "android"))))]
#[serial(loopback)]
fn test_watch_interfaces_blocking_loopback_ipv6_changes() {
    use helpers::assert_update_has_ip;
    use helpers::sys::discover_loopback_interface;

    let loopback_interface = discover_loopback_interface();
    println!("discovered loopback interface: '{loopback_interface}'");

    let expected_original = IpRecord {
        ip: IpAddr::V6(Ipv6Addr::LOCALHOST),
        prefix_len: 128,
    };
    let expected_added = IpRecord {
        ip: IpAddr::V6("2001:db8::2".parse().unwrap()),
        prefix_len: 128,
    };

    let mut watch = watch_interfaces_blocking().expect("failed to create blocking watcher");

    let initial = watch.changed();
    assert!(initial.is_initial);
    assert_update_has_ip(&initial, &expected_original, true);
    assert_update_has_ip(&initial, &expected_added, false);

    ipv6_helpers::add_ipv6_to_interface(&loopback_interface, "2001:db8::2");
    let added = watch.changed();
    assert!(!added.is_initial);
    assert_update_has_ip(&added, &expected_original, true);
    assert_update_has_ip(&added, &expected_added, true);

    ipv6_helpers::remove_ipv6_from_interface(&loopback_interface, "2001:db8::2");
    let removed = watch.changed();
    assert!(!removed.is_initial);
    assert_update_has_ip(&removed, &expected_original, true);
    assert_update_has_ip(&removed, &expected_added, false);
}
