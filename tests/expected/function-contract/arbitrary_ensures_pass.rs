// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// kani-flags: -Zfunction-contracts

#[kani::ensures(|result : &u32| *result == x || *result == y)]
fn max(x: u32, y: u32) -> u32 {
    if x > y { x } else { y }
}

#[kani::proof_for_contract(max)]
fn max_harness() {
    let _ = Box::new(());
    max(kani::any(), kani::any());
}
