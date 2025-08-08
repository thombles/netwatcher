use std::fmt::Write;
use std::{collections::HashMap, net::IpAddr};

use nix::{ifaddrs::getifaddrs, net::if_::if_nametoindex};

use crate::{Error, Interface, IpRecord, List};

struct CandidateInterface {
    name: String,
    index: u32,
    hw_addr: Option<String>,
    ips: Vec<CandidateIpRecord>,
}

struct CandidateIpRecord {
    pub ip: IpAddr,
    pub prefix_len: Option<u8>,
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
            let (ip, prefix_len) = if let Some(a) = a.as_sockaddr_in() {
                (
                    IpAddr::V4(a.ip()),
                    addr.netmask.and_then(|netmask| {
                        netmask
                            .as_sockaddr_in()
                            .map(|netmask_in| netmask_in.ip().to_bits().leading_ones() as u8)
                    }),
                )
            } else if let Some(a) = a.as_sockaddr_in6() {
                (
                    IpAddr::V6(a.ip()),
                    addr.netmask.and_then(|netmask| {
                        netmask
                            .as_sockaddr_in6()
                            .map(|netmask_in6| netmask_in6.ip().to_bits().leading_ones() as u8)
                    }),
                )
            } else {
                continue;
            };
            candidate.ips.push(CandidateIpRecord { ip, prefix_len });
        }
    }

    let ifs = candidates
        .drain()
        .map(|(_, mut c)| {
            // alias IPs on Mac do not get their own prefix len
            if let Some(prefix_in_use) = c
                .ips
                .iter()
                .filter(|cip| cip.ip.is_ipv4())
                .flat_map(|cip| cip.prefix_len)
                .next()
            {
                for cip in &mut c.ips {
                    cip.prefix_len = Some(cip.prefix_len.unwrap_or(prefix_in_use));
                }
            }
            let ips = c
                .ips
                .iter()
                .flat_map(|cip| {
                    cip.prefix_len.map(|pl| IpRecord {
                        ip: cip.ip,
                        prefix_len: pl,
                    })
                })
                .collect();
            // MAC suppressed on Android
            let hw_addr = c.hw_addr.unwrap_or_else(|| "00:00:00:00:00:00".to_string());
            (
                c.index,
                Interface {
                    index: c.index,
                    hw_addr,
                    name: c.name,
                    ips,
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
        write!(mac, "{b:02X}").map_err(|_| Error::FormatMacAddress)?;
    }
    Ok(mac)
}
