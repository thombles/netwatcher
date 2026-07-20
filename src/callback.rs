use std::{
    any::Any,
    panic::{catch_unwind, AssertUnwindSafe},
};

use crate::Update;

pub(crate) struct Callback {
    callback: Box<dyn FnMut(Update) + Send + 'static>,
    failed: bool,
}

impl Callback {
    pub(crate) fn new(callback: Box<dyn FnMut(Update) + Send + 'static>) -> Self {
        Self {
            callback,
            failed: false,
        }
    }

    /// Deliver the synchronous initial update.
    ///
    /// This intentionally does not catch panics: watcher construction is still on the
    /// caller's stack and owns all resources needed to unwind cleanly.
    pub(crate) fn call_initial(&mut self, update: Update) {
        (self.callback)(update);
    }

    /// Deliver an update originating from a platform notification.
    ///
    /// A callback that unwinds may have left its captured state inconsistent, so it is
    /// permanently quarantined rather than called again.
    pub(crate) fn call_from_notification(&mut self, update: Update) {
        if self.failed {
            return;
        }

        if let Err(payload) = catch_unwind(AssertUnwindSafe(|| (self.callback)(update))) {
            self.failed = true;
            drop_panic_payload(payload);
        }
    }

    #[cfg(test)]
    pub(crate) fn has_failed(&self) -> bool {
        self.failed
    }
}

/// Drop a caught panic payload without allowing its destructor to unwind.
///
/// Panic payloads can have arbitrary destructors. If dropping one panics, forget the
/// replacement payload: attempting to drop that in turn could recurse indefinitely.
fn drop_panic_payload(payload: Box<dyn Any + Send>) {
    if let Err(payload) = catch_unwind(AssertUnwindSafe(|| drop(payload))) {
        std::mem::forget(payload);
    }
}

/// Dispatch an update to a set of callbacks.
#[cfg(any(target_os = "android", test))]
pub(crate) fn dispatch_callbacks<'a>(
    callbacks: impl IntoIterator<Item = &'a mut Callback>,
    update: Update,
) {
    let mut callbacks = callbacks.into_iter().peekable();
    while let Some(callback) = callbacks.next() {
        if callbacks.peek().is_some() {
            callback.call_from_notification(update.clone());
        } else {
            callback.call_from_notification(update);
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::panic::{catch_unwind, panic_any, AssertUnwindSafe};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    use super::*;
    use crate::List;

    #[test]
    fn initial_callback_panic_propagates() {
        let mut callback = Callback::new(Box::new(|_| panic!("initial callback failed")));

        let result = catch_unwind(AssertUnwindSafe(|| {
            callback.call_initial(List::default().initial_update());
        }));

        assert!(result.is_err());
        assert!(!callback.has_failed());
    }

    #[test]
    fn dispatch_continues_after_callback_panic() {
        let failed_calls = Arc::new(AtomicUsize::new(0));
        let failed_calls_for_callback = failed_calls.clone();
        let healthy_calls = Arc::new(AtomicUsize::new(0));
        let healthy_calls_for_callback = healthy_calls.clone();

        let mut callbacks = [
            Callback::new(Box::new(move |_| {
                failed_calls_for_callback.fetch_add(1, Ordering::Relaxed);
                panic!("callback failed");
            })),
            Callback::new(Box::new(move |_| {
                healthy_calls_for_callback.fetch_add(1, Ordering::Relaxed);
            })),
        ];
        let update = List::default().initial_update();

        for _ in 0..2 {
            dispatch_callbacks(callbacks.iter_mut(), update.clone());
        }

        assert_eq!(failed_calls.load(Ordering::Relaxed), 1);
        assert!(callbacks[0].has_failed());
        assert_eq!(healthy_calls.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn dispatch_continues_when_panic_payload_drop_panics() {
        struct PanicsOnDrop;

        impl Drop for PanicsOnDrop {
            fn drop(&mut self) {
                panic!("panic payload drop failed");
            }
        }

        let healthy_calls = Arc::new(AtomicUsize::new(0));
        let healthy_calls_for_callback = healthy_calls.clone();
        let mut callbacks = [
            Callback::new(Box::new(|_| panic_any(PanicsOnDrop))),
            Callback::new(Box::new(move |_| {
                healthy_calls_for_callback.fetch_add(1, Ordering::Relaxed);
            })),
        ];

        dispatch_callbacks(callbacks.iter_mut(), List::default().initial_update());

        assert!(callbacks[0].has_failed());
        assert_eq!(healthy_calls.load(Ordering::Relaxed), 1);
    }
}
