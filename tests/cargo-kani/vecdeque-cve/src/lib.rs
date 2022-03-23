// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0 OR MIT

#![feature(slice_range)]
#![feature(extend_one)]
#![feature(try_reserve_kind)]
#![feature(allocator_api)]
#![feature(dropck_eyepatch)]
#![feature(rustc_attrs)]
#![feature(core_intrinsics)]
#![feature(ptr_internals)]
#![feature(rustc_allow_const_fn_unstable)]

mod raw_vec;
mod vec_deque;

// Older version of vec_deque with reserve issue.
use crate::vec_deque::VecDeque;
//use std::collections::vec_deque::VecDeque;

const MAX_CAPACITY: usize = usize::MAX >> 1;

/// Verify that a request to reserve space to `n` elements is a no-op when there's enough capacity.
#[kani::proof]
pub fn reserve_available_capacity_is_no_op() {
    // Start with a default VecDeque object.
    // Markers:     H
    //              T
    // vec_deque: [ . . . . . . . . ]
    let mut vec_deque = VecDeque::<u8>::new();
    let old_capacity = vec_deque.capacity();

    // Insert an element to empty VecDeque.
    // Markers:     H             T
    // vec_deque: [ . . . . . . . o ]
    let front = kani::any();
    vec_deque.push_front(front);

    // Reserve space to *any* value that is less than available capacity.
    let new_capacity: usize = kani::any();
    kani::assume(new_capacity <= (old_capacity - vec_deque.len()));
    vec_deque.reserve(new_capacity);

    // Capacity should stay the same.
    assert_eq!(vec_deque.capacity(), old_capacity);
}

/// Verify that a request to reserve space to `n` elements is a no-op when there's enough capacity.
#[kani::proof]
pub fn reserve_more_capacity_ok() {
    // Start with a default VecDeque object.
    // Markers:     H
    //              T
    // vec_deque: [ . . . . . . . . ]
    let mut vec_deque = VecDeque::<u8>::new();
    let old_capacity = vec_deque.capacity();

    // Insert an element to empty VecDeque.
    // Markers:     H             T
    // vec_deque: [ . . . . . . . o ]
    let front = kani::any();
    vec_deque.push_front(front);

    // Reserve space to *any* value that is more than available capacity.
    let new_capacity: usize = kani::any();
    kani::assume(new_capacity > (old_capacity - vec_deque.len()));
    kani::assume(new_capacity <= (MAX_CAPACITY - vec_deque.len()));
    vec_deque.reserve(new_capacity);

    // Capacity should increase.
    assert!(vec_deque.capacity() > new_capacity);
}
