use std::process::Command;

#[cfg(target_vendor = "apple")]
pub fn add_ipv6_to_interface(interface_name: &str, ip: &str) {
    println!("adding IPv6 address {ip} to {interface_name}");
    let result = Command::new("sudo")
        .args([
            "ifconfig",
            interface_name,
            "inet6",
            ip,
            "prefixlen",
            "128",
            "alias",
        ])
        .output()
        .expect("failed to execute ifconfig inet6 alias add");
    if !result.status.success() {
        panic!(
            "failed to add IPv6 address: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
}

#[cfg(target_vendor = "apple")]
pub fn remove_ipv6_from_interface(interface_name: &str, ip: &str) {
    println!("removing IPv6 address {ip} from {interface_name}");
    let result = Command::new("sudo")
        .args(["ifconfig", interface_name, "inet6", ip, "-alias"])
        .output()
        .expect("failed to execute ifconfig inet6 alias remove");
    if !result.status.success() {
        panic!(
            "failed to remove IPv6 address: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
}

#[cfg(target_os = "linux")]
pub fn add_ipv6_to_interface(interface_name: &str, ip: &str) {
    println!("adding IPv6 address {ip} to {interface_name}");
    let result = Command::new("sudo")
        .args([
            "ip",
            "-6",
            "addr",
            "add",
            &format!("{ip}/128"),
            "dev",
            interface_name,
        ])
        .output()
        .expect("failed to execute ip addr add command");
    if !result.status.success() {
        panic!(
            "failed to add IPv6 address: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
}

#[cfg(target_os = "linux")]
pub fn remove_ipv6_from_interface(interface_name: &str, ip: &str) {
    println!("removing IPv6 address {ip} from {interface_name}");
    let result = Command::new("sudo")
        .args([
            "ip",
            "-6",
            "addr",
            "del",
            &format!("{ip}/128"),
            "dev",
            interface_name,
        ])
        .output()
        .expect("failed to execute ip addr del command");
    if !result.status.success() {
        panic!(
            "failed to remove IPv6 address: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
}

#[cfg(all(
    unix,
    not(target_os = "linux"),
    not(target_vendor = "apple"),
    not(target_os = "android")
))]
pub fn add_ipv6_to_interface(interface_name: &str, ip: &str) {
    println!("adding IPv6 address {ip} to {interface_name}");
    let result = Command::new("ifconfig")
        .args([interface_name, "inet6", ip, "prefixlen", "128", "alias"])
        .output()
        .expect("failed to execute ifconfig inet6 alias add");
    if !result.status.success() {
        panic!(
            "failed to add IPv6 address: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
}

#[cfg(all(
    unix,
    not(target_os = "linux"),
    not(target_vendor = "apple"),
    not(target_os = "android")
))]
pub fn remove_ipv6_from_interface(interface_name: &str, ip: &str) {
    println!("removing IPv6 address {ip} from {interface_name}");
    let result = Command::new("ifconfig")
        .args([interface_name, "inet6", ip, "-alias"])
        .output()
        .expect("failed to execute ifconfig inet6 alias remove");
    if !result.status.success() {
        panic!(
            "failed to remove IPv6 address: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
}

#[cfg(windows)]
pub fn add_ipv6_to_interface(interface_name: &str, ip: &str) {
    println!("adding IPv6 address {ip} to {interface_name}");
    let result = Command::new("netsh")
        .args([
            "interface",
            "ipv6",
            "add",
            "address",
            interface_name,
            &format!("{ip}/128"),
        ])
        .output()
        .expect("failed to execute netsh add command");
    if !result.status.success() {
        panic!(
            "failed to add IPv6 address: {}",
            String::from_utf8_lossy(&result.stdout)
        );
    }
}

#[cfg(windows)]
pub fn remove_ipv6_from_interface(interface_name: &str, ip: &str) {
    println!("removing IPv6 address {ip} from {interface_name}");
    let result = Command::new("netsh")
        .args(["interface", "ipv6", "delete", "address", interface_name, ip])
        .output()
        .expect("failed to execute netsh delete command");
    if !result.status.success() {
        panic!(
            "failed to remove IPv6 address: {}",
            String::from_utf8_lossy(&result.stdout)
        );
    }
}
