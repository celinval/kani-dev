// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
//
//! Ensure we compute fail verification if user tries to compute the size of a foreign item.

#![feature(extern_types)]

extern "C" {
    type ExternalType;
}

#[kani::proof]
#[kani::should_panic]
fn check_adjusted_tup_size() {
    let tup: (u32, usize) = kani::any();
    let ptr: *const (u32, ExternalType) = &tup as *const _ as *const _;
    let _size = std::mem::size_of_val(&ptr);
}
