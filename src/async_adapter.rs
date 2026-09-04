use std::{future::Future, pin::Pin};

#[cfg(any(windows, target_os = "android"))]
use std::marker::PhantomData;

#[cfg(all(
    any(feature = "async-io", feature = "tokio"),
    unix,
    not(target_os = "android")
))]
use std::io;
#[cfg(all(
    any(feature = "async-io", feature = "tokio"),
    unix,
    not(target_os = "android")
))]
use std::os::fd::AsFd;
#[cfg(all(unix, not(target_os = "android")))]
use std::os::fd::{BorrowedFd, OwnedFd};

/// Runtime-owned readiness source passed to an [`AsyncFdAdapter`].
///
/// On non-Android Unix platforms this wraps the nonblocking watch file descriptor.
/// On other platforms the value is never used.
pub struct AsyncFd {
    #[cfg(all(unix, not(target_os = "android")))]
    inner: OwnedFd,
    #[cfg(any(windows, target_os = "android"))]
    _private: (),
}

impl AsyncFd {
    #[cfg(all(unix, not(target_os = "android")))]
    pub(crate) fn from_owned_fd(inner: OwnedFd) -> Self {
        Self { inner }
    }

    #[cfg(all(unix, not(target_os = "android")))]
    pub fn into_owned_fd(self) -> OwnedFd {
        self.inner
    }
}

/// Borrowed readiness source passed to an [`AsyncFdRegistration`] drain callback.
///
/// On non-Android Unix platforms this wraps the watch file descriptor.
/// On other platforms the value is never used.
pub struct AsyncFdRef<'a> {
    #[cfg(all(unix, not(target_os = "android")))]
    inner: BorrowedFd<'a>,
    #[cfg(any(windows, target_os = "android"))]
    _marker: PhantomData<&'a ()>,
}

impl<'a> AsyncFdRef<'a> {
    #[cfg(all(unix, not(target_os = "android")))]
    pub fn from_borrowed_fd(inner: BorrowedFd<'a>) -> Self {
        Self { inner }
    }

    #[cfg(all(unix, not(target_os = "android")))]
    pub fn as_fd(&self) -> BorrowedFd<'_> {
        self.inner
    }
}

/// A runtime adapter that can register an existing nonblocking file descriptor for async waiting.
///
/// On Windows and Android, the adapter type is still required for API consistency, but the
/// platform watcher uses callback-driven notifications and does not invoke the adapter.
pub trait AsyncFdAdapter {
    fn register(fd: AsyncFd) -> std::io::Result<Box<dyn AsyncFdRegistration>>;
}

/// Boxed future returned by [`AsyncFdRegistration::readable_and_drain`].
pub type AsyncFdReadableFuture<'a> = Pin<Box<dyn Future<Output = std::io::Result<()>> + Send + 'a>>;

/// Registered readiness source for a watch file descriptor.
///
/// After the file descriptor becomes readable, an implementation must invoke `drain` before it
/// acknowledges the readiness event to the runtime. The callback must be invoked exactly once
/// before the returned future completes successfully, and must not be invoked if the future
/// completes with an error.
pub trait AsyncFdRegistration: Send + Sync {
    fn readable_and_drain<'a>(
        &'a self,
        drain: &'a mut (dyn for<'fd> FnMut(AsyncFdRef<'fd>) + Send + 'a),
    ) -> AsyncFdReadableFuture<'a>;
}

#[cfg(feature = "async-io")]
pub struct AsyncIo;

#[cfg(feature = "tokio")]
pub struct Tokio;

#[cfg(all(feature = "tokio", unix, not(target_os = "android")))]
impl AsyncFdAdapter for Tokio {
    fn register(fd: AsyncFd) -> io::Result<Box<dyn AsyncFdRegistration>> {
        Ok(Box::new(tokio::io::unix::AsyncFd::new(fd.into_owned_fd())?))
    }
}

#[cfg(all(feature = "tokio", unix, not(target_os = "android")))]
impl AsyncFdRegistration for tokio::io::unix::AsyncFd<OwnedFd> {
    fn readable_and_drain<'a>(
        &'a self,
        drain: &'a mut (dyn for<'fd> FnMut(AsyncFdRef<'fd>) + Send + 'a),
    ) -> AsyncFdReadableFuture<'a> {
        Box::pin(async move {
            let mut guard = self.readable().await?;
            drain(AsyncFdRef::from_borrowed_fd(guard.get_inner().as_fd()));
            guard.clear_ready();
            Ok(())
        })
    }
}

#[cfg(all(feature = "tokio", any(windows, target_os = "android")))]
impl AsyncFdAdapter for Tokio {
    fn register(_fd: AsyncFd) -> std::io::Result<Box<dyn AsyncFdRegistration>> {
        unreachable!("Tokio AsyncFd registration is not used on this platform")
    }
}

#[cfg(all(feature = "async-io", unix, not(target_os = "android")))]
impl AsyncFdAdapter for AsyncIo {
    fn register(fd: AsyncFd) -> io::Result<Box<dyn AsyncFdRegistration>> {
        Ok(Box::new(async_io::Async::new(fd.into_owned_fd())?))
    }
}

#[cfg(all(feature = "async-io", unix, not(target_os = "android")))]
impl AsyncFdRegistration for async_io::Async<OwnedFd> {
    fn readable_and_drain<'a>(
        &'a self,
        drain: &'a mut (dyn for<'fd> FnMut(AsyncFdRef<'fd>) + Send + 'a),
    ) -> AsyncFdReadableFuture<'a> {
        Box::pin(async move {
            self.readable().await?;
            drain(AsyncFdRef::from_borrowed_fd(self.get_ref().as_fd()));
            Ok(())
        })
    }
}

#[cfg(all(feature = "async-io", any(windows, target_os = "android")))]
impl AsyncFdAdapter for AsyncIo {
    fn register(_fd: AsyncFd) -> std::io::Result<Box<dyn AsyncFdRegistration>> {
        unreachable!("async-io AsyncFd registration is not used on this platform")
    }
}

#[cfg(all(
    test,
    unix,
    not(target_os = "android"),
    any(feature = "async-io", feature = "tokio")
))]
mod tests {
    use std::os::fd::{AsRawFd, OwnedFd};
    use std::os::unix::net::UnixDatagram;

    use nix::errno::Errno;
    use nix::sys::socket::{recv, MsgFlags};

    use super::{AsyncFd, AsyncFdAdapter};

    async fn registration_drains_and_rearms<A: AsyncFdAdapter>() {
        let (reader, writer) = UnixDatagram::pair().unwrap();
        reader.set_nonblocking(true).unwrap();
        let registration = A::register(AsyncFd::from_owned_fd(OwnedFd::from(reader))).unwrap();

        for expected_drain_count in 1..=2 {
            writer.send(&[expected_drain_count]).unwrap();

            let mut drain_count = 0;
            registration
                .readable_and_drain(&mut |fd| {
                    let mut buffer = [0_u8; 8];
                    loop {
                        match recv(fd.as_fd().as_raw_fd(), &mut buffer, MsgFlags::empty()) {
                            Ok(_) => continue,
                            Err(Errno::EAGAIN) => break,
                            Err(err) => panic!("failed to drain test socket: {err}"),
                        }
                    }
                    drain_count += 1;
                })
                .await
                .unwrap();

            assert_eq!(drain_count, 1);
        }
    }

    #[cfg(feature = "tokio")]
    #[test]
    fn tokio_registration_drains_and_rearms() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(registration_drains_and_rearms::<super::Tokio>());
    }

    #[cfg(feature = "async-io")]
    #[test]
    fn async_io_registration_drains_and_rearms() {
        async_io::block_on(registration_drains_and_rearms::<super::AsyncIo>());
    }
}
