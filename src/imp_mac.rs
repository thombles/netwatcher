use std::{collections::HashMap, net::IpAddr};

use block2::Block;
use nix::libc::c_long;
use nix::{ifaddrs::getifaddrs, net::if_::if_nametoindex};

use crate::util::format_mac;
use crate::{Error, IfIndex, Interface};

struct CandidateInterface {
    name: String,
    index: u32,
    hw_addr: Option<String>,
    ips: Vec<IpAddr>,
}

pub(crate) fn list_interfaces() -> Result<HashMap<IfIndex, Interface>, Error> {
    let addrs = getifaddrs().map_err(|_| Error::Internal)?;
    let mut candidates = HashMap::new();

    for addr in addrs {
        let index = if_nametoindex(addr.interface_name.as_str()).map_err(|_| Error::Internal)?;
        let candidate = candidates
            .entry(addr.interface_name.clone())
            .or_insert_with(|| CandidateInterface {
                name: addr.interface_name.clone(),
                index,
                hw_addr: None,
                ips: vec![],
            });
        if let Some(a) = addr.address {
            if let Some(a) = a.as_link_addr() {
                if let Some(raw_addr) = a.addr() {
                    candidate.hw_addr = Some(format_mac(&raw_addr)?);
                }
            }
            if let Some(a) = a.as_sockaddr_in() {
                candidate.ips.push(IpAddr::V4(a.ip()));
            }
            if let Some(a) = a.as_sockaddr_in6() {
                candidate.ips.push(IpAddr::V6(a.ip()));
            }
        }
    }

    let ifs = candidates
        .drain()
        .flat_map(|(_, c)| {
            c.hw_addr.map(|hw_addr| {
                (
                    c.index,
                    Interface {
                        index: c.index,
                        hw_addr,
                        name: c.name,
                        ips: c.ips,
                    },
                )
            })
        })
        .collect();
    Ok(ifs)
}

// The "objc2" project aims to provide bindings for all frameworks but Network.framework
// isn't ready yet so let's kick it old-school

struct nw_path_monitor;
type nw_path_monitor_t = *mut nw_path_monitor;
struct nw_path;
type nw_path_t = *mut nw_path;
struct dispatch_queue;
type dispatch_queue_t = *mut dispatch_queue;
const QOS_CLASS_BACKGROUND: usize = 0x09;

#[link(name = "Network", kind = "framework")]
extern "C" {
    fn nw_path_monitor_create() -> nw_path_monitor_t;
    fn nw_path_monitor_set_update_handler(
        monitor: nw_path_monitor_t,
        update_handler: &Block<dyn Fn(nw_path_t)>,
    );
    fn nw_path_monitor_set_queue(monitor: nw_path_monitor_t, queue: dispatch_queue_t);
    fn nw_path_monitor_start(monitor: nw_path_monitor_t);
    fn nw_path_monitor_cancel(monitor: nw_path_monitor_t);

    fn dispatch_get_global_queue(identifier: usize, flag: usize) -> dispatch_queue_t;
}

#[cfg(test)]
mod test {
    use super::list_interfaces;

    #[test]
    fn list() {
        let ifaces = list_interfaces().unwrap();
        println!("{:?}", ifaces);
    }
}
