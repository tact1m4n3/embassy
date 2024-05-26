use core::{
    cell::RefCell,
    future::{poll_fn, Future},
    task::Poll,
};

use crate::{
    blocking_mutex::{raw::RawMutex, Mutex},
    waitqueue::MultiWakerRegistration,
};

pub struct Watch<M: RawMutex, T: Clone, const R: usize> {
    state: Mutex<M, RefCell<State<T, R>>>,
}

impl<M: RawMutex, T: Clone, const R: usize> Watch<M, T, R> {
    pub const fn new() -> Self {
        Self {
            state: Mutex::new(RefCell::new(State {
                value: None,
                version: Version::new(),
                wakers: MultiWakerRegistration::new(),
                receiver_count: 0,
            })),
        }
    }

    pub fn sender(&self) -> Sender<'_, M, T, R> {
        Sender { watch: self }
    }

    pub fn receiver(&self) -> Receiver<'_, M, T, R> {
        self.state.lock(|s| {
            let mut s = s.borrow_mut();
            if s.receiver_count >= R {
                panic!("too many receivers. you better increase the max number");
            }
            s.receiver_count += 1;
            Receiver { watch: self, version: s.version }
        })
    }
}

pub struct Sender<'a, M: RawMutex, T: Clone, const R: usize> {
    watch: &'a Watch<M, T, R>,
}

impl<M: RawMutex, T: Clone, const R: usize> Sender<'_, M, T, R> {
    pub fn send(&self, value: T) {
        self.watch.state.lock(|s| {
            let mut s = s.borrow_mut();
            s.value = Some((value, s.receiver_count));
            s.version.update();
            s.wakers.wake();
        });
    }
}

pub struct Receiver<'a, M: RawMutex, T: Clone, const R: usize> {
    watch: &'a Watch<M, T, R>,
    version: Version,
}

impl<M: RawMutex, T: Clone, const R: usize> Receiver<'_, M, T, R> {
    pub fn wait(&mut self) -> impl Future<Output = T> + '_ {
        poll_fn(|ctx| {
            self.watch.state.lock(|s| {
                let mut s = s.borrow_mut();

                if self.version != s.version {
                    if let Some((value, remaining)) = s.value.take() {
                        if remaining > 0 {
                            s.value = Some((value.clone(), remaining - 1));
                        }
                        self.version = s.version;
                        return Poll::Ready(value)
                    }
                }

                s.wakers.register(ctx.waker()).expect("should not have happened");
                Poll::Pending
            })
        })
    }
}

struct State<T: Clone, const R: usize> {
    value: Option<(T, usize)>,
    version: Version,
    wakers: MultiWakerRegistration<R>,
    receiver_count: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Version {
    value: u32,
}

impl Version {
    pub const fn new() -> Self {
        Self { value: 0 }
    }

    pub fn update(&mut self) {
        self.value = self.value.wrapping_add(1);
    }
}
