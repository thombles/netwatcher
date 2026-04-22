#[cfg(all(
    any(feature = "async-io", feature = "tokio"),
    any(target_os = "linux", target_vendor = "apple")
))]
use std::io;
#[cfg(all(
    any(feature = "async-io", feature = "tokio"),
    any(target_os = "linux", target_vendor = "apple")
))]
use std::os::fd::{AsFd, OwnedFd};

#[cfg(feature = "async-io")]
pub struct AsyncIo;

#[cfg(feature = "tokio")]
pub struct Tokio;

#[cfg(all(feature = "tokio", any(target_os = "linux", target_vendor = "apple")))]
impl crate::AsyncFdAdapter for Tokio {
    fn register(fd: OwnedFd) -> io::Result<Box<dyn crate::AsyncFdRegistration>> {
        Ok(Box::new(tokio::io::unix::AsyncFd::new(fd)?))
    }
}

#[cfg(all(feature = "tokio", any(target_os = "linux", target_vendor = "apple")))]
impl crate::AsyncFdRegistration for tokio::io::unix::AsyncFd<OwnedFd> {
    fn readable(&self) -> crate::AsyncFdReadableFuture<'_> {
        Box::pin(async move {
            let guard = self.readable().await?;
            Ok(Box::new(guard) as Box<dyn crate::AsyncFdReadyGuard>)
        })
    }
}

#[cfg(all(feature = "tokio", any(target_os = "linux", target_vendor = "apple")))]
impl crate::AsyncFdReadyGuard for tokio::io::unix::AsyncFdReadyGuard<'_, OwnedFd> {
    fn fd(&self) -> std::os::fd::BorrowedFd<'_> {
        self.get_inner().as_fd()
    }

    fn clear_ready(&mut self) {
        tokio::io::unix::AsyncFdReadyGuard::clear_ready(self);
    }
}

#[cfg(all(feature = "tokio", any(windows, target_os = "android")))]
impl crate::AsyncFdAdapter for Tokio {}

#[cfg(all(
    feature = "async-io",
    any(target_os = "linux", target_vendor = "apple")
))]
impl crate::AsyncFdAdapter for AsyncIo {
    fn register(fd: OwnedFd) -> io::Result<Box<dyn crate::AsyncFdRegistration>> {
        Ok(Box::new(AsyncIoRegistration(async_io::Async::new(fd)?)))
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
impl crate::AsyncFdRegistration for AsyncIoRegistration {
    fn readable(&self) -> crate::AsyncFdReadableFuture<'_> {
        Box::pin(async move {
            self.0.readable().await?;
            Ok(Box::new(AsyncIoReadyGuard(&self.0)) as Box<dyn crate::AsyncFdReadyGuard>)
        })
    }
}

#[cfg(all(
    feature = "async-io",
    any(target_os = "linux", target_vendor = "apple")
))]
impl crate::AsyncFdReadyGuard for AsyncIoReadyGuard<'_> {
    fn fd(&self) -> std::os::fd::BorrowedFd<'_> {
        self.0.get_ref().as_fd()
    }

    fn clear_ready(&mut self) {}
}

#[cfg(all(feature = "async-io", any(windows, target_os = "android")))]
impl crate::AsyncFdAdapter for AsyncIo {}
