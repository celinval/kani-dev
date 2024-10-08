// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! This test checks that the regions after the `debug_assert` macro are
//! `UNCOVERED`. In fact, for this example, the region associated to `"This
//! should fail and stop the execution"` is also `UNCOVERED` because the macro
//! calls span two regions each.

#[kani::proof]
fn main() {
    for i in 0..4 {
        debug_assert!(i > 0, "This should fail and stop the execution");
        assert!(i == 0, "This should be unreachable");
    }
}
