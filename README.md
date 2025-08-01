# netwatcher

[![Crates.io Version](https://img.shields.io/crates/v/netwatcher)](https://crates.io/crates/netwatcher)
[![docs.rs](https://img.shields.io/docsrs/netwatcher)](https://docs.rs/netwatcher)

`netwatcher` is a cross-platform Rust library for enumerating network interfaces and their IP addresses, featuring the ability to watch for changes to those interfaces _efficiently_. It uses platform-specific methods to detect when interface changes have occurred instead of polling, which means that you find out about changes more quickly and there is no CPU or wakeup overhead when nothing is happening.

## Current platform support

| Platform | Min Version | List | Watch | Notes                                                                                 |
|----------|-------------|------|-------|---------------------------------------------------------------------------------------|
| Windows  | -           | ✅    | ✅     |                                                                                       |
| Mac      | 10.14       | ✅    | ✅     |                                                                                       |
| Linux    | -           | ✅    | ✅     | Creates a background thread                                                           |
| iOS      | 12.0        | ✅    | ✅     |                                                                                       |
| Android  | 8.0         | ✅    | ✅     | Watch support requires extra setup. See Android Setup instruction below.              |

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

### Android Setup

Ensure the app module which is going to end up running `netwatcher` has these permissions:

```xml
    <uses-permission android:name="android.permission.ACCESS_NETWORK_STATE" />
    <uses-permission android:name="android.permission.INTERNET" />
```

You will also need to make sure that `netwatcher` gets access to the Android app's `Context`. There is built-in support for the [ndk-context](https://crates.io/crates/ndk-context) crate. What this means is that if you're using certain frameworks for building all-Rust Android apps then it will be able to pick up the context automatically. In other situations, the Rust code in your app will have to call `netwatcher::set_android_context`.

There is a test app included in the repo that provides a full example. [MainActivity.kt](https://github.com/thombles/netwatcher/blob/main/android/app/src/main/java/net/octet_stream/netwatcher/netwatchertestapp/MainActivity.kt) is an activity with some methods defined in Rust. [app-native/src/lib.rs](https://github.com/thombles/netwatcher/blob/main/android/app-native/src/lib.rs) provides the native implementations of those methods. This includes an example of calling `set_android_context`, and using the `netwatcher` library to watch for interface changes, passing the results back to the Java GUI.

## Licence

MIT
