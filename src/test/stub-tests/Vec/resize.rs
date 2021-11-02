// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0 OR MIT
// rmc-flags: --use-abs --abs-type rmc
fn main() {
    fn resize_test() {
        let mut vec = rmc_vec![1];
        vec.resize(3, 2);
        assert!(vec == [1, 2, 2]);

        let mut vec = rmc_vec![1, 2, 3, 4];
        vec.resize(2, 0);
        assert!(vec == [1, 2]);
    }

    resize_test();
}