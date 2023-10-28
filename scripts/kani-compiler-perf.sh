#!/usr/bin/env bash
# Copyright Kani Contributors
# SPDX-License-Identifier: Apache-2.0 OR MIT

# Compare the compiler performance for two different Kani versions
set -e
set -u
export RUST_BACKTRACE=1

#================ INPUTS (env var) =====================
# Location of the Kani repository (git URL) -- use local by default
KANI_REPO="${KANI_REPO:-"$(git rev-parse --show-toplevel)"}"

# Folder where we keep our run
RUN_DIR="${RUN_DIR:-"/tmp/kani_comp_$(date +%s)"}"

OLD_ID="${ORIG_ID:-"kani-0.18.0"}"
NEW_ID="${NEW_ID:-"HEAD"}"

#============ Other constants ===================

mkdir -p "${RUN_DIR}"
RUN_DIR="$(realpath ${RUN_DIR})"
LOG_DIR="${RUN_DIR}/logs"
NEW_DIR="${RUN_DIR}/old"
OLD_DIR="${RUN_DIR}/new"
ORIG_PATH="${PATH}"
mkdir -p "${LOG_DIR}"

#============ Functions ==================

function log() {
    tee "${LOG_DIR}/$1"
}

function log_append() {
    tee -a "${LOG_DIR}/$1"
}

function clone() {
    pushd ${RUN_DIR} > /dev/null
    if [[ ! -e "${RUN_DIR}/kani-repo" ]]; then
        git clone ${KANI_REPO} kani-repo
    fi

    pushd kani-repo > /dev/null
    # Create worktree with branch new-ID
    if [[ ! -e "${NEW_DIR}" ]]; then
        git worktree add -b "new-${NEW_ID}" "${NEW_DIR}" ${NEW_ID}
    fi
    if [[ ! -e "${OLD_DIR}" ]]; then
        git worktree add -b "old-${OLD_ID}" "${OLD_DIR}" ${OLD_ID}
    fi
    popd > /dev/null

    popd > /dev/null
}

function build() {
    echo "Build new"
    pushd ${RUN_DIR}/old > /dev/null
    cargo build-dev --release 2>&1 | log new_build.log
    popd


    echo "Build old"
    pushd ${RUN_DIR}/new > /dev/null
    cargo build-dev --release 2>&1 | log old_build.log
    popd
}

# Arg 1: Path to cargo.toml
# Arg 2..N: Extra arguments for Kani
function run_test() {
    pkg_path=$1
    shift
    kani_args="${@}"

    pushd ${pkg_path} > /dev/null

    pkg_name=$(cargo get package.name)
    echo "Run: ${pkg_name} ($(pwd))" | log tests/${pkg_name}_new.log
    set +u
    export PATH="${NEW_DIR}/scripts:${ORIG_PATH}"
    cargo clean --target-dir target_new
    cargo kani --target-dir target_new ${kani_args[@]} 2>&1 | log_append tests/${pkg_name}_new.log
    new_result=$?
    set -u

    echo "Run: ${pkg_name} ($(pwd))" | log tests/${pkg_name}_old.log
    set +u
    export PATH="${OLD_DIR}/scripts:${ORIG_PATH}"
    cargo clean --target-dir target_old
    cargo kani --target-dir target_old ${kani_args[@]} 2>&1 | log_append tests/${pkg_name}_old.log
    old_result=$?
    set -u

    export PATH="${ORIG_PATH}"
    echo "${pkg_name}, ${new_result}, ${old_result}" | log_append statuses.csv
    popd
}

function build_time() {
    log_file=$1
    grep "Finished.*target(s) in " "${log_file}" | sed 's/.*in \([0-9.]*\)s/\1/'
}

# Arg 1: Path to cargo.toml
function post_process() {
    pkg_path=$1
    pushd ${pkg_path} > /dev/null
    pkg_name=$(cargo get package.name)

    new_result=$(build_time "${LOG_DIR}/tests/${pkg_name}_new.log")
    old_result=$(build_time "${LOG_DIR}/tests/${pkg_name}_old.log")
    echo "${pkg_name}, ${new_result}, ${old_result}" | log_append build_time.csv
}


#============= Main ==================

cd ${RUN_DIR}

echo "
===============================
== Run kani-compiler script:
==  REPO: ${KANI_REPO}
==  RUN_DIR: ${RUN_DIR}
==
== Compare:
==  OLD: ${OLD_ID}
==  NEW: ${NEW_ID}
===============================
" | log inputs.log

# Use cargo get for debugging
cargo install cargo-get

clone
build

TESTS=(
    "tests/cargo-kani/itoa_dep"
    "tests/cargo-kani/mir-linker"
    "tests/cargo-kani/firecracker-block-example"
)

rm -rf "${LOG_DIR}/tests"
mkdir -p "${LOG_DIR}/tests"

echo "Package, ${NEW_ID}, ${OLD_ID}" | log statuses.csv
echo "Package, ${NEW_ID}, ${OLD_ID}" | log build_time.csv

for test in "${TESTS[@]}"; do
    echo "
===============================
== ${test}
===============================
    "
    run_test "${NEW_DIR}/${test}" --only-codegen
    post_process "${NEW_DIR}/${test}"
done


echo "
===============================
== Finished execution
==
== Build times (in seconds)
===============================
$(cat ${LOG_DIR}/build_time.csv)
===============================
"

