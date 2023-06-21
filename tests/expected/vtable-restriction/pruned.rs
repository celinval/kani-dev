// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// kani-flags: --enable-unstable --restrict-vtable
//! Test that vtable function pointer restrictions prunes the number of candidates for vtable
//! method resolution.

/// Define a trait with two methods with the same signature.
trait FooBar {
    fn foo(&self);
    fn bar(&self);
}

/// Define concrete types.
struct Concrete1(char);
struct Concrete2(bool);

impl FooBar for Concrete1 {
    fn foo(&self) {
        kani::cover!(true, "Concrete1::foo");
    }
    fn bar(&self) {
        kani::cover!(true, "Concrete1::bar");
    }
}

impl FooBar for Concrete2 {
    fn foo(&self) {
        kani::cover!(true, "Concrete2::foo");
    }
    fn bar(&self) {
        kani::cover!(true, "Concrete2::bar");
    }
}

#[kani::proof]
pub fn check_foo() {
    let foobar: Box<dyn FooBar> = if kani::any() {
        Box::new(Concrete1(kani::any()))
    } else {
        Box::new(Concrete2(kani::any()))
    };
    foobar.foo();
}
