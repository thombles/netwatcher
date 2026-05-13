use std::{
    net::{IpAddr, Ipv4Addr},
    process::Command,
};

pub fn discover_loopback_interface() -> String {
    let loopback_ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
    let interfaces = netwatcher::list_interfaces().expect("failed to list network interfaces");
    interfaces
        .values()
        .find(|interface| {
            interface
                .ips
                .iter()
                .any(|ip_record| ip_record.ip == loopback_ip)
        })
        .map(|interface| interface.name.clone())
        .expect("could not find interface for 127.0.0.1")
}

pub fn add_ip_to_interface(interface_name: &str, ip: &str) {
    println!("adding IP address {ip} to {interface_name}");
    let result = Command::new("ifconfig")
        .args([interface_name, "inet", ip, "netmask", "255.0.0.0", "alias"])
        .output()
        .expect("failed to execute ifconfig alias add");
    if !result.status.success() {
        panic!(
            "failed to add IP address: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
}

pub fn remove_ip_from_interface(interface_name: &str, ip: &str) {
    println!("removing IP address {ip} from {interface_name}");
    let result = Command::new("ifconfig")
        .args([interface_name, "inet", ip, "-alias"])
        .output()
        .expect("failed to execute ifconfig alias remove");
    if !result.status.success() {
        panic!(
            "failed to remove IP address: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
}
