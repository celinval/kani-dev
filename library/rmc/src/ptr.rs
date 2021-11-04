// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0 OR MIT

/// Stub signatures for std::ptr methods.

#[inline(never)]
#[rustc_diagnostic_item = "RmcPtrRead"]
pub unsafe fn read<T>(_src: *const T) -> T {
    unimplemented!("RMC ptr::read")
}

#[inline(never)]
#[rustc_diagnostic_item = "RmcPtrWrite"]
pub unsafe fn write<T>(_dst: *mut T, _src: T) {}
