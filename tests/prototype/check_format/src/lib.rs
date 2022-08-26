// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

#[kani::proof]
#[kani::unwind(4)]
pub fn check_format() {
    assert!("2".parse::<u32>().unwrap() == 2);
}
