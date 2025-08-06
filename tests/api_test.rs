use netwatcher::{list_interfaces, IpRecord};
use std::net::{IpAddr, Ipv4Addr};

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
