use netwatcher::{list_interfaces, IpRecord};
use std::net::{IpAddr, Ipv4Addr};

#[cfg(any(target_os = "windows", target_os = "linux"))]
mod helpers;

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
#[cfg(any(target_os = "windows", target_os = "linux"))]
fn test_watch_interfaces_loopback_changes() {
    use helpers::sys::*;
    use helpers::*;

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
