use std::os::fd::{AsRawFd, OwnedFd};
use std::sync::mpsc;

use nix::libc::{poll, pollfd, POLLIN};
use nix::sys::socket::{recv, socket, AddressFamily, MsgFlags, SockFlag, SockType};
use nix::unistd::pipe;

use crate::{Error, List, Update};

pub(crate) struct WatchHandle {
    pipefd: Option<OwnedFd>,
    complete: Option<mpsc::Receiver<()>>,
}

impl Drop for WatchHandle {
    fn drop(&mut self) {
        drop(self.pipefd.take());
        let _ = self.complete.take().unwrap().recv();
    }
}

pub(crate) fn watch_interfaces<F: FnMut(Update) + Send + 'static>(
    callback: F,
) -> Result<WatchHandle, Error> {
    let (pipefd, complete) = start_watcher_thread(callback)?;
    Ok(WatchHandle {
        pipefd: Some(pipefd),
        complete: Some(complete),
    })
}

fn start_watcher_thread<F: FnMut(Update) + Send + 'static>(
    mut callback: F,
) -> Result<(OwnedFd, mpsc::Receiver<()>), Error> {
    let sockfd = socket(AddressFamily::Route, SockType::Raw, SockFlag::empty(), None)
        .map_err(|e| Error::CreateSocket(e.to_string()))?;

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

    // Initial snapshot
    handle_update(crate::list::list_interfaces()?);

    let (complete_tx, complete_rx) = mpsc::channel();

    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
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
            unsafe { poll(fds.as_mut_ptr(), fds.len() as _, -1) };
            if fds[0].revents != 0 && recv(sockfd.as_raw_fd(), &mut buf, MsgFlags::empty()).is_ok()
            {
                let Ok(new_list) = crate::list::list_interfaces() else {
                    continue;
                };
                handle_update(new_list);
            }
            if fds[1].revents != 0 {
                break;
            }
        }
        drop(complete_tx);
    });

    Ok((pipe_wr, complete_rx))
}
