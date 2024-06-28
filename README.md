# netwatcher

[![Crates.io Version](https://img.shields.io/crates/v/netwatcher)](https://crates.io/crate/netwatcher)
[![docs.rs](https://img.shields.io/docsrs/netwatcher)](https://docs.rs/netwatcher)

`netwatcher` is a cross-platform Rust library for enumerating network interfaces and their IP addresses, featuring the ability to watch for changes to those interfaces _efficiently_. It uses platform-specific methods to detect when interface changes have occurred instead of polling, which means that you find out about changes more quickly and there is no CPU or wakeup overhead when nothing is happening.

## Current platform suport

| Platform | Min Version | List | Watch | Notes                                                                                 |
|----------|-------------|------|-------|---------------------------------------------------------------------------------------|
| Windows  | -           | ✅    | ✅     |                                                                                       |
| Mac      | 10.14       | ✅    | ✅     |                                                                                       |
| Linux    | -           | ✅    | ✅     | Creates a background thread                                                           |
| iOS      | 12.0        | ✅    | ✅     |                                                                                       |
| Android  | -           | ✅    | ❌     | Linux-style watch fails on Android 11+ due to privacy restrictions. Alternatives WIP. |

## Usage

### Listing interfaces

```rust
/// Returns a HashMap from ifindex (a `u32`) to an `Interface` struct
let interfaces = netwatcher::list_interfaces().unwrap();
for i in interfaces.values() {
    println!("interface {} has {} IPs", i.name, i.ips.len());
}
```

### Watching for changes to interfaces

```rust
let handle = netwatcher::watch_interfaces(|update| {
    // This callback will fire once immediately with the existing state

    // Update includes the latest snapshot of all interfaces
    println!("Current interface map: {:#?}", update.interfaces);

    // The `UpdateDiff` describes changes since previous callback
    // You can choose whether to use the snapshot, diff, or both
    println!("ifindexes added: {:?}", update.diff.added);
    println!("ifindexes removed: {:?}", update.diff.removed);
    for (ifindex, if_diff) in update.diff.modified {
        println!("Interface index {} has changed", ifindex);
        println!("Added IPs: {:?}", if_diff.addrs_added);
        println!("Removed IPs: {:?}", if_diff.addrs_removed);
    }
});
// keep `handle` alive as long as you want callbacks
// ...
drop(handle);
```

## Licence

Apache License Version 2.0 - see `LICENSE`.