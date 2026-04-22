use std::{future::Future, pin::Pin};

#[cfg(not(any(target_os = "linux", target_vendor = "apple")))]
use std::marker::PhantomData;

#[cfg(all(
    any(feature = "async-io", feature = "tokio"),
    any(target_os = "linux", target_vendor = "apple")
))]
use std::io;
#[cfg(all(
    any(feature = "async-io", feature = "tokio"),
    any(target_os = "linux", target_vendor = "apple")
))]
use std::os::fd::AsFd;
#[cfg(any(target_os = "linux", target_vendor = "apple"))]
use std::os::fd::{BorrowedFd, OwnedFd};

/// Runtime-owned readiness source passed to an [`AsyncFdAdapter`].
///
/// On Linux and Apple platforms this wraps the nonblocking watch file descriptor.
/// On other platforms the value is never used.
pub struct AsyncFd {
    #[cfg(any(target_os = "linux", target_vendor = "apple"))]
    inner: OwnedFd,
    #[cfg(not(any(target_os = "linux", target_vendor = "apple")))]
    _private: (),
}

impl AsyncFd {
    #[cfg(any(target_os = "linux", target_vendor = "apple"))]
    pub(crate) fn from_owned_fd(inner: OwnedFd) -> Self {
        Self { inner }
    }

    #[cfg(any(target_os = "linux", target_vendor = "apple"))]
    pub fn into_owned_fd(self) -> OwnedFd {
        self.inner
    }
}

/// Borrowed readiness source returned by an [`AsyncFdReadyGuard`].
///
/// On Linux and Apple platforms this wraps the watch file descriptor.
/// On other platforms the value is never used.
pub struct AsyncFdRef<'a> {
    #[cfg(any(target_os = "linux", target_vendor = "apple"))]
    inner: BorrowedFd<'a>,
    #[cfg(not(any(target_os = "linux", target_vendor = "apple")))]
    _marker: PhantomData<&'a ()>,
}

impl<'a> AsyncFdRef<'a> {
    #[cfg(any(target_os = "linux", target_vendor = "apple"))]
    pub fn from_borrowed_fd(inner: BorrowedFd<'a>) -> Self {
        Self { inner }
    }

    #[cfg(any(target_os = "linux", target_vendor = "apple"))]
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

pub type AsyncFdReadableFuture<'a> =
    Pin<Box<dyn Future<Output = std::io::Result<Box<dyn AsyncFdReadyGuard + 'a>>> + 'a>>;

/// Registered readiness source for a watch file descriptor.
pub trait AsyncFdRegistration: Send + Sync {
    fn readable(&self) -> AsyncFdReadableFuture<'_>;
}

/// Guard returned once the runtime reports the watch file descriptor as readable.
pub trait AsyncFdReadyGuard {
    fn fd(&self) -> AsyncFdRef<'_>;
    fn clear_ready(&mut self);
}

#[cfg(feature = "async-io")]
pub struct AsyncIo;

#[cfg(feature = "tokio")]
pub struct Tokio;

#[cfg(all(feature = "tokio", any(target_os = "linux", target_vendor = "apple")))]
impl AsyncFdAdapter for Tokio {
    fn register(fd: AsyncFd) -> io::Result<Box<dyn AsyncFdRegistration>> {
        Ok(Box::new(tokio::io::unix::AsyncFd::new(fd.into_owned_fd())?))
    }
}

#[cfg(all(feature = "tokio", any(target_os = "linux", target_vendor = "apple")))]
impl AsyncFdRegistration for tokio::io::unix::AsyncFd<OwnedFd> {
    fn readable(&self) -> AsyncFdReadableFuture<'_> {
        Box::pin(async move {
            let guard = self.readable().await?;
            Ok(Box::new(guard) as Box<dyn AsyncFdReadyGuard>)
        })
    }
}

#[cfg(all(feature = "tokio", any(target_os = "linux", target_vendor = "apple")))]
impl AsyncFdReadyGuard for tokio::io::unix::AsyncFdReadyGuard<'_, OwnedFd> {
    fn fd(&self) -> AsyncFdRef<'_> {
        AsyncFdRef::from_borrowed_fd(self.get_inner().as_fd())
    }

    fn clear_ready(&mut self) {
        tokio::io::unix::AsyncFdReadyGuard::clear_ready(self);
    }
}

#[cfg(all(feature = "tokio", any(windows, target_os = "android")))]
impl AsyncFdAdapter for Tokio {
    fn register(_fd: AsyncFd) -> std::io::Result<Box<dyn AsyncFdRegistration>> {
        unreachable!("Tokio AsyncFd registration is not used on this platform")
    }
}

#[cfg(all(
    feature = "async-io",
    any(target_os = "linux", target_vendor = "apple")
))]
impl AsyncFdAdapter for AsyncIo {
    fn register(fd: AsyncFd) -> io::Result<Box<dyn AsyncFdRegistration>> {
        Ok(Box::new(AsyncIoRegistration(async_io::Async::new(
            fd.into_owned_fd(),
        )?)))
    }
}

#[cfg(all(
    feature = "async-io",
    any(target_os = "linux", target_vendor = "apple")
))]
struct AsyncIoRegistration(async_io::Async<OwnedFd>);

#[cfg(all(
    feature = "async-io",
    any(target_os = "linux", target_vendor = "apple")
))]
struct AsyncIoReadyGuard<'a>(&'a async_io::Async<OwnedFd>);

#[cfg(all(
    feature = "async-io",
    any(target_os = "linux", target_vendor = "apple")
))]
impl AsyncFdRegistration for AsyncIoRegistration {
    fn readable(&self) -> AsyncFdReadableFuture<'_> {
        Box::pin(async move {
            self.0.readable().await?;
            Ok(Box::new(AsyncIoReadyGuard(&self.0)) as Box<dyn AsyncFdReadyGuard>)
        })
    }
}

#[cfg(all(
    feature = "async-io",
    any(target_os = "linux", target_vendor = "apple")
))]
impl AsyncFdReadyGuard for AsyncIoReadyGuard<'_> {
    fn fd(&self) -> AsyncFdRef<'_> {
        AsyncFdRef::from_borrowed_fd(self.0.get_ref().as_fd())
    }

    fn clear_ready(&mut self) {}
}

#[cfg(all(feature = "async-io", any(windows, target_os = "android")))]
impl AsyncFdAdapter for AsyncIo {
    fn register(_fd: AsyncFd) -> std::io::Result<Box<dyn AsyncFdRegistration>> {
        unreachable!("async-io AsyncFd registration is not used on this platform")
    }
}
