use std::future::poll_fn;
use std::sync::{Arc, Condvar, Mutex};
use std::task::{Poll, Waker};

use crate::List;

#[derive(Default)]
pub(crate) struct AsyncCallbackState {
    latest: Option<List>,
    waker: Option<Waker>,
}

pub(crate) struct AsyncCallbackQueue {
    state: Mutex<AsyncCallbackState>,
    ready: Condvar,
}

pub(crate) fn empty_async_callback_queue() -> AsyncCallbackQueue {
    AsyncCallbackQueue::default()
}

impl Default for AsyncCallbackQueue {
    fn default() -> Self {
        Self {
            state: Mutex::new(AsyncCallbackState::default()),
            ready: Condvar::new(),
        }
    }
}

pub(crate) type SharedAsyncCallbackQueue = Arc<AsyncCallbackQueue>;

pub(crate) fn shared_async_callback_queue() -> SharedAsyncCallbackQueue {
    Arc::new(empty_async_callback_queue())
}

pub(crate) fn push_async_list(queue: &SharedAsyncCallbackQueue, list: List) {
    let mut state = queue.state.lock().unwrap();
    state.latest = Some(list);
    if let Some(waker) = state.waker.take() {
        waker.wake();
    }
    queue.ready.notify_one();
}

pub(crate) async fn next_async_list(queue: &SharedAsyncCallbackQueue) -> List {
    poll_fn(|cx| {
        let mut state = queue.state.lock().unwrap();
        if let Some(list) = state.latest.take() {
            Poll::Ready(list)
        } else {
            state.waker = Some(cx.waker().clone());
            Poll::Pending
        }
    })
    .await
}

pub(crate) fn wait_next_list(queue: &SharedAsyncCallbackQueue) -> List {
    let mut state = queue.state.lock().unwrap();
    loop {
        if let Some(list) = state.latest.take() {
            return list;
        }
        state = queue.ready.wait(state).unwrap();
    }
}
