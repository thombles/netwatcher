use netwatcher::{watch_interfaces, IpRecord, Update};
use std::net::{IpAddr, Ipv4Addr};
use std::process::Command;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

fn discover_loopback_interface() -> String {
    let output = Command::new("powershell")
        .args([
            "-Command",
            "(Get-NetIPAddress -IPAddress 127.0.0.1).InterfaceAlias",
        ])
        .output()
        .expect("failed to execute PS for Get-NetIPAddress");
    if !output.status.success() {
        panic!(
            "failed to get loopback interface: {}",
            String::from_utf8_lossy(&output.stdout)
        );
    }
    let interface_name = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if interface_name.is_empty() {
        panic!("could not find interface for 127.0.0.1");
    }
    interface_name
}

fn add_ip_to_interface(interface_name: &str, ip: &str) {
    println!("adding IP address {ip} to {interface_name}");
    let result = Command::new("netsh")
        .args([
            "interface",
            "ip",
            "add",
            "address",
            interface_name,
            ip,
            "255.0.0.0",
        ])
        .output()
        .expect("failed to execute netsh add command");
    if !result.status.success() {
        panic!(
            "failed to add IP address: {}",
            String::from_utf8_lossy(&result.stdout)
        );
    }
}

fn remove_ip_from_interface(interface_name: &str, ip: &str) {
    println!("removing IP address {ip} from {interface_name}");
    let result = Command::new("netsh")
        .args(["interface", "ip", "delete", "address", interface_name, ip])
        .output()
        .expect("failed to execute netsh delete command");
    if !result.status.success() {
        panic!(
            "failed to remove IP address: {}",
            String::from_utf8_lossy(&result.stdout)
        );
    }
}

fn setup_callback_handler() -> (
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

fn assert_has_ip(
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

#[test]
#[ignore] // needs to run in administrator context
fn test_watch_interfaces_loopback_changes() {
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
    assert_has_ip(&updates, 0, &expected_original, true);
    assert_has_ip(&updates, 0, &expected_added, false);

    // Add test IP and verify both addresses are present
    add_ip_to_interface(&loopback_interface, "127.0.0.10");
    wait_for_callback(2);
    assert_has_ip(&updates, 1, &expected_original, true);
    assert_has_ip(&updates, 1, &expected_added, true);

    // Remove test IP and verify only original remains
    remove_ip_from_interface(&loopback_interface, "127.0.0.10");
    wait_for_callback(3);
    assert_has_ip(&updates, 2, &expected_original, true);
    assert_has_ip(&updates, 2, &expected_added, false);
}
