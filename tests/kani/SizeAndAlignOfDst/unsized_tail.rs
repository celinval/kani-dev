// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
//
//! Ensure we compute the size correctly including padding.

use std::fmt::Debug;

#[derive(kani::Arbitrary)]
struct Pair<T, U: ?Sized>(T, U);
#[kani::proof]
fn check_adjusted_size_slice() {
    let tup: Pair<[u8; 5], [u16; 3]> = kani::any();
    let size = std::mem::size_of_val(&tup);

    let unsized_tup: *const Pair<[u8; 5], [u16]> = &tup as *const _ as *const _;
    let adjusted_size = std::mem::size_of_val(unsafe { &*unsized_tup });

    assert_eq!(size, adjusted_size);
}

#[kani::proof]
fn check_adjusted_size_dyn() {
    let tup: Pair<u32, [u8; 5]> = kani::any();
    let size = std::mem::size_of_val(&tup);

    let unsized_tup: *const Pair<u32, dyn Debug> = &tup as *const _ as *const _;
    let adjusted_size = std::mem::size_of_val(unsafe { &*unsized_tup });

    assert_eq!(size, adjusted_size);
}
