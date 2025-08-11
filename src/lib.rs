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
//! /// Returns a HashMap from ifindex (a `u32`) to an `Interface` struct
//! let interfaces = netwatcher::list_interfaces().unwrap();
//! for i in interfaces.values() {
//!     println!("interface {}", i.name);
//!     for ip_record in &i.ips {
//!         println!("IP: {}/{}", ip_record.ip, ip_record.prefix_len);
//!     }
//! }
//! ```
//!
//! ## Watch example
//!
//! ```
//! let handle = netwatcher::watch_interfaces(|update| {
//!     // This callback will fire once immediately with the existing state
//!
//!     // Update includes the latest snapshot of all interfaces
//!     println!("Current interface map: {:#?}", update.interfaces);
//!
//!     // The `UpdateDiff` describes changes since previous callback
//!     // You can choose whether to use the snapshot, diff, or both
//!     println!("ifindexes added: {:?}", update.diff.added);
//!     println!("ifindexes removed: {:?}", update.diff.removed);
//!     for (ifindex, if_diff) in update.diff.modified {
//!         println!("Interface index {} has changed", ifindex);
//!         println!("Added IPs: {:?}", if_diff.addrs_added);
//!         println!("Removed IPs: {:?}", if_diff.addrs_removed);
//!     }
//! }).unwrap();
//! // keep `handle` alive as long as you want callbacks
//! // ...
//! drop(handle);
//! ```

use std::{
    collections::{HashMap, HashSet},
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    ops::Sub,
};

mod error;

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

/// Information delivered via callback when a network interface change is detected.
///
/// This contains up-to-date information about all interfaces, plus a diff which
/// details which interfaces and IP addresses have changed since the last callback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Update {
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

#[derive(Default, PartialEq, Eq)]
struct List(HashMap<IfIndex, Interface>);

impl List {
    fn diff_from(&self, prev: &List) -> UpdateDiff {
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
        UpdateDiff {
            added,
            removed,
            modified,
        }
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

/// Retrieve information about all enabled network interfaces and their IP addresses.
///
/// This is a once-off operation. If you want to detect changes over time, see `watch_interfaces`.
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
pub fn watch_interfaces<F: FnMut(Update) + Send + 'static>(
    callback: F,
) -> Result<WatchHandle, Error> {
    watch::watch_interfaces(callback).map(|handle| WatchHandle { _inner: handle })
}
