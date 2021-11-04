// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0 OR MIT

/// Stub signatures for std::mem methods.

#[inline(never)]
#[rustc_diagnostic_item = "RmcMemSwap"]
pub fn swap<T>(_x: &mut T, _y: &mut T) {}

#[inline(never)]
#[rustc_diagnostic_item = "RmcMemReplace"]
pub fn replace<T>(_dest: &mut T, _src: T) -> T {
    unimplemented!("RMC mem::swap")
}
