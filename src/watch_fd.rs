use std::os::fd::{AsFd, AsRawFd, BorrowedFd, OwnedFd};
use std::sync::mpsc;

use nix::libc::{poll, pollfd, POLLIN};
use nix::unistd::pipe;

use crate::{Error, Update};

pub(crate) type DrainEventSocket = for<'fd> fn(BorrowedFd<'fd>);
pub(crate) type OpenEventSocket = fn() -> Result<OwnedFd, Error>;

#[derive(Clone, Copy)]
pub(crate) struct EventSocketOps {
    pub(crate) open: OpenEventSocket,
    pub(crate) drain: DrainEventSocket,
}

pub(crate) struct WatchHandle {
    pipefd: Option<OwnedFd>,
    complete: Option<mpsc::Receiver<()>>,
}

pub(crate) struct AsyncWatch {
    registration: Box<dyn crate::async_adapter::AsyncFdRegistration>,
    cursor: crate::UpdateCursor,
    initial_update: Option<Update>,
    drain_event_socket: DrainEventSocket,
}

pub(crate) struct BlockingWatch {
    socket: OwnedFd,
    cursor: crate::UpdateCursor,
    initial_update: Option<Update>,
    drain_event_socket: DrainEventSocket,
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

            (self.drain_event_socket)(ready.fd().as_fd());
            ready.clear_ready();

            let Ok(new_list) = crate::list::list_interfaces() else {
                continue;
            };
            if let Some(update) = self.cursor.advance(new_list) {
                return update;
            }
        }
    }
}

impl BlockingWatch {
    pub(crate) fn changed(&mut self) -> Update {
        if let Some(initial_update) = self.initial_update.take() {
            return initial_update;
        }

        loop {
            wait_for_readable(self.socket.as_fd());
            if let Some(update) = next_update(
                &mut self.cursor,
                self.socket.as_fd(),
                self.drain_event_socket,
            ) {
                return update;
            }
        }
    }
}

impl Drop for WatchHandle {
    fn drop(&mut self) {
        drop(self.pipefd.take());
        let _ = self.complete.take().unwrap().recv();
    }
}

pub(crate) fn watch_interfaces_with_callback<F: FnMut(Update) + Send + 'static>(
    callback: F,
    ops: EventSocketOps,
) -> Result<WatchHandle, Error> {
    let (pipefd, complete) = start_watcher_thread(callback, ops)?;
    Ok(WatchHandle {
        pipefd: Some(pipefd),
        complete: Some(complete),
    })
}

pub(crate) fn watch_interfaces_async<A: crate::async_adapter::AsyncFdAdapter>(
    ops: EventSocketOps,
) -> Result<AsyncWatch, Error> {
    let socket = (ops.open)()?;
    let registration = A::register(crate::async_adapter::AsyncFd::from_owned_fd(socket))
        .map_err(crate::Error::Io)?;
    let current_list = crate::list::list_interfaces()?;
    let mut cursor = crate::UpdateCursor::default();
    let initial_update = cursor.advance(current_list);
    Ok(AsyncWatch {
        registration,
        cursor,
        initial_update,
        drain_event_socket: ops.drain,
    })
}

pub(crate) fn watch_interfaces_blocking(ops: EventSocketOps) -> Result<BlockingWatch, Error> {
    let socket = (ops.open)()?;
    let current_list = crate::list::list_interfaces()?;
    let mut cursor = crate::UpdateCursor::default();
    let initial_update = cursor.advance(current_list);
    Ok(BlockingWatch {
        socket,
        cursor,
        initial_update,
        drain_event_socket: ops.drain,
    })
}

fn start_watcher_thread<F: FnMut(Update) + Send + 'static>(
    mut callback: F,
    ops: EventSocketOps,
) -> Result<(OwnedFd, mpsc::Receiver<()>), Error> {
    let sockfd = (ops.open)()?;
    let (pipe_rd, pipe_wr) = pipe().map_err(|e| Error::CreatePipe(e.to_string()))?;
    let mut cursor = crate::UpdateCursor::default();

    (callback)(cursor.advance(crate::list::list_interfaces()?).unwrap());

    let (complete_tx, complete_rx) = mpsc::channel();

    std::thread::spawn(move || {
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
                if let Some(update) = next_update(&mut cursor, sockfd.as_fd(), ops.drain) {
                    (callback)(update);
                }
            }
            if fds[1].revents != 0 {
                break;
            }
        }

        drop(complete_tx);
    });

    Ok((pipe_wr, complete_rx))
}

fn next_update(
    cursor: &mut crate::UpdateCursor,
    fd: BorrowedFd<'_>,
    drain_event_socket: DrainEventSocket,
) -> Option<Update> {
    drain_event_socket(fd);
    let Ok(new_list) = crate::list::list_interfaces() else {
        return None;
    };
    cursor.advance(new_list)
}

fn wait_for_readable(fd: BorrowedFd<'_>) {
    loop {
        let mut fds = [pollfd {
            fd: fd.as_raw_fd(),
            events: POLLIN,
            revents: 0,
        }];
        unsafe {
            poll(&mut fds as *mut _, 1, -1);
        }
        if fds[0].revents != 0 {
            return;
        }
    }
}
