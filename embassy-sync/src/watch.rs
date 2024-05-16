use core::{
    cell::Cell,
    future::poll_fn,
    task::Poll,
    future::Future,
};

use crate::{
    blocking_mutex::{raw::RawMutex, Mutex},
    waitqueue::MultiWakerRegistration,
};

pub struct Watch<M: RawMutex, T: Clone, const N: usize> {
    state: Mutex<M, Cell<State<T, N>>>,
}

impl<M: RawMutex, T: Clone, const N: usize> Watch<M, T, N> {
    pub const fn new() -> Self {
        Self {
            state: Mutex::new(Cell::new(State::Waiting {
                wakers: MultiWakerRegistration::new(),
            })),
        }
    }

    pub fn set(&self, value: T) {
        self.state.lock(|state| {
            if let State::Waiting { mut wakers } = state.take() {
                state.set(State::Ready {
                    num_wakers: wakers.len(),
                    value,
                });
                wakers.wake();
            }
        });
    }

    pub fn wait(&self) -> impl Future<Output = T> + '_ {
        poll_fn(move |ctx| {
            self.state.lock(|state| match state.take() {
                State::Waiting { mut wakers } => {
                    if wakers.register(ctx.waker()).is_err() {
                        panic!("Maximum number of wakers reached");
                    }

                    state.set(State::Waiting { wakers });
                    Poll::Pending
                }
                State::Ready { num_wakers, value } if num_wakers > 0 => {
                    state.set(State::Ready {
                        num_wakers: num_wakers - 1,
                        value: value.clone(),
                    });
                    Poll::Ready(value)
                }
                State::Ready { value, .. } => Poll::Ready(value),
            })
        })
    }
}

enum State<T: Clone, const N: usize> {
    Waiting { wakers: MultiWakerRegistration<N> },
    Ready { num_wakers: usize, value: T },
}

impl<T: Clone, const N: usize> Default for State<T, N> {
    fn default() -> Self {
        Self::Waiting {
            wakers: MultiWakerRegistration::new(),
        }
    }
}
