// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// kani-flags: -Z mem-predicates
//! Check that Kani's memory predicates work for ADT with metadata.
#![feature(ptr_metadata)]

extern crate kani;

use kani::mem::{can_dereference, can_write};

#[derive(Clone, Copy, kani::Arbitrary)]
struct Wrapper<T: ?Sized> {
    _size: usize,
    _value: T,
}

mod valid_access {
    use super::*;
    #[kani::proof]
    #[cfg(blah)]
    pub fn check_valid_dyn_ptr() {
        let mut var: Wrapper<u64> = kani::any();
        let fat_ptr: *mut Wrapper<dyn PartialEq<u64>> = &mut var as *mut _;
        assert!(can_write(fat_ptr));
    }
}

mod invalid_access {
    use super::*;
    use std::ptr;
    #[kani::proof]
    #[kani::should_panic]
    #[cfg(blah)]
    pub fn check_invalid_dyn_ptr() {
        unsafe fn new_dead_ptr<T>(val: T) -> *const T {
            let local = val;
            &local as *const _
        }

        let raw_ptr: *const Wrapper<dyn PartialEq<u8>> =
            unsafe { new_dead_ptr::<Wrapper<u8>>(kani::any()) };
        assert!(can_dereference(raw_ptr));
    }

    #[kani::proof]
    pub fn check_arbitrary_metadata() {
        let mut var: Wrapper<[u64; 4]> = kani::any();
        let fat_ptr: *mut Wrapper<[u64]> = &mut var as *mut _;
        let (thin_ptr, size) = fat_ptr.to_raw_parts();
        let new_size: usize = kani::any();
        let new_ptr: *const [u64] = ptr::from_raw_parts(thin_ptr, new_size);
        if new_size <= size {
            assert!(can_dereference(new_ptr));
        } else {
            assert!(!can_dereference(new_ptr));
        }
    }

    #[kani::proof]
    pub fn check_arbitrary_metadata_is_sound() {
        let mut var: Wrapper<[u64; 4]> = kani::any();
        let fat_ptr: *mut Wrapper<[u64]> = &mut var as *mut _;
        let (thin_ptr, size) = fat_ptr.to_raw_parts();
        let new_size: usize = size + 1;
        let new_ptr: *const [u64] = ptr::from_raw_parts(thin_ptr, new_size);
        assert!(can_dereference(new_ptr));
    }
}
