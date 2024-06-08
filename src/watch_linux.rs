use std::os::fd::AsRawFd;
use std::os::fd::OwnedFd;

use nix::libc::nlmsghdr;
use nix::libc::RTMGRP_IPV4_IFADDR;
use nix::libc::RTMGRP_IPV6_IFADDR;
use nix::libc::RTMGRP_LINK;
use nix::sys::socket::bind;
use nix::sys::socket::recv;
use nix::sys::socket::socket;
use nix::sys::socket::AddressFamily;
use nix::sys::socket::MsgFlags;
use nix::sys::socket::NetlinkAddr;
use nix::sys::socket::SockFlag;
use nix::sys::socket::SockProtocol;
use nix::sys::socket::SockType;

use crate::Error;
use crate::Update;

pub(crate) struct WatchHandle {
    // PROBLEM: close() doesn't cancel recv() for a netlink socket
    sockfd: OwnedFd,
}

pub(crate) fn watch_interfaces<F: FnMut(Update) + 'static>(
    callback: F,
) -> Result<WatchHandle, Error> {
    let sockfd = start_watcher_thread(callback)?;
    Ok(WatchHandle { sockfd })
}

fn start_watcher_thread<F: FnMut(Update) + 'static>(callback: F) -> Result<OwnedFd, Error> {
    let sockfd = socket(AddressFamily::Netlink, SockType::Raw, SockFlag::empty(), Some(SockProtocol::NetlinkRoute))
        .map_err(|_| Error::Internal)?; // TODO: proper errors
    let sa_nl = NetlinkAddr::new(0, (RTMGRP_LINK | RTMGRP_IPV4_IFADDR | RTMGRP_IPV6_IFADDR) as u32);
    bind(sockfd.as_raw_fd(), &sa_nl).map_err(|_| Error::Internal)?; // TODO: proper errors
    let fd = sockfd.as_raw_fd();
    println!("netlink socket on fd {}", fd);

    std::thread::spawn(move || {
        println!("watch thread running");
        let mut buf = [0u8; 4096];
        // recvmsg?
        while let Ok(n) = recv(fd, &mut buf, MsgFlags::empty()) {
            println!("something on the netlink socket: {} bytes", n);
            let nlmsg_ptr = &buf as *const _ as *const nlmsghdr;
            let nlmsg = unsafe { &*nlmsg_ptr };
            // Right conventionally there's some trick here involving macros NLMSG_OK
            // I can presumably do this using NetlinkGeneric too
            // It's unclear whether this is worse or not - need to know what those macros do
        }
        println!("netlink recv thread terminating");
    });
    
    Ok(sockfd)
}
