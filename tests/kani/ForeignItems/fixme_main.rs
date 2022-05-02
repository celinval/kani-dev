// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
//! To run this test, do
//! kani fixme_main.rs -- lib.c

// kani-flags: --c-lib lib.c

// TODO, we should also test packed and transparent representations
// https://doc.rust-lang.org/reference/type-layout.html
#[repr(C)]
pub struct Foo {
    i: u32,
    c: u8,
}

// TODO:
//#[repr(packed)]
#[repr(C)]
pub struct Foo2 {
    i: u32,
    c: u8,
    i2: u32,
}

// https://doc.rust-lang.org/reference/items/external-blocks.html
// https://doc.rust-lang.org/nomicon/ffi.html
extern "C" {
    // NOTE: this currently works even if I don't make S static, but is UB.
    // We should have a check for that.
    static mut S: u32;

    fn update_static();
    fn takes_int(i: u32) -> u32;
    fn takes_ptr(p: &u32) -> u32;
    // In rust, you say nullable pointer by using option of reference.
    // Rust guarantees that this has the bitwise represntation
    // Some(&x) => &x;
    // None => NULL;
    // FIXME: we need to notice when this happens and do a bitcast, or C is unhappy
    // https://github.com/model-checking/kani/issues/3
    fn takes_ptr_option(p: Option<&u32>) -> u32;
    fn mutates_ptr(p: &mut u32);
    #[link_name = "name_in_c"]
    fn name_in_rust(i: u32) -> u32;
    fn takes_struct(f: Foo) -> u32;
    fn takes_struct_ptr(f: &Foo) -> u32;
    fn takes_struct2(f: Foo2) -> u32;
    fn takes_struct_ptr2(f: &Foo2) -> u32;
}

#[kani::proof]
fn main() {
    unsafe {
        assert!(S == 12);
        update_static();
        assert!(S == 13);

        assert!(takes_int(1) == 3);
        assert!(takes_ptr(&5) == 7);
        //assert!(takes_ptr_option(Some(&5)) == 4);
        //assert!(takes_ptr_option(None) == 0);
        let mut p = 17;
        mutates_ptr(&mut p);
        assert!(p == 16);

        assert!(name_in_rust(2) == 4);

        let f = Foo { i: 12, c: 7 };
        assert!(takes_struct_ptr(&f) == 19);
        assert!(takes_struct(f) == 19);

        let f2 = Foo2 { i: 12, c: 7, i2: 8 };
        assert!(takes_struct_ptr2(&f2) == 19);
        assert!(takes_struct2(f2) == 19);
    }
}
