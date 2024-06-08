use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
};

#[cfg_attr(windows, path = "list_win.rs")]
#[cfg_attr(target_vendor = "apple", path = "list_mac.rs")]
mod list;

#[cfg_attr(windows, path = "watch_win.rs")]
#[cfg_attr(target_vendor = "apple", path = "watch_mac.rs")]
mod watch;

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

impl Update {
    pub fn diff_from_previous(_prev: &Update) -> UpdateDiff {
        // TODO: real calculation
        UpdateDiff::default()
    }
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

pub use list::list_interfaces;
pub use watch::{watch_interfaces, WatchHandle};

