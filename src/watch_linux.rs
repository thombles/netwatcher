use std::os::fd::{AsRawFd, BorrowedFd, OwnedFd};

use nix::errno::Errno;
use nix::sys::socket::bind;
use nix::sys::socket::recv;
use nix::sys::socket::socket;
use nix::sys::socket::AddressFamily;
use nix::sys::socket::MsgFlags;
use nix::sys::socket::NetlinkAddr;
use nix::sys::socket::SockFlag;
use nix::sys::socket::SockProtocol;
use nix::sys::socket::SockType;

pub(crate) use crate::watch_fd::{AsyncWatch, BlockingWatch, WatchHandle};
use crate::Error;
use crate::Update;

const EVENT_SOCKET_OPS: crate::watch_fd::EventSocketOps = crate::watch_fd::EventSocketOps {
    open: open_event_socket,
    drain: drain_event_socket,
};

const RTMGRP_IPV4_IFADDR: u32 = 0x10;
const RTMGRP_IPV6_IFADDR: u32 = 0x20;
const RTMGRP_LINK: u32 = 0x01;

pub(crate) fn watch_interfaces_with_callback<F: FnMut(Update) + Send + 'static>(
    callback: F,
) -> Result<WatchHandle, Error> {
    crate::watch_fd::watch_interfaces_with_callback(callback, EVENT_SOCKET_OPS)
}

pub(crate) fn watch_interfaces_async<A: crate::AsyncFdAdapter>() -> Result<AsyncWatch, Error> {
    crate::watch_fd::watch_interfaces_async::<A>(EVENT_SOCKET_OPS)
}

pub(crate) fn watch_interfaces_blocking() -> Result<BlockingWatch, Error> {
    crate::watch_fd::watch_interfaces_blocking(EVENT_SOCKET_OPS)
}

pub(crate) fn open_event_socket() -> Result<OwnedFd, Error> {
    let sockfd = socket(
        AddressFamily::Netlink,
        SockType::Raw,
        SockFlag::SOCK_NONBLOCK,
        Some(SockProtocol::NetlinkRoute),
    )
    .map_err(|e| Error::CreateSocket(e.to_string()))?;
    let sa_nl = NetlinkAddr::new(0, RTMGRP_LINK | RTMGRP_IPV4_IFADDR | RTMGRP_IPV6_IFADDR);
    bind(sockfd.as_raw_fd(), &sa_nl).map_err(|e| Error::Bind(e.to_string()))?;
    Ok(sockfd)
}

pub(crate) fn drain_event_socket(fd: BorrowedFd<'_>) {
    let mut buf = [0u8; 4096];
    loop {
        match recv(fd.as_raw_fd(), &mut buf, MsgFlags::empty()) {
            Ok(0) => break,
            Ok(_) => continue,
            Err(Errno::EAGAIN) => break,
            Err(_) => break,
        }
    }
}
