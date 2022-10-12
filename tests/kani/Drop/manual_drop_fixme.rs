// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// kani-verify-fail
//! A few examples of ManuallyDrop feature.
//! We currently don't support dropping structs with unsized fields.
//! https://github.com/model-checking/kani/issues/1072
use std::mem::ManuallyDrop;
use std::sync::atomic::{AtomicU8, Ordering};

pub trait DummyTrait {}

pub struct Wrapper<T: ?Sized> {
    pub w_id: u128,
    pub inner: T,
}

#[cfg(blah)]
impl<T: ?Sized> Drop for Wrapper<T> {
    fn drop(&mut self) {
        assert_eq!(self.w_id, 2);
    }
}

struct DummyImpl {
    pub id: u128,
}

impl DummyTrait for DummyImpl {}

static counter: AtomicU8 = AtomicU8::new(0);

impl Drop for DummyImpl {
    fn drop(&mut self) {
        let _ = counter.fetch_add(1, Ordering::Relaxed);
        assert_eq!(self.id, 10);
    }
}

#[kani::proof]
unsafe fn check_manual_drop() {
    let ptr = &mut ManuallyDrop::new(Wrapper { w_id: 2, inner: DummyImpl { id: 10 } });
    ManuallyDrop::drop(ptr);
    assert_eq!(counter.load(Ordering::Relaxed), 1);
}

#[kani::proof]
unsafe fn check_manual_dyn_drop() {
    let ptr: &mut ManuallyDrop<Wrapper<dyn DummyTrait>> =
        &mut ManuallyDrop::new(Wrapper { w_id: 2, inner: DummyImpl { id: 10 } });
    ManuallyDrop::drop(ptr);
    assert_eq!(counter.load(Ordering::Relaxed), 1);
}

#[kani::proof]
unsafe fn check_no_drop() {
    let _ptr: &mut ManuallyDrop<Wrapper<dyn DummyTrait>> =
        &mut ManuallyDrop::new(Wrapper { w_id: 2, inner: DummyImpl { id: 10 } });
    assert_eq!(counter.load(Ordering::Relaxed), 0);
}
