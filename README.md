# netwatcher

[![Crates.io Version](https://img.shields.io/crates/v/netwatcher)](https://crates.io/crates/netwatcher)
[![docs.rs](https://img.shields.io/docsrs/netwatcher)](https://docs.rs/netwatcher)

`netwatcher` is a cross-platform Rust library for enumerating network interfaces and their IP addresses, featuring the ability to watch for changes to those interfaces _efficiently_. It uses platform-specific methods to detect when interface changes have occurred instead of polling, which means that you find out about changes more quickly and there is no CPU or wakeup overhead when nothing is happening.

## Current platform support

| Platform | Min Version | List | Watch | Notes                                                                                 |
|----------|-------------|------|-------|---------------------------------------------------------------------------------------|
| Windows  | -           | ✅    | ✅     |                                                                                       |
| Mac      | -           | ✅    | ✅     | Callback watch creates background thread                                                       |
| Linux    | -           | ✅    | ✅     | Callback watch creates background thread                                                       |
| iOS      | -           | ✅    | ✅     | Callback watch creates background thread                                                       |
| Android  | 5.0         | ✅    | ✅     | Watch requires extra setup. See Android Setup instructions below.             |

## Usage

### Listing interfaces

```rust
// Returns a HashMap from ifindex (a `u32`) to an `Interface` struct.
let interfaces = netwatcher::list_interfaces().unwrap();
for i in interfaces.values() {
    println!("interface {} has {} IPs", i.name, i.ips.len());
}
```

### Watching for changes to interfaces

Choose one of the three watch APIs:

- `watch_interfaces_with_callback`: easiest when you want interface changes pushed into a callback. On macOS, Linux, and iOS this creates a background thread.
- `watch_interfaces_blocking`: waits in the current thread until there is a change. If nothing changes, `updated()` never returns, so this is best for a dedicated thread or a program with no other work to do until interfaces change.
- `watch_interfaces_async::<T>`: allows you to `.await` interface changes by integrating with an async runtime adapter such as `Tokio` or `AsyncIo`.

#### Callback watch

This is the simplest option when you want change notifications delivered to a callback.

```rust
let handle = netwatcher::watch_interfaces_with_callback(|update| {
    // All watch types will fire immediately with initial interface state
    println!("Is initial update: {}", update.is_initial);
    println!("Current interface map: {:#?}", update.interfaces);

    // Interfaces may appear or disappear entirely.
    for ifindex in &update.diff.added {
        println!("ifindex {} was added", ifindex);
    }
    for ifindex in &update.diff.removed {
        println!("ifindex {} was removed", ifindex);
    }

    // Existing interfaces may gain or lose IPs.
    for (ifindex, diff) in &update.diff.modified {
        let interface = &update.interfaces[ifindex];
        for addr in &diff.addrs_added {
            println!("{} gained {}/{}", interface.name, addr.ip, addr.prefix_len);
        }
        for addr in &diff.addrs_removed {
            println!("{} lost {}/{}", interface.name, addr.ip, addr.prefix_len);
        }
    }
})
.unwrap();

// Keep `handle` alive as long as you want callbacks.
// ...
drop(handle);
```

#### Blocking watch

This waits in the current thread until an update is available.

```rust,no_run
let mut watch = netwatcher::watch_interfaces_blocking().unwrap();

loop {
    let update = watch.updated();
    println!("Initial update: {}", update.is_initial);
    println!("Current interface map: {:#?}", update.interfaces);
}
```

#### Async watch

This integrates with your async runtime. On macOS, Linux, and iOS it avoids creating a dedicated background thread for the watcher.

```rust,no_run
use netwatcher::async_adapter::Tokio;

let runtime = tokio::runtime::Builder::new_current_thread()
    .enable_all()
    .build()
    .unwrap();

runtime.block_on(async {
    let mut watch = netwatcher::watch_interfaces_async::<Tokio>().unwrap();

    loop {
        let update = watch.changed().await;
        println!("Initial update: {}", update.is_initial);
        println!("Current interface map: {:#?}", update.interfaces);
    }
});
```

### Android Setup

Ensure the app module which is going to end up running `netwatcher` has these permissions:

```xml
    <uses-permission android:name="android.permission.ACCESS_NETWORK_STATE" />
    <uses-permission android:name="android.permission.INTERNET" />
```

You will also need to make sure that `netwatcher` gets access to the Android app's `Context`. There is built-in support for the [ndk-context](https://crates.io/crates/ndk-context) crate. What this means is that if you're using certain frameworks for building all-Rust Android apps then it will be able to pick up the context automatically. In other situations, the Rust code in your app will have to call `netwatcher::set_android_context` ([example code](https://github.com/thombles/netwatcher/blob/b58d2283f5a3f7a5c324946ba8e92407c0d8a2dd/android/app-native/src/lib.rs#L32-L44)).

There is a test app included in the repo that provides a full example. [MainActivity.kt](https://github.com/thombles/netwatcher/blob/main/android/app/src/main/java/net/octet_stream/netwatcher/netwatchertestapp/MainActivity.kt) is an activity with some methods defined in Rust. [app-native/src/lib.rs](https://github.com/thombles/netwatcher/blob/main/android/app-native/src/lib.rs) provides the native implementations of those methods. This includes an example of calling `set_android_context`, and using the `netwatcher` library to watch for interface changes, passing the results back to the Java GUI.

## Licence

MIT
