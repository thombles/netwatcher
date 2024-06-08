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

#[cfg(unix)]
mod util;

type IfIndex = u32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Interface {
    pub index: u32,
    pub name: String,
    pub hw_addr: String,
    pub ips: Vec<IpAddr>,
}

impl Interface {
    pub fn ipv4_ips(&self) -> impl Iterator<Item = &Ipv4Addr> {
        self.ips.iter().filter_map(|ip| match ip {
            IpAddr::V4(v4) => Some(v4),
            IpAddr::V6(_) => None,
        })
    }

    pub fn ipv6_ips(&self) -> impl Iterator<Item = &Ipv6Addr> {
        self.ips.iter().filter_map(|ip| match ip {
            IpAddr::V4(_) => None,
            IpAddr::V6(v6) => Some(v6),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Update {
    pub interfaces: HashMap<IfIndex, Interface>,
    pub diff: UpdateDiff,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UpdateDiff {
    pub added: Vec<IfIndex>,
    pub removed: Vec<IfIndex>,
    pub modified: HashMap<IfIndex, InterfaceDiff>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InterfaceDiff {
    pub hw_addr_changed: bool,
    pub addrs_added: Vec<IpAddr>,
    pub addrs_removed: Vec<IpAddr>,
}

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

pub struct WatchHandle {
    _inner: watch::WatchHandle,
}

pub fn list_interfaces() -> Result<HashMap<IfIndex, Interface>, Error> {
    list::list_interfaces().map(|list| list.0)
}

pub fn watch_interfaces<F: FnMut(Update) + 'static>(callback: F) -> Result<WatchHandle, Error> {
    watch::watch_interfaces(callback).map(|handle| WatchHandle { _inner: handle })
}
