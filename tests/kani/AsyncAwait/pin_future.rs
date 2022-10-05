// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// compile-flags: --edition 2018
// kani-flags: --legacy-linker

//! This file tests a hand-written spawn infrastructure and executor.
//! This should be replaced with code from the Kani library as soon as the executor can get merged.
//! Tracking issue: https://github.com/model-checking/kani/issues/1685

use std::{
    future::Future,
    pin::Pin,
    task::{Context, RawWaker, RawWakerVTable, Waker},
};

/// A dummy waker, which is needed to call [`Future::poll`]
const NOOP_RAW_WAKER: RawWaker = {
    #[inline]
    unsafe fn clone_waker(_: *const ()) -> RawWaker {
        NOOP_RAW_WAKER
    }

    #[inline]
    unsafe fn noop(_: *const ()) {}

    RawWaker::new(std::ptr::null(), &RawWakerVTable::new(clone_waker, noop, noop, noop))
};

const MAX_TASKS: usize = 16;

type BoxFuture = Pin<Box<dyn Future<Output = ()> + Sync + 'static>>;

/// Polls the given future and the tasks it may spawn until all of them complete
///
/// Contrary to block_on, this allows `spawn`ing other futures
pub fn spawnable_block_on<F: Future<Output = ()> + Sync + 'static>(fut: F) -> BoxFuture {
    Box::pin(fut)
}

pub fn poll(pin_fut: &mut BoxFuture) {
    let waker = unsafe { Waker::from_raw(NOOP_RAW_WAKER) };
    let cx = &mut Context::from_waker(&waker);
    pin_fut.as_mut().poll(cx);
}

#[kani::proof]
#[kani::unwind(4)]
fn arc_spawn_deterministic_test() {
    let num = 10;
    let mut pin_fut = spawnable_block_on(async move {
        assert_eq!(num, 10);
    });
    poll(&mut pin_fut);
}
