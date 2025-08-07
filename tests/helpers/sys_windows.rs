use std::process::Command;

pub fn discover_loopback_interface() -> String {
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

pub fn add_ip_to_interface(interface_name: &str, ip: &str) {
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

pub fn remove_ip_from_interface(interface_name: &str, ip: &str) {
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
