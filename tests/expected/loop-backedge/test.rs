// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! This test checks that the unwinding assertions pass for nested loops for
//! which there's a backedge into the middle of the loop

#![feature(core_intrinsics)]
use std::ptr::addr_of;

#[kani::proof]
#[kani::unwind(3)]
fn check_unwind_assertion() {
    let a: &[i32] = &[0, 0];
    let mut iter = a.iter();
    let first = iter.next();
    let second = iter.next().unwrap();
    assert!(iter.next().is_none());
    assert_eq!(*second, 0);
}
