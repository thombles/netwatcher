use std::fmt::Write;
use std::{collections::HashMap, net::IpAddr};

use nix::{
    ifaddrs::getifaddrs,
    net::if_::{if_nametoindex, InterfaceFlags},
};

use crate::{Error, Interface, IpRecord, List};

struct CandidateInterface {
    name: String,
    index: u32,
    flags: InterfaceFlags,
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
                flags: addr.flags,
                hw_addr: None,
                ips: vec![],
            });
        candidate.flags |= addr.flags;
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
        .filter(|(_, c)| c.flags.contains(InterfaceFlags::IFF_UP))
        .map(|(_, mut c)| {
            // alias IPs on Mac do not get their own prefix len
            apply_alias_prefix_fallback(&mut c.ips);
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
            let mut interface = Interface {
                index: c.index,
                hw_addr,
                name: c.name,
                ips,
            };
            interface.normalise();
            (c.index, interface)
        })
        .collect();
    Ok(List(ifs))
}

// On macOS, alias addresses are not reported with their own netmask. Borrow a
// prefix length from another address of the same family on the interface so
// that aliases are not dropped. The fallback is applied per address family so
// that an IPv6 address never inherits an IPv4 prefix.
fn apply_alias_prefix_fallback(ips: &mut [CandidateIpRecord]) {
    apply_fallback_for_family(ips, IpAddr::is_ipv4);
    apply_fallback_for_family(ips, IpAddr::is_ipv6);
}

fn apply_fallback_for_family(
    ips: &mut [CandidateIpRecord],
    matches_family: impl Fn(&IpAddr) -> bool,
) {
    let prefix_in_use = ips
        .iter()
        .filter(|cip| matches_family(&cip.ip))
        .find_map(|cip| cip.prefix_len);
    let Some(prefix_in_use) = prefix_in_use else {
        return;
    };
    for cip in ips.iter_mut().filter(|cip| matches_family(&cip.ip)) {
        cip.prefix_len = Some(cip.prefix_len.unwrap_or(prefix_in_use));
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    fn record(ip: IpAddr, prefix_len: Option<u8>) -> CandidateIpRecord {
        CandidateIpRecord { ip, prefix_len }
    }

    #[test]
    fn ipv4_prefix_is_applied_to_ipv4_without_prefix() {
        let mut ips = vec![
            record(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)), Some(24)),
            record(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 2)), None),
        ];
        apply_alias_prefix_fallback(&mut ips);
        assert_eq!(ips[0].prefix_len, Some(24));
        assert_eq!(ips[1].prefix_len, Some(24));
    }

    #[test]
    fn ipv6_prefix_is_applied_to_ipv6_without_prefix() {
        let mut ips = vec![
            record(IpAddr::V6(Ipv6Addr::LOCALHOST), Some(128)),
            record(IpAddr::V6("2001:db8::2".parse().unwrap()), None),
        ];
        apply_alias_prefix_fallback(&mut ips);
        assert_eq!(ips[0].prefix_len, Some(128));
        assert_eq!(ips[1].prefix_len, Some(128));
    }

    #[test]
    fn ipv4_prefix_is_not_applied_to_ipv6() {
        let mut ips = vec![
            record(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)), Some(24)),
            record(IpAddr::V6(Ipv6Addr::LOCALHOST), None),
        ];
        apply_alias_prefix_fallback(&mut ips);
        assert_eq!(ips[0].prefix_len, Some(24));
        assert_eq!(ips[1].prefix_len, None);
    }

    #[test]
    fn existing_prefixes_are_preserved() {
        let mut ips = vec![
            record(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)), Some(24)),
            record(IpAddr::V4(Ipv4Addr::new(198, 51, 100, 1)), Some(16)),
            record(IpAddr::V6(Ipv6Addr::LOCALHOST), Some(128)),
            record(
                IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1)),
                Some(64),
            ),
        ];
        apply_alias_prefix_fallback(&mut ips);
        assert_eq!(ips[0].prefix_len, Some(24));
        assert_eq!(ips[1].prefix_len, Some(16));
        assert_eq!(ips[2].prefix_len, Some(128));
        assert_eq!(ips[3].prefix_len, Some(64));
    }
}
