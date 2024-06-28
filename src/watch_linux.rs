use std::os::fd::AsRawFd;
use std::os::fd::OwnedFd;

use nix::libc::poll;
use nix::libc::pollfd;
use nix::libc::POLLIN;
use nix::sys::socket::bind;
use nix::sys::socket::recv;
use nix::sys::socket::socket;
use nix::sys::socket::AddressFamily;
use nix::sys::socket::MsgFlags;
use nix::sys::socket::NetlinkAddr;
use nix::sys::socket::SockFlag;
use nix::sys::socket::SockProtocol;
use nix::sys::socket::SockType;
use nix::unistd::pipe;

use crate::Error;
use crate::List;
use crate::Update;

const RTMGRP_IPV4_IFADDR: u32 = 0x10;
const RTMGRP_IPV6_IFADDR: u32 = 0x20;
const RTMGRP_LINK: u32 = 0x01;

pub(crate) struct WatchHandle {
    // Dropping will close the fd which will be detected by poll
    _pipefd: OwnedFd,
}

pub(crate) fn watch_interfaces<F: FnMut(Update) + Send + 'static>(
    callback: F,
) -> Result<WatchHandle, Error> {
    let pipefd = start_watcher_thread(callback)?;
    Ok(WatchHandle { _pipefd: pipefd })
}

fn start_watcher_thread<F: FnMut(Update) + Send + 'static>(
    mut callback: F,
) -> Result<OwnedFd, Error> {
    let sockfd = socket(
        AddressFamily::Netlink,
        SockType::Raw,
        SockFlag::empty(),
        Some(SockProtocol::NetlinkRoute),
    )
    .map_err(|e| Error::CreateSocket(e.to_string()))?;
    sockfd.set_nonblocking(true);
    let sa_nl = NetlinkAddr::new(
        0,
        (RTMGRP_LINK | RTMGRP_IPV4_IFADDR | RTMGRP_IPV6_IFADDR) as u32,
    );
    bind(sockfd.as_raw_fd(), &sa_nl).map_err(|e| Error::Bind(e.to_string()))?;
    let (pipe_rd, pipe_wr) = pipe().map_err(|e| Error::CreatePipe(e.to_string()))?;

    let mut prev_list = List::default();
    let mut handle_update = move |new_list: List| {
        if new_list == prev_list {
            return;
        }
        let update = Update {
            interfaces: new_list.0.clone(),
            diff: new_list.diff_from(&prev_list),
        };
        (callback)(update);
        prev_list = new_list;
    };

    // Now that netlink socket is open, provide an initial update.
    // By having this outside the thread we can return an error synchronously if it
    // looks like we're going to have trouble listing interfaces.
    handle_update(crate::list::list_interfaces()?);

    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];

        loop {
            let mut fds = [
                pollfd {
                    fd: sockfd.as_raw_fd(),
                    events: POLLIN,
                    revents: 0,
                },
                pollfd {
                    fd: pipe_rd.as_raw_fd(),
                    events: POLLIN,
                    revents: 0,
                },
            ];
            unsafe {
                poll(&mut fds as *mut _, 2, -1);
            }
            if fds[0].revents != 0 {
                // netlink socket had something happen
                if recv(sockfd.as_raw_fd(), &mut buf, MsgFlags::empty()).is_ok() {
                    let Ok(new_list) = crate::list::list_interfaces() else {
                        continue;
                    };
                    handle_update(new_list);
                }
            }
            if fds[1].revents != 0 {
                // pipe had something happen
                break;
            }
        }
    });

    Ok(pipe_wr)
}
