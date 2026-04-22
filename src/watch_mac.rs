use std::os::fd::{AsRawFd, BorrowedFd, OwnedFd};

use nix::errno::Errno;
use nix::libc::{fcntl, F_GETFL, F_SETFL, O_NONBLOCK};
use nix::sys::socket::{recv, socket, AddressFamily, MsgFlags, SockFlag, SockType};

pub(crate) use crate::watch_fd::{AsyncWatch, BlockingWatch, WatchHandle};
use crate::{Error, Update};

const EVENT_SOCKET_OPS: crate::watch_fd::EventSocketOps = crate::watch_fd::EventSocketOps {
    open: open_event_socket,
    drain: drain_event_socket,
};

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
    let sockfd = socket(AddressFamily::Route, SockType::Raw, SockFlag::empty(), None)
        .map_err(|e| Error::CreateSocket(e.to_string()))?;

    let flags = unsafe { fcntl(sockfd.as_raw_fd(), F_GETFL) };
    if flags == -1 {
        return Err(Error::CreateSocket(
            std::io::Error::last_os_error().to_string(),
        ));
    }

    let set_status = unsafe { fcntl(sockfd.as_raw_fd(), F_SETFL, flags | O_NONBLOCK) };
    if set_status == -1 {
        return Err(Error::CreateSocket(
            std::io::Error::last_os_error().to_string(),
        ));
    }

    Ok(sockfd)
}

pub(crate) fn drain_event_socket(fd: BorrowedFd<'_>) {
    let mut buf = [0u8; 8192];
    loop {
        match recv(fd.as_raw_fd(), &mut buf, MsgFlags::empty()) {
            Ok(0) => break,
            Ok(_) => continue,
            Err(Errno::EAGAIN) => break,
            Err(_) => break,
        }
    }
}
