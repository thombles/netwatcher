# netwatcher

[![Crates.io Version](https://img.shields.io/crates/v/netwatcher)](https://crates.io/crates/netwatcher)
[![docs.rs](https://img.shields.io/docsrs/netwatcher)](https://docs.rs/netwatcher)

`netwatcher` is a cross-platform Rust library for enumerating network interfaces and their IP addresses, featuring the ability to watch for changes to those interfaces _efficiently_. It uses platform-specific methods to detect when interface changes have occurred instead of polling, which means that you find out about changes more quickly and there is no CPU or wakeup overhead when nothing is happening.

Sync and async APIs are available, with no extra dependencies for sync users. If you are using tokio, enable feature `tokio`. For async-io, enable `async-io`. Other reactors may be used by implementing the appropriate traits.

## Current platform support

| Platform | Min Version | List | Watch | Notes                                                                                 |
|----------|-------------|------|-------|---------------------------------------------------------------------------------------|
| Windows  | -           | ✅    | ✅     |                                                                                       |
| Mac      | -           | ✅    | ✅     | Callback watch creates background thread. |
| Linux    | -           | ✅    | ✅     | Callback watch creates background thread. |
| iOS      | -           | ✅    | ✅     | Callback watch creates background thread. |
| Android  | 5.0         | ✅    | ✅     | Watch requires extra setup. See Android Setup instructions below. |
| BSD  | - | ✅ | ✅ | FreeBSD 15.0 is tested in CI. Callback watch creates background thread. |

## Usage

### Listing interfaces

Use [`list_interfaces`](https://docs.rs/netwatcher/latest/netwatcher/fn.list_interfaces.html).

```rust
// Returns a HashMap from ifindex (a `u32`) to an `Interface` struct.
let interfaces = netwatcher::list_interfaces().unwrap();
for i in interfaces.values() {
    println!("interface {} has {} IPs", i.name, i.ips.len());
}
```

### Watching for changes to interfaces

Choose one of the three options:

- **Sync callback:** [`watch_interfaces_with_callback`](https://docs.rs/netwatcher/latest/netwatcher/fn.watch_interfaces_with_callback.html)
- **Sync blocking:** [`watch_interfaces_blocking`](https://docs.rs/netwatcher/latest/netwatcher/fn.watch_interfaces_blocking.html)
- **Async:** [`watch_interfaces_async::<T>`](https://docs.rs/netwatcher/latest/netwatcher/fn.watch_interfaces_async.html)

#### Sync callback watch

Deliver change notifications to a callback.

```rust
let handle = netwatcher::watch_interfaces_with_callback(|update| {
    // All watch types will fire immediately with initial interface state
    println!("Is initial update: {}", update.is_initial);
    println!("Current interface map: {:#?}", update.interfaces);

    // Added and removed entries contain the complete interface state.
    for interface in update.diff.added.values() {
        println!(
            "new interface: {} (ifindex {})",
            interface.name, interface.index
        );
    }
    for interface in update.diff.removed.values() {
        println!(
            "removed interface: {} (ifindex {})",
            interface.name, interface.index
        );
    }

    // These include addresses on entirely added or removed interfaces.
    for (ifindex, addr) in update.addrs_added() {
        println!("ifindex {} gained {}/{}", ifindex, addr.ip, addr.prefix_len);
    }
    for (ifindex, addr) in update.addrs_removed() {
        println!("ifindex {} lost {}/{}", ifindex, addr.ip, addr.prefix_len);
    }
})
.unwrap();

// Keep `handle` alive as long as you want callbacks.
// ...
drop(handle);
```

#### Sync blocking watch

Park the current thread until a change notification is available.

```rust,no_run
let mut watch = netwatcher::watch_interfaces_blocking().unwrap();

loop {
    let update = watch.changed();
    println!("Initial update: {}", update.is_initial);
    println!("Current interface map: {:#?}", update.interfaces);
}
```

#### Async watch

`.await` interface changes. This requires a small amount of integration with your async runtime. You will probably want to enable a crate feature such as `tokio` or `async-io` to use the provided adapter.

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
