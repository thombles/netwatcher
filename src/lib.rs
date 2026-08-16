//! # netwatcher
//!
//! `netwatcher` is a cross-platform Rust library for enumerating network interfaces and their
//! IP addresses, featuring the ability to watch for changes to those interfaces
//! _efficiently_. It uses platform-specific methods to detect when interface changes
//! have occurred instead of polling, which means that you find out about changes more
//! quickly and there is no CPU or wakeup overhead when nothing is happening.
//!
//! Sync and async APIs are available, with no extra dependencies for sync users. If you are
//! using tokio, enable feature `tokio`. For async-io, enable `async-io`. Other reactors may
//! be used by implementing the appropriate traits.
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
//! - **Sync callback:** [`watch_interfaces_with_callback`](https://docs.rs/netwatcher/latest/netwatcher/fn.watch_interfaces_with_callback.html)
//! - **Sync blocking:** [`watch_interfaces_blocking`](https://docs.rs/netwatcher/latest/netwatcher/fn.watch_interfaces_blocking.html)
//! - **Async:** [`watch_interfaces_async::<T>`](https://docs.rs/netwatcher/latest/netwatcher/fn.watch_interfaces_async.html)
//!
//! ### Sync callback watch
//!
//! Deliver change notifications to a callback.
//!
//! ```no_run
//! let handle = netwatcher::watch_interfaces_with_callback(|update| {
//!     // All watch types will fire immediately with initial interface state
//!     println!("Is initial update: {}", update.is_initial);
//!     println!("Current interface map: {:#?}", update.interfaces);
//!
//!     // Added and removed entries contain the complete interface state.
//!     for interface in update.diff.added.values() {
//!         println!(
//!             "new interface: {} (ifindex {})",
//!             interface.name, interface.index
//!         );
//!     }
//!     for interface in update.diff.removed.values() {
//!         println!(
//!             "removed interface: {} (ifindex {})",
//!             interface.name, interface.index
//!         );
//!     }
//!
//!     // These include addresses on entirely added or removed interfaces.
//!     for (ifindex, addr) in update.addrs_added() {
//!         println!("ifindex {} gained {}/{}", ifindex, addr.ip, addr.prefix_len);
//!     }
//!     for (ifindex, addr) in update.addrs_removed() {
//!         println!("ifindex {} lost {}/{}", ifindex, addr.ip, addr.prefix_len);
//!     }
//! })
//! .unwrap();
//!
//! // Keep `handle` alive as long as you want callbacks.
//! // ...
//! drop(handle);
//! ```
//!
//! ### Sync blocking watch
//!
//! Park the current thread until a change notification is available.
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
//! ### Async watch
//!
//! `.await` interface changes. This requires a small amount of integration with your async
//! runtime. You will probably want to enable a crate feature such as `tokio` or `async-io`
//! to use the provided adapter.
//!
//! ```no_run
//! # #[cfg(feature = "tokio")]
//! # {
//! use netwatcher::async_adapter::Tokio;
//!
//! let runtime = tokio::runtime::Builder::new_current_thread()
//!     .enable_all()
//!     .build()
//!     .unwrap();
//!
//! runtime.block_on(async {
//!     let mut watch = netwatcher::watch_interfaces_async::<Tokio>().unwrap();
//!
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
};

mod error;

#[cfg(any(windows, target_os = "android", test))]
mod callback;

#[cfg(any(windows, target_os = "android"))]
mod async_callback;

#[cfg(all(unix, not(target_os = "android")))]
mod watch_fd;

#[cfg_attr(windows, path = "list_win.rs")]
#[cfg_attr(unix, path = "list_unix.rs")]
mod list;

#[cfg(target_os = "android")]
mod android;

#[cfg_attr(windows, path = "watch_win.rs")]
#[cfg_attr(target_os = "linux", path = "watch_linux.rs")]
#[cfg_attr(
    all(unix, not(target_os = "linux"), not(target_os = "android")),
    path = "watch_route.rs"
)]
#[cfg_attr(target_os = "android", path = "watch_android.rs")]
mod watch;

pub mod async_adapter;

type IfIndex = u32;

pub use error::Error;

#[cfg(target_os = "android")]
pub use android::set_android_context;

/// An IP address paired with its prefix length (network mask).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
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

    /// Canonicalise the address list so that vector equality is equivalent to
    /// set equality.
    ///
    /// Addresses are sorted and duplicates are removed. This makes `Interface`
    /// (and `List`) equality insensitive to the order in which a platform
    /// returns addresses, so that a reordering of otherwise identical addresses
    /// does not produce a spurious "modified" update. It also collapses
    /// duplicate records into a single canonical entry.
    fn normalise(&mut self) {
        self.ips.sort();
        self.ips.dedup();
    }
}

/// Information delivered when a network interface snapshot changes.
///
/// This contains up-to-date information about all interfaces, plus a diff which
/// details which interfaces and IP addresses have changed since the previous update.
/// For an initial update, the diff treats every current interface as newly added.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Update {
    /// Whether this update represents the initial existing interface state.
    pub is_initial: bool,
    /// The complete current interface snapshot, keyed by interface index.
    pub interfaces: HashMap<IfIndex, Interface>,
    /// The changes from the preceding snapshot to `interfaces`.
    pub diff: UpdateDiff,
}

impl Update {
    /// Iterate over every address that appeared in this update.
    ///
    /// This includes addresses belonging to newly added interfaces as well as
    /// addresses added to interfaces that were present in both snapshots. Each
    /// item is `(ifindex, &IpRecord)`.
    pub fn addrs_added(&self) -> impl Iterator<Item = (IfIndex, &IpRecord)> + '_ {
        let from_added_interfaces = self
            .diff
            .added
            .iter()
            .flat_map(|(&idx, interface)| interface.ips.iter().map(move |addr| (idx, addr)));
        let from_modified_interfaces = self
            .diff
            .modified
            .iter()
            .flat_map(|(&idx, diff)| diff.addrs_added.iter().map(move |addr| (idx, addr)));

        from_added_interfaces.chain(from_modified_interfaces)
    }

    /// Iterate over every address that disappeared in this update.
    ///
    /// This includes addresses belonging to removed interfaces as well as
    /// addresses removed from interfaces that were present in both snapshots.
    /// Each item is `(ifindex, &IpRecord)`.
    pub fn addrs_removed(&self) -> impl Iterator<Item = (IfIndex, &IpRecord)> + '_ {
        let from_removed_interfaces = self
            .diff
            .removed
            .iter()
            .flat_map(|(&idx, interface)| interface.ips.iter().map(move |addr| (idx, addr)));
        let from_modified_interfaces = self
            .diff
            .modified
            .iter()
            .flat_map(|(&idx, diff)| diff.addrs_removed.iter().map(move |addr| (idx, addr)));

        from_removed_interfaces.chain(from_modified_interfaces)
    }
}

/// What changed between one `Update` and the next.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UpdateDiff {
    /// Interfaces that appeared, containing their new state.
    pub added: HashMap<IfIndex, Interface>,
    /// Interfaces that disappeared, containing their last known state.
    pub removed: HashMap<IfIndex, Interface>,
    /// Changes to interfaces that were present in both snapshots.
    pub modified: HashMap<IfIndex, InterfaceDiff>,
}

/// What changed within a single interface between updates, if it was present in both.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InterfaceDiff {
    /// Whether the interface name changed.
    pub name_changed: bool,
    /// Whether the hardware address changed.
    pub hw_addr_changed: bool,
    /// Addresses that appeared on this interface.
    pub addrs_added: Vec<IpRecord>,
    /// Addresses that disappeared from this interface.
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
        let added = self
            .0
            .iter()
            .filter(|(index, _)| !prev.0.contains_key(index))
            .map(|(&index, interface)| (index, interface.clone()))
            .collect();
        let removed = prev
            .0
            .iter()
            .filter(|(index, _)| !self.0.contains_key(index))
            .map(|(&index, interface)| (index, interface.clone()))
            .collect();
        let mut modified = HashMap::new();
        for (&index, interface) in &self.0 {
            let Some(prev_interface) = prev.0.get(&index) else {
                continue;
            };
            if prev_interface == interface {
                continue;
            }
            let (addrs_added, addrs_removed) = if prev_interface.ips == interface.ips {
                (Vec::new(), Vec::new())
            } else {
                let prev_addr_set: HashSet<&IpRecord> = prev_interface.ips.iter().collect();
                let curr_addr_set: HashSet<&IpRecord> = interface.ips.iter().collect();
                let addrs_added = curr_addr_set
                    .difference(&prev_addr_set)
                    .map(|addr| (*addr).clone())
                    .collect();
                let addrs_removed = prev_addr_set
                    .difference(&curr_addr_set)
                    .map(|addr| (*addr).clone())
                    .collect();
                (addrs_added, addrs_removed)
            };
            let name_changed = prev_interface.name != interface.name;
            let hw_addr_changed = prev_interface.hw_addr != interface.hw_addr;
            modified.insert(
                index,
                InterfaceDiff {
                    name_changed,
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
/// The callback is invoked once, synchronously before this function returns, with an initial
/// interface list and a diff as if there were originally no interfaces present. Subsequent
/// callbacks, including any that race with initialisation, are delivered on a platform-defined
/// thread that is not necessarily the thread that called this function, and may arrive before or
/// after this function returns. Do not rely on a particular delivery thread or on the timing of
/// later updates relative to this function returning.
///
/// If the initial callback panics, watcher construction unwinds and no watcher remains registered.
/// With the default unwinding panic strategy, a later callback panic is contained and permanently
/// disables that callback watcher without affecting other watchers. The panic hook still runs, and
/// the returned handle remains safe to drop. With `panic = "abort"`, any panic still aborts the
/// process.
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

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(last_octet: u8) -> IpRecord {
        IpRecord {
            ip: IpAddr::V4(Ipv4Addr::new(192, 0, 2, last_octet)),
            prefix_len: 24,
        }
    }

    fn interface(
        index: IfIndex,
        name: &str,
        hw_addr: &str,
        ips: impl IntoIterator<Item = IpRecord>,
    ) -> Interface {
        let mut interface = Interface {
            index,
            name: name.into(),
            hw_addr: hw_addr.into(),
            ips: ips.into_iter().collect(),
        };
        interface.normalise();
        interface
    }

    fn list(interfaces: impl IntoIterator<Item = Interface>) -> List {
        List(
            interfaces
                .into_iter()
                .map(|interface| (interface.index, interface))
                .collect(),
        )
    }

    fn owned_addresses<'a>(
        addresses: impl Iterator<Item = (IfIndex, &'a IpRecord)>,
    ) -> HashSet<(IfIndex, IpRecord)> {
        addresses
            .map(|(index, address)| (index, address.clone()))
            .collect()
    }

    #[test]
    fn initial_update_reports_complete_interfaces_and_addresses_as_added() {
        let first = interface(1, "first", "00:00:00:00:00:01", [ip(1), ip(2)]);
        let second = interface(2, "second", "00:00:00:00:00:02", []);
        let current = list([first.clone(), second.clone()]);

        let update = current.initial_update();

        assert!(update.is_initial);
        assert_eq!(update.interfaces, current.0);
        assert_eq!(
            update.diff.added,
            HashMap::from([(first.index, first), (second.index, second)])
        );
        assert!(update.diff.removed.is_empty());
        assert!(update.diff.modified.is_empty());
        assert_eq!(
            owned_addresses(update.addrs_added()),
            HashSet::from([(1, ip(1)), (1, ip(2))])
        );
        assert_eq!(update.addrs_removed().next(), None);
    }

    #[test]
    fn update_preserves_complete_added_and_removed_interfaces() {
        let removed = interface(1, "removed", "00:00:00:00:00:01", [ip(1), ip(2)]);
        let before = interface(2, "before", "00:00:00:00:00:02", [ip(10), ip(11)]);
        let after = interface(2, "after", "00:00:00:00:00:22", [ip(10), ip(12)]);
        let added = interface(3, "added", "00:00:00:00:00:03", [ip(20), ip(21)]);
        let unchanged = interface(4, "unchanged", "00:00:00:00:00:04", [ip(30)]);
        let previous = list([removed.clone(), before, unchanged.clone()]);
        let current = list([after.clone(), added.clone(), unchanged]);

        let update = current.update_from(&previous);

        assert!(!update.is_initial);
        assert_eq!(
            update.diff.added,
            HashMap::from([(added.index, added.clone())])
        );
        assert_eq!(
            update.diff.removed,
            HashMap::from([(removed.index, removed.clone())])
        );
        assert_eq!(
            update.diff.modified,
            HashMap::from([(
                after.index,
                InterfaceDiff {
                    name_changed: true,
                    hw_addr_changed: true,
                    addrs_added: vec![ip(12)],
                    addrs_removed: vec![ip(11)],
                }
            )])
        );
        assert_eq!(
            owned_addresses(update.addrs_added()),
            HashSet::from([(2, ip(12)), (3, ip(20)), (3, ip(21))])
        );
        assert_eq!(
            owned_addresses(update.addrs_removed()),
            HashSet::from([(1, ip(1)), (1, ip(2)), (2, ip(11))])
        );
    }

    #[test]
    fn metadata_only_change_does_not_report_address_changes() {
        let previous = list([interface(1, "before", "00:00:00:00:00:01", [ip(1)])]);
        let current = list([interface(1, "after", "00:00:00:00:00:01", [ip(1)])]);

        let update = current.update_from(&previous);

        assert_eq!(
            update.diff.modified,
            HashMap::from([(
                1,
                InterfaceDiff {
                    name_changed: true,
                    hw_addr_changed: false,
                    addrs_added: Vec::new(),
                    addrs_removed: Vec::new(),
                }
            )])
        );
        assert_eq!(update.addrs_added().next(), None);
        assert_eq!(update.addrs_removed().next(), None);
    }

    #[test]
    fn unchanged_update_has_an_empty_diff() {
        let current = list([interface(1, "unchanged", "00:00:00:00:00:01", [ip(1)])]);

        let update = current.update_from(&current);

        assert!(!update.is_initial);
        assert_eq!(update.interfaces, current.0);
        assert_eq!(update.diff, UpdateDiff::default());
        assert_eq!(update.addrs_added().next(), None);
        assert_eq!(update.addrs_removed().next(), None);
    }

    #[test]
    fn reordered_addresses_do_not_produce_an_update() {
        let previous = list([interface(1, "iface", "00:00:00:00:00:01", [ip(2), ip(1)])]);
        let current = list([interface(1, "iface", "00:00:00:00:00:01", [ip(1), ip(2)])]);

        let update = current.update_from(&previous);

        assert!(update.diff.added.is_empty());
        assert!(update.diff.removed.is_empty());
        assert!(update.diff.modified.is_empty());
        assert_eq!(update.addrs_added().next(), None);
        assert_eq!(update.addrs_removed().next(), None);
    }

    #[test]
    fn duplicate_addresses_are_normalised_and_produce_no_update() {
        let previous = list([interface(
            1,
            "iface",
            "00:00:00:00:00:01",
            [ip(1), ip(1), ip(2)],
        )]);
        let current = list([interface(1, "iface", "00:00:00:00:00:01", [ip(2), ip(1)])]);

        let update = current.update_from(&previous);

        assert!(update.diff.modified.is_empty());
        assert_eq!(update.interfaces[&1].ips, vec![ip(1), ip(2)]);
        assert_eq!(update.addrs_added().next(), None);
        assert_eq!(update.addrs_removed().next(), None);
    }

    #[test]
    fn reordering_alongside_real_changes_still_reports_them() {
        let previous = list([interface(
            1,
            "iface",
            "00:00:00:00:00:01",
            [ip(1), ip(2), ip(3)],
        )]);
        let current = list([interface(
            1,
            "iface",
            "00:00:00:00:00:01",
            [ip(3), ip(1), ip(4)],
        )]);

        let update = current.update_from(&previous);

        assert_eq!(
            update.diff.modified,
            HashMap::from([(
                1,
                InterfaceDiff {
                    name_changed: false,
                    hw_addr_changed: false,
                    addrs_added: vec![ip(4)],
                    addrs_removed: vec![ip(2)],
                }
            )])
        );
        assert_eq!(
            owned_addresses(update.addrs_added()),
            HashSet::from([(1, ip(4))])
        );
        assert_eq!(
            owned_addresses(update.addrs_removed()),
            HashSet::from([(1, ip(2))])
        );
    }

    #[test]
    fn normalise_sorts_and_dedups_ips() {
        let mut iface = Interface {
            index: 1,
            name: "iface".into(),
            hw_addr: "00:00:00:00:00:01".into(),
            ips: vec![ip(3), ip(1), ip(3), ip(2), ip(1)],
        };

        iface.normalise();

        assert_eq!(iface.ips, vec![ip(1), ip(2), ip(3)]);
    }
}
