use std::time::Duration;

fn main() {
    let _handle = netwatcher::watch_interfaces(|update| {
        println!("Interface update!");
        println!("State: {:?}", update.interfaces);
        println!("Diff: {:?}", update.diff);
    });

    loop {
        std::thread::sleep(Duration::from_secs(60));
    }
}
