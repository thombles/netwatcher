use std::os::fd::{AsFd, AsRawFd, BorrowedFd, OwnedFd};
use std::sync::mpsc;

use nix::errno::Errno;
use nix::libc::{fcntl, poll, pollfd, F_GETFL, F_SETFL, O_NONBLOCK, POLLIN};
use nix::sys::socket::{recv, socket, AddressFamily, MsgFlags, SockFlag, SockType};
use nix::unistd::pipe;

use crate::{Error, List, Update};

pub(crate) struct WatchHandle {
    pipefd: Option<OwnedFd>,
    complete: Option<mpsc::Receiver<()>>,
}

pub(crate) struct AsyncWatch {
    registration: Box<dyn crate::AsyncFdRegistration>,
    prev_list: List,
    initial_update: Option<Update>,
}

impl AsyncWatch {
    pub(crate) async fn changed(&mut self) -> Update {
        if let Some(initial_update) = self.initial_update.take() {
            return initial_update;
        }

        loop {
            let mut ready = match self.registration.readable().await {
                Ok(ready) => ready,
                Err(_) => continue,
            };

            drain_event_socket(ready.fd());
            ready.clear_ready();

            let Ok(new_list) = crate::list::list_interfaces() else {
                continue;
            };
            if new_list == self.prev_list {
                continue;
            }

            let update = new_list.update_from(&self.prev_list);
            self.prev_list = new_list;
            return update;
        }
    }
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

pub(crate) fn watch_interfaces_async<A: crate::AsyncFdAdapter>() -> Result<AsyncWatch, Error> {
    let socket = open_event_socket()?;
    let registration = A::register(socket).map_err(crate::Error::Io)?;
    let current_list = crate::list::list_interfaces()?;
    let initial_update = current_list.update_from(&List::default());
    Ok(AsyncWatch {
        registration,
        prev_list: current_list,
        initial_update: Some(initial_update),
    })
}

fn start_watcher_thread<F: FnMut(Update) + Send + 'static>(
    mut callback: F,
) -> Result<(OwnedFd, mpsc::Receiver<()>), Error> {
    let sockfd = open_event_socket()?;

    let (pipe_rd, pipe_wr) = pipe().map_err(|e| Error::CreatePipe(e.to_string()))?;

    let mut prev_list = List::default();
    let mut handle_update = move |new_list: List| {
        if new_list == prev_list {
            return;
        }
        let update = new_list.update_from(&prev_list);
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
                drain_event_socket(sockfd.as_fd());
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
