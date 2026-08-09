use std::collections::HashMap;
use std::fmt::Write;
use std::net::IpAddr;
use windows::Win32::Foundation::{
    ERROR_ADDRESS_NOT_ASSOCIATED, ERROR_BUFFER_OVERFLOW, ERROR_INVALID_PARAMETER,
    ERROR_NOT_ENOUGH_MEMORY, ERROR_NO_DATA, ERROR_SUCCESS, WIN32_ERROR,
};
use windows::Win32::NetworkManagement::IpHelper::{
    GetAdaptersAddresses, IP_ADAPTER_UNICAST_ADDRESS_LH,
};
use windows::Win32::NetworkManagement::IpHelper::{
    GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_MULTICAST, IP_ADAPTER_ADDRESSES_LH,
};
use windows::Win32::NetworkManagement::Ndis::IfOperStatusDown;
use windows::Win32::Networking::WinSock::{
    AF_INET, AF_INET6, AF_UNSPEC, SOCKADDR, SOCKADDR_IN, SOCKADDR_IN6,
};

use crate::{Error, Interface, IpRecord, List};
use aligned_vec::{AVec, ConstAlign};

pub(crate) fn list_interfaces() -> Result<List, Error> {
    let mut ifs = HashMap::new();
    // Microsoft recommends a 15 KB initial buffer
    let start_size = 15 * 1024;
    // GetAdaptersAddresses fills this buffer with IP_ADAPTER_ADDRESSES_LH structs, so it must
    // meet that type's alignment. Unlike Vec<u8>, AVec guarantees the requested alignment
    // regardless of the global allocator.
    let mut buf: AVec<u8, ConstAlign<8>> = AVec::new(8);
    buf.resize(start_size, 0);
    let mut sizepointer: u32 = start_size as u32;

    unsafe {
        loop {
            debug_assert!(
                (buf.as_ptr() as usize).is_multiple_of(align_of::<IP_ADAPTER_ADDRESSES_LH>())
            );
            let bufptr = buf.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH;
            let res = GetAdaptersAddresses(
                AF_UNSPEC.0.into(),
                GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST,
                None,
                Some(bufptr),
                &mut sizepointer,
            );
            match WIN32_ERROR(res) {
                ERROR_SUCCESS => break,
                ERROR_ADDRESS_NOT_ASSOCIATED => return Err(Error::AddressNotAssociated),
                ERROR_BUFFER_OVERFLOW => {
                    buf.resize(sizepointer as usize, 0);
                    continue;
                }
                ERROR_INVALID_PARAMETER => return Err(Error::InvalidParameter),
                ERROR_NOT_ENOUGH_MEMORY => return Err(Error::NotEnoughMemory),
                ERROR_NO_DATA => return Ok(List(HashMap::new())), // there aren't any
                _ => return Err(Error::UnexpectedWindowsResult(res)),
            }
        }

        // We have at least one
        let mut adapter_ptr = buf.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH;
        while !adapter_ptr.is_null() {
            let adapter = &*adapter_ptr as &IP_ADAPTER_ADDRESSES_LH;
            if adapter.OperStatus == IfOperStatusDown {
                adapter_ptr = adapter.Next;
                continue;
            }
            let mut hw_addr = String::with_capacity(adapter.PhysicalAddressLength as usize * 3);
            for i in 0..adapter.PhysicalAddressLength as usize {
                if i != 0 {
                    write!(hw_addr, ":").map_err(|_| Error::FormatMacAddress)?;
                }
                write!(hw_addr, "{:02X}", adapter.PhysicalAddress[i])
                    .map_err(|_| Error::FormatMacAddress)?;
            }
            let mut ips = vec![];
            let mut unicast_ptr = adapter.FirstUnicastAddress;
            while !unicast_ptr.is_null() {
                let unicast = &*unicast_ptr as &IP_ADAPTER_UNICAST_ADDRESS_LH;
                let sockaddr = &*unicast.Address.lpSockaddr as &SOCKADDR;
                let ip = match sockaddr.sa_family {
                    AF_INET => {
                        let sockaddr_in =
                            &*(unicast.Address.lpSockaddr as *const SOCKADDR_IN) as &SOCKADDR_IN;
                        IpAddr::V4(sockaddr_in.sin_addr.into())
                    }
                    AF_INET6 => {
                        let sockaddr_in6 =
                            &*(unicast.Address.lpSockaddr as *const SOCKADDR_IN6) as &SOCKADDR_IN6;
                        IpAddr::V6(sockaddr_in6.sin6_addr.into())
                    }
                    _ => continue,
                };
                let prefix_len = unicast.OnLinkPrefixLength;
                ips.push(IpRecord { ip, prefix_len });
                unicast_ptr = unicast.Next;
            }

            let ipv4_if_index = adapter.Anonymous1.Anonymous.IfIndex;
            let ipv6_if_index = adapter.Ipv6IfIndex;
            let ifindex = if ipv4_if_index != 0 {
                ipv4_if_index
            } else if ipv6_if_index != 0 {
                ipv6_if_index
            } else {
                adapter_ptr = adapter.Next;
                continue;
            };

            let name = adapter
                .FriendlyName
                .to_string()
                .unwrap_or_else(|_| "".to_owned());
            let mut iface = Interface {
                index: ifindex,
                name,
                hw_addr,
                ips,
            };
            iface.normalise();
            ifs.insert(ifindex, iface);
            adapter_ptr = adapter.Next;
        }
    }

    Ok(List(ifs))
}

#[cfg(test)]
mod tests {
    use super::*;

    // The buffer handed to GetAdaptersAddresses is reinterpreted as IP_ADAPTER_ADDRESSES_LH,
    // so its alignment must survive both the initial allocation and the growth that follows
    // ERROR_BUFFER_OVERFLOW. An odd size models the byte counts the API actually reports.
    #[test]
    fn adapter_buffer_stays_aligned_across_growth() {
        let mut buf: AVec<u8, ConstAlign<8>> = AVec::new(8);
        buf.resize(15 * 1024, 0);
        assert!((buf.as_ptr() as usize).is_multiple_of(align_of::<IP_ADAPTER_ADDRESSES_LH>()));

        buf.resize(16 * 1024 + 383, 0);
        assert!((buf.as_ptr() as usize).is_multiple_of(align_of::<IP_ADAPTER_ADDRESSES_LH>()));
    }
}
