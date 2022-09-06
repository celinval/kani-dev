// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

use std::env;
use std::path::PathBuf;
use std::process::Command;

macro_rules! path_str {
    ($input:expr) => {
        String::from(
            $input
                .iter()
                .collect::<PathBuf>()
                .to_str()
                .unwrap_or_else(|| panic!("Invalid path {}", stringify!($input))),
        )
    };
}

/// Build the target library, and setup cargo to rerun them if the source has changed.
fn setup_libs() {
    // Compile kani library and export KANI_LIB_PATH variable with its relative location.
    let out_dir = env::var("OUT_DIR").unwrap();
    let lib_out = path_str!([&out_dir, "lib"]);
    println!("cargo:rustc-env=KANI_LIB_PATH={}", lib_out);

    let kani_libs = vec!["..", "library"];
    println!("cargo:rerun-if-changed={}", path_str!(kani_libs));

    let result = Command::new("cp")
        .args(vec!["-r", "../target/lib", &lib_out])
        .status()
        .expect("Make sure you run ${KANI_ROOT}/scripts/build-sysroot.sh");
    if !result.success() {
        std::process::exit(1);
    }
}

/// Configure the compiler to build kani-compiler binary. We currently support building
/// kani-compiler with nightly only. We also link to the rustup rustc_driver library for now.
pub fn main() {
    // Prints each argument on a separate line
    for var in env::vars() {
        println!("{} = {}", var.0, var.1);
    }

    // Add rustup to the rpath in order to properly link with the correct rustc version.
    let rustup_home = env::var("RUSTUP_HOME").unwrap();
    let rustup_tc = env::var("RUSTUP_TOOLCHAIN").unwrap();
    let rustup_lib = path_str!([&rustup_home, "toolchains", &rustup_tc, "lib"]);
    println!("cargo:rustc-link-arg-bin=kani-compiler=-Wl,-rpath,{}", rustup_lib);

    // While we hard-code the above for development purposes, for a release/install we look
    // in a relative location for a symlink to the local rust toolchain
    let origin = if cfg!(target_os = "macos") { "@loader_path" } else { "$ORIGIN" };
    println!("cargo:rustc-link-arg-bin=kani-compiler=-Wl,-rpath,{}/../toolchain/lib", origin);

    setup_libs();
}
