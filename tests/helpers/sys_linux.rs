use std::process::Command;

pub fn discover_loopback_interface() -> String {
    "lo".to_string()
}

pub fn add_ip_to_interface(interface_name: &str, ip: &str) {
    println!("adding IP address {ip} to {interface_name}");
    let result = Command::new("sudo")
        .args([
            "ip",
            "addr",
            "add",
            &format!("{ip}/8"),
            "dev",
            interface_name,
        ])
        .output()
        .expect("failed to execute ip addr add command");
    if !result.status.success() {
        panic!(
            "failed to add IP address: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
}

pub fn remove_ip_from_interface(interface_name: &str, ip: &str) {
    println!("removing IP address {ip} from {interface_name}");
    let result = Command::new("sudo")
        .args([
            "ip",
            "addr",
            "del",
            &format!("{ip}/8"),
            "dev",
            interface_name,
        ])
        .output()
        .expect("failed to execute ip addr del command");
    if !result.status.success() {
        panic!(
            "failed to remove IP address: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
}
