use std::future::poll_fn;
use std::sync::{Arc, Mutex};
use std::task::{Poll, Waker};

use crate::List;

#[derive(Default)]
pub(crate) struct AsyncCallbackState {
    latest: Option<List>,
    waker: Option<Waker>,
}

pub(crate) type AsyncCallbackQueue = Arc<Mutex<AsyncCallbackState>>;

pub(crate) fn empty_async_callback_queue() -> AsyncCallbackQueue {
    Arc::new(Mutex::new(AsyncCallbackState::default()))
}

pub(crate) fn push_async_list(queue: &AsyncCallbackQueue, list: List) {
    let mut state = queue.lock().unwrap();
    state.latest = Some(list);
    if let Some(waker) = state.waker.take() {
        waker.wake();
    }
}

pub(crate) async fn next_async_list(queue: &AsyncCallbackQueue) -> List {
    poll_fn(|cx| {
        let mut state = queue.lock().unwrap();
        if let Some(list) = state.latest.take() {
            Poll::Ready(list)
        } else {
            state.waker = Some(cx.waker().clone());
            Poll::Pending
        }
    })
    .await
}
