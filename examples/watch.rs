use std::time::Duration;

fn main() {
    println!("Watching for changes for 30 seconds...");

    let handle = netwatcher::watch_interfaces(|update| {
        println!("Interface update!");
        println!("State: {:#?}", update.interfaces);
        println!("Diff: {:#?}", update.diff);
    })
    .unwrap();

    std::thread::sleep(Duration::from_secs(30));

    drop(handle);
    println!("Stopped watching! Program will end in 30 seconds.");

    std::thread::sleep(Duration::from_secs(30));
}
