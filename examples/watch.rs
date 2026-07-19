use std::time::Duration;

fn main() {
    println!("Watching for changes for 30 seconds...");

    let handle = netwatcher::watch_interfaces_with_callback(|update| {
        println!("Interface update!");
        println!("Initial: {}", update.is_initial);
        println!("State: {:#?}", update.interfaces);
        for interface in update.diff.added.values() {
            println!(
                "Added interface: {} (ifindex {})",
                interface.name, interface.index
            );
        }
        for interface in update.diff.removed.values() {
            println!(
                "Removed interface: {} (ifindex {})",
                interface.name, interface.index
            );
        }
        for (ifindex, address) in update.addrs_added() {
            println!(
                "Added address on ifindex {}: {}/{}",
                ifindex, address.ip, address.prefix_len
            );
        }
        for (ifindex, address) in update.addrs_removed() {
            println!(
                "Removed address from ifindex {}: {}/{}",
                ifindex, address.ip, address.prefix_len
            );
        }
    })
    .unwrap();

    std::thread::sleep(Duration::from_secs(30));

    drop(handle);
    println!("Stopped watching! Program will end in 30 seconds.");

    std::thread::sleep(Duration::from_secs(30));
}
