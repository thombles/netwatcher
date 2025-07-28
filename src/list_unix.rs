use std::fmt::Write;
use std::{collections::HashMap, net::IpAddr};

use nix::{ifaddrs::getifaddrs, net::if_::if_nametoindex};

use crate::{Error, Interface, List};

struct CandidateInterface {
    name: String,
    index: u32,
    hw_addr: Option<String>,
    ips: Vec<IpAddr>,
}

pub(crate) fn list_interfaces() -> Result<List, Error> {
    let addrs = getifaddrs().map_err(|e| Error::Getifaddrs(e.to_string()))?;
    let mut candidates = HashMap::new();

    for addr in addrs {
        let index = if_nametoindex(addr.interface_name.as_str())
            .map_err(|e| Error::GetInterfaceName(e.to_string()))?;
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
        .map(|(_, c)| {
            // suppressed on Android
            let hw_addr = c.hw_addr.unwrap_or_else(|| "00:00:00:00:00:00".to_string());
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
        .collect();
    Ok(List(ifs))
}

fn format_mac(bytes: &[u8]) -> Result<String, Error> {
    let mut mac = String::with_capacity(bytes.len() * 3);
    for (i, b) in bytes.iter().enumerate() {
        if i != 0 {
            write!(mac, ":").map_err(|_| Error::FormatMacAddress)?;
        }
        write!(mac, "{:02X}", b).map_err(|_| Error::FormatMacAddress)?;
    }
    Ok(mac)
}
