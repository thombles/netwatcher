use std::{
    collections::{HashMap, HashSet},
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    ops::Sub,
};

#[cfg_attr(windows, path = "list_win.rs")]
#[cfg_attr(unix, path = "list_unix.rs")]
mod list;

#[cfg_attr(windows, path = "watch_win.rs")]
#[cfg_attr(target_vendor = "apple", path = "watch_mac.rs")]
#[cfg_attr(target_os = "linux", path = "watch_linux.rs")]
mod watch;

type IfIndex = u32;

/// Information about one network interface at a point in time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Interface {
    pub index: u32,
    pub name: String,
    pub hw_addr: String,
    pub ips: Vec<IpAddr>,
}

impl Interface {
    /// Helper to iterate over only the IPv4 addresses on this interface.
    pub fn ipv4_ips(&self) -> impl Iterator<Item = &Ipv4Addr> {
        self.ips.iter().filter_map(|ip| match ip {
            IpAddr::V4(v4) => Some(v4),
            IpAddr::V6(_) => None,
        })
    }

    /// Helper to iterate over only the IPv6 addresses on this interface.
    pub fn ipv6_ips(&self) -> impl Iterator<Item = &Ipv6Addr> {
        self.ips.iter().filter_map(|ip| match ip {
            IpAddr::V4(_) => None,
            IpAddr::V6(v6) => Some(v6),
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
    pub addrs_added: Vec<IpAddr>,
    pub addrs_removed: Vec<IpAddr>,
}

/// Errors in netwatcher or in one of the underlying platform integratinos.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    // TODO: handle all cases with proper sources
    Internal,
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
            let prev_addr_set: HashSet<&IpAddr> = prev.0[index].ips.iter().collect();
            let curr_addr_set: HashSet<&IpAddr> = self.0[index].ips.iter().collect();
            let addrs_added: Vec<IpAddr> = curr_addr_set
                .sub(&prev_addr_set)
                .iter()
                .cloned()
                .cloned()
                .collect();
            let addrs_removed: Vec<IpAddr> = prev_addr_set
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
pub fn watch_interfaces<F: FnMut(Update) + Send + 'static>(callback: F) -> Result<WatchHandle, Error> {
    watch::watch_interfaces(callback).map(|handle| WatchHandle { _inner: handle })
}
