#!/usr/bin/env bash
# Copyright Kani Contributors
# SPDX-License-Identifier: Apache-2.0 OR MIT

# Build a custom sysroot for Kani compiler.
# Rustc expects the sysroot to have a specific folder layout:
# ${SYSROOT}/rustlib/<target-triplet>/lib/<libraries>

set -eu

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" >/dev/null 2>&1 && pwd )"
ROOT_DIR=$(dirname "$SCRIPT_DIR")

# We don't cross-compile. Target is the same as the host.
TARGET=$(rustc -vV | awk '/^host/ { print $2 }')
TARGET_DIR="${ROOT_DIR}/target/library-build"
OUT_DIR="${1:-"${ROOT_DIR}/target/lib"}"
# Rust toolchain expects a specific format.
STD_OUT_DIR="${OUT_DIR}/rustlib/${TARGET}/lib/"
mkdir -p "${TARGET_DIR}"
mkdir -p "${OUT_DIR}"
mkdir -p "${STD_OUT_DIR}"

# Build Kani libraries with custom std.
cd "${ROOT_DIR}"
# note: build.hostflags isn't working.
RUSTFLAGS="-Z always-encode-mir --cfg=kani" \
    cargo build -v -Z unstable-options \
    --out-dir="${OUT_DIR}" \
    -Z target-applies-to-host \
    -Z host-config \
    -Z build-std=panic_abort,std,test \
    --target ${TARGET} \
    -p kani \
    -p std \
    -p kani_macros \
    --target-dir "${TARGET_DIR}" \
    --profile dev \
    --config 'profile.dev.panic="abort"' \
    --config 'host.rustflags=["--cfg=kani"]'

# Copy std and dependencies to expected path.
echo "Copy deps to ${OUT_DIR}"
cp -r "${TARGET_DIR}"/${TARGET}/debug/deps/*rlib "${OUT_DIR}"

# Link to src
STD_SRC="$(rustc --print sysroot)/lib/rustlib/src"
ln -f -s "$STD_SRC" "${OUT_DIR}/rustlib/src"

# Build kani here for now since there's an expected order.
cargo build
