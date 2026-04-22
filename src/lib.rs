//! # netwatcher
//!
//! `netwatcher` is a cross-platform library for enumerating network interfaces and their
//! IP addresses, featuring the ability to watch for changes to those interfaces
//! _efficiently_. It uses platform-specific methods to detect when interface changes
//! have occurred instead of polling, which means that you find out about changes more
//! quickly and there is no CPU or wakeup overhead when nothing is happening.
//!
//! ## List example
//!
//! ```
//! // Returns a HashMap from ifindex (a `u32`) to an `Interface` struct.
//! let interfaces = netwatcher::list_interfaces().unwrap();
//! for i in interfaces.values() {
//!     println!("interface {}", i.name);
//!     for ip_record in &i.ips {
//!         println!("IP: {}/{}", ip_record.ip, ip_record.prefix_len);
//!     }
//! }
//! ```
//!
//! ## Watch options
//!
//! - `watch_interfaces_with_callback`: simplest callback-based API. On Linux and Apple
//!   platforms this creates a background thread.
//! - `watch_interfaces_blocking`: waits in the current thread until there is an interface
//!   change.
//! - `watch_interfaces_async::<T>`: allows you to `.await` interface changes by integrating
//     with an async runtime adapter such as `Tokio` or `AsyncIo`.
//!
//! ### Callback watch example
//!
//! ```no_run
//! let handle = netwatcher::watch_interfaces_with_callback(|update| {
//!     println!("Initial update: {}", update.is_initial);
//!     println!("Current interface map: {:#?}", update.interfaces);
//!
//!     // Interfaces may appear or disappear entirely.
//!     for ifindex in &update.diff.added {
//!         println!("ifindex {} was added", ifindex);
//!     }
//!     for ifindex in &update.diff.removed {
//!         println!("ifindex {} was removed", ifindex);
//!     }
//!
//!     // Existing interfaces may gain or lose IPs.
//!     for (ifindex, diff) in &update.diff.modified {
//!         let interface = &update.interfaces[ifindex];
//!         for addr in &diff.addrs_added {
//!             println!("{} gained {}/{}", interface.name, addr.ip, addr.prefix_len);
//!         }
//!         for addr in &diff.addrs_removed {
//!             println!("{} lost {}/{}", interface.name, addr.ip, addr.prefix_len);
//!         }
//!     }
//! })
//! .unwrap();
//!
//! // Keep `handle` alive as long as you want callbacks.
//! // ...
//! drop(handle);
//! ```
//!
//! ### Blocking watch example
//!
//! `changed()` waits forever if nothing changes, so it is intended for a thread or program
//! that has no other work to do until an interface change arrives.
//!
//! ```no_run
//! let mut watch = netwatcher::watch_interfaces_blocking().unwrap();
//!
//! loop {
//!     let update = watch.changed();
//!     println!("Initial update: {}", update.is_initial);
//!     println!("Current interface map: {:#?}", update.interfaces);
//! }
//! ```
//!
//! ### Async watch example
//!
//! You will probably want to enable a crate feature such as `tokio` or `async-io` in order
//! to use the adapter appropriate for your async runtime.
//!
//! ```no_run
//! # #[cfg(all(target_os = "linux", feature = "tokio"))]
//! # {
//! use netwatcher::async_adapter::Tokio;
//!
//! let runtime = tokio::runtime::Builder::new_current_thread()
//!     .enable_all()
//!     .build()
//!     .unwrap();
//! runtime.block_on(async {
//!     let mut watch = netwatcher::watch_interfaces_async::<Tokio>().unwrap();
//!     loop {
//!         let update = watch.changed().await;
//!         println!("Initial update: {}", update.is_initial);
//!         println!("Current interface map: {:#?}", update.interfaces);
//!     }
//! });
//! # }
//! ```

use std::{
    collections::{HashMap, HashSet},
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    ops::Sub,
};

mod error;

#[cfg(any(windows, target_os = "android"))]
mod async_callback;

#[cfg(any(target_os = "linux", target_vendor = "apple"))]
mod watch_fd;

#[cfg_attr(windows, path = "list_win.rs")]
#[cfg_attr(unix, path = "list_unix.rs")]
mod list;

#[cfg(target_os = "android")]
mod android;

#[cfg_attr(windows, path = "watch_win.rs")]
#[cfg_attr(target_vendor = "apple", path = "watch_mac.rs")]
#[cfg_attr(target_os = "linux", path = "watch_linux.rs")]
#[cfg_attr(target_os = "android", path = "watch_android.rs")]
mod watch;

pub mod async_adapter;

type IfIndex = u32;

pub use error::Error;

#[cfg(target_os = "android")]
pub use android::set_android_context;

/// An IP address paired with its prefix length (network mask).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IpRecord {
    pub ip: IpAddr,
    pub prefix_len: u8,
}

/// Information about one network interface at a point in time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Interface {
    /// Internal index identifying this interface.
    pub index: u32,
    /// Interface name.
    pub name: String,
    /// Hardware address. Android may have a placeholder due to privacy restrictions.
    pub hw_addr: String,
    /// List of associated IPs and prefix length (netmask).
    pub ips: Vec<IpRecord>,
}

impl Interface {
    /// Helper to iterate over only the IPv4 addresses on this interface.
    pub fn ipv4_ips(&self) -> impl Iterator<Item = &Ipv4Addr> {
        self.ips.iter().filter_map(|ip_record| match ip_record.ip {
            IpAddr::V4(ref v4) => Some(v4),
            IpAddr::V6(_) => None,
        })
    }

    /// Helper to iterate over only the IPv6 addresses on this interface.
    pub fn ipv6_ips(&self) -> impl Iterator<Item = &Ipv6Addr> {
        self.ips.iter().filter_map(|ip_record| match ip_record.ip {
            IpAddr::V4(_) => None,
            IpAddr::V6(ref v6) => Some(v6),
        })
    }
}

/// Information delivered when a network interface snapshot changes.
///
/// This contains up-to-date information about all interfaces, plus a diff which
/// details which interfaces and IP addresses have changed since the previous update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Update {
    /// Whether this update represents the initial existing interface state.
    pub is_initial: bool,
    pub interfaces: HashMap<IfIndex, Interface>,
    pub diff: UpdateDiff,
}

/// What changed between one `Update` and the next.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UpdateDiff {
    pub added: Vec<IfIndex>,
    pub removed: Vec<IfIndex>,
    pub modified: HashMap<IfIndex, InterfaceDiff>,
}

/// What changed within a single interface between updates, if it was present in both.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InterfaceDiff {
    pub hw_addr_changed: bool,
    pub addrs_added: Vec<IpRecord>,
    pub addrs_removed: Vec<IpRecord>,
}

#[derive(Default, PartialEq, Eq, Clone)]
struct List(HashMap<IfIndex, Interface>);

impl List {
    fn initial_update(&self) -> Update {
        self.update_from_with_flag(&List::default(), true)
    }

    fn update_from(&self, prev: &List) -> Update {
        self.update_from_with_flag(prev, false)
    }

    fn update_from_with_flag(&self, prev: &List, is_initial: bool) -> Update {
        let prev_index_set: HashSet<IfIndex> = prev.0.keys().cloned().collect();
        let curr_index_set: HashSet<IfIndex> = self.0.keys().cloned().collect();
        let added = curr_index_set.sub(&prev_index_set).into_iter().collect();
        let removed = prev_index_set.sub(&curr_index_set).into_iter().collect();
        let mut modified = HashMap::new();
        for index in curr_index_set.intersection(&prev_index_set) {
            if prev.0[index] == self.0[index] {
                continue;
            }
            let prev_addr_set: HashSet<&IpRecord> = prev.0[index].ips.iter().collect();
            let curr_addr_set: HashSet<&IpRecord> = self.0[index].ips.iter().collect();
            let addrs_added: Vec<IpRecord> = curr_addr_set
                .sub(&prev_addr_set)
                .iter()
                .cloned()
                .cloned()
                .collect();
            let addrs_removed: Vec<IpRecord> = prev_addr_set
                .sub(&curr_addr_set)
                .iter()
                .cloned()
                .cloned()
                .collect();
            let hw_addr_changed = prev.0[index].hw_addr != self.0[index].hw_addr;
            modified.insert(
                *index,
                InterfaceDiff {
                    hw_addr_changed,
                    addrs_added,
                    addrs_removed,
                },
            );
        }
        Update {
            is_initial,
            interfaces: self.0.clone(),
            diff: UpdateDiff {
                added,
                removed,
                modified,
            },
        }
    }
}

struct UpdateCursor {
    prev_list: List,
    initial_pending: bool,
}

impl Default for UpdateCursor {
    fn default() -> Self {
        Self {
            prev_list: List::default(),
            initial_pending: true,
        }
    }
}

impl UpdateCursor {
    fn advance(&mut self, new_list: List) -> Option<Update> {
        if self.initial_pending {
            self.initial_pending = false;
            self.prev_list = new_list.clone();
            return Some(new_list.initial_update());
        }

        if new_list == self.prev_list {
            return None;
        }

        let update = new_list.update_from(&self.prev_list);
        self.prev_list = new_list;
        Some(update)
    }
}

/// A handle to keep alive as long as you wish to receive callbacks.
///
/// If the callback is executing at the time the handle is dropped, drop will block until
/// the callback is finished and it's guaranteed that it will not be called again.
///
/// Do not drop the handle from within the callback itself. It will probably deadlock.
pub struct WatchHandle {
    _inner: watch::WatchHandle,
}

/// A handle that yields `Update`s asynchronously when network interfaces change.
pub struct AsyncWatch {
    _inner: watch::AsyncWatch,
}

/// A handle that yields `Update`s synchronously when network interfaces change.
pub struct BlockingWatch {
    _inner: watch::BlockingWatch,
}

impl AsyncWatch {
    /// Wait for the next interface snapshot that differs from the last snapshot yielded.
    ///
    /// The first call returns the current interface snapshot immediately. Subsequent calls wait
    /// until there is a change.
    ///
    /// This method is infallible. Once a watch has been created successfully, later failures to
    /// read platform notifications or re-list interfaces are swallowed and no update is emitted
    /// for that event.
    pub async fn changed(&mut self) -> Update {
        self._inner.changed().await
    }
}

impl BlockingWatch {
    /// Wait for the next interface snapshot that differs from the last snapshot yielded.
    ///
    /// The first call returns the current interface snapshot immediately. Subsequent calls wait
    /// until there is a change.
    ///
    /// This method is infallible. Once a watch has been created successfully, later failures to
    /// read platform notifications or re-list interfaces are swallowed and no update is emitted
    /// for that event.
    pub fn changed(&mut self) -> Update {
        self._inner.changed()
    }
}

/// Retrieve information about all enabled network interfaces and their IP addresses.
///
/// This is a once-off operation. If you want to detect changes over time, see
/// `watch_interfaces_with_callback`, `watch_interfaces_blocking`, or `watch_interfaces_async`.
pub fn list_interfaces() -> Result<HashMap<IfIndex, Interface>, Error> {
    list::list_interfaces().map(|list| list.0)
}

/// Retrieve interface information and watch for changes, which will be delivered via callback.
///
/// If setting up the watch is successful, this returns a `WatchHandle` which must be kept for
/// as long as the provided callback should operate.
///
/// The callback will fire once immediately with an initial interface list, and a diff as if
/// there were originally no interfaces present.
///
/// This function will return an error if there is a problem configuring the watcher, or if there
/// is an error retrieving the initial interface list.
///
/// We assume that if listing the interfaces worked the first time, then it will continue to work
/// for as long as the watcher is running. If listing interfaces begins to fail later, those
/// failures will be swallowed and the callback will not be called for that change event.
pub fn watch_interfaces_with_callback<F: FnMut(Update) + Send + 'static>(
    callback: F,
) -> Result<WatchHandle, Error> {
    watch::watch_interfaces_with_callback(callback).map(|handle| WatchHandle { _inner: handle })
}

/// Retrieve interface information and watch for changes synchronously.
///
/// The first call to `changed()` returns the current interface snapshot immediately.
pub fn watch_interfaces_blocking() -> Result<BlockingWatch, Error> {
    watch::watch_interfaces_blocking().map(|handle| BlockingWatch { _inner: handle })
}

/// Retrieve interface information and watch for changes asynchronously using the given runtime adapter.
///
/// The first call to `changed()` returns the current interface snapshot immediately.
pub fn watch_interfaces_async<A: async_adapter::AsyncFdAdapter>() -> Result<AsyncWatch, Error> {
    watch::watch_interfaces_async::<A>().map(|handle| AsyncWatch { _inner: handle })
}
