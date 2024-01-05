#!/usr/bin/env bash
# Copyright Kani Contributors
# SPDX-License-Identifier: Apache-2.0 OR MIT

# Compare the compiler performance for two different Kani versions
set -e
set -u
set -o pipefail
export RUST_BACKTRACE=1
# Print the time that took to run a command

#================ INPUTS (env var) =====================
# Location of the Kani repository (git URL) -- use local by default
KANI_REPO="${KANI_REPO:-"$(git rev-parse --show-toplevel)"}"

# Folder where we keep our run
RUN_DIR="${RUN_DIR:-"/tmp/kani_comp_$(date +%s)"}"

OLD_ID="${ORIG_ID:-"kani-0.18.0"}"
NEW_ID="${NEW_ID:-"kani-0.43.0"}"

#============ Other constants ===================

mkdir -p "${RUN_DIR}"
RUN_DIR="$(realpath ${RUN_DIR})"
LOG_DIR="${RUN_DIR}/logs"
NEW_DIR="${RUN_DIR}/new"
OLD_DIR="${RUN_DIR}/old"
# We use old directory due to compatibility issues.
TEST_DIR="${NEW_DIR}/tests"
ORIG_PATH="${PATH}"
mkdir -p "${LOG_DIR}"

#============ Functions ==================

function log() {
    tee "${LOG_DIR}/$1"
}

function log_append() {
    tee -a "${LOG_DIR}/$1"
}

function etime() {
    /usr/bin/time -f "Elapsed time: %e" "$@"
}

function clone() {
    pushd ${RUN_DIR} > /dev/null
    if [[ ! -e "${RUN_DIR}/kani-repo" ]]; then
        git clone ${KANI_REPO} kani-repo
        pushd kani-repo > /dev/null
        git submodule update --init
        popd > /dev/null
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

    pushd "${NEW_DIR}" > /dev/null
    git submodule update --init
    popd > /dev/null

    pushd "${OLD_DIR}" > /dev/null
    git submodule update --init
    popd > /dev/null

    popd > /dev/null
}

function build() {
    echo "Build new"
    pushd ${RUN_DIR}/new > /dev/null
    cargo build-dev --release 2>&1 | log new_build.log
    popd > /dev/null


    echo "Build old"
    pushd ${RUN_DIR}/old > /dev/null
    cargo build-dev --release 2>&1 | log old_build.log
    popd > /dev/null
}

# Arg 1: Path to cargo.toml
# Arg 2..N: Extra arguments for Kani
function run_test() {
    pkg_path=$1
    shift
    kani_args="${@}"

    pushd ${pkg_path} > /dev/null

    # Package name does not work for workspaces... skip workspace only manifests.
    pkg_name=$(cargo get package.name || echo "skip")
    if [ "$pkg_name" == "skip" ]; then
        popd > /dev/null
        return
    fi

    # Things can fail here... e.g.: cargo kani may fail if the build is meant to fail.
    set +e

    #=== New version run
    echo "Run: ${pkg_name} ($(pwd))" | log tests/${pkg_name}_new.log
    export PATH="${NEW_DIR}/scripts:${ORIG_PATH}"
    cargo clean --target-dir target_new
    etime cargo kani --target-dir target_new ${kani_args[@]} -Z async-lib --enable-unstable 2>&1 \
        | log_append tests/${pkg_name}_new.log

    # Check recompilation after changing the target files date.
    find . -name "*.rs" -exec touch {} \;
    etime cargo kani --target-dir target_new ${kani_args[@]} -Z async-lib --enable-unstable 2>&1 \
        | log_append tests/recomp_${pkg_name}_new.log

    #=== Old version run
    echo "Run: ${pkg_name} ($(pwd))" | log tests/${pkg_name}_old.log
    export PATH="${OLD_DIR}/scripts:${ORIG_PATH}"
    cargo clean --target-dir target_old
    etime cargo kani --target-dir target_old ${kani_args[@]} --enable-unstable 2>&1 \
        | log_append tests/${pkg_name}_old.log

    # Check recompilation after changing the target files date.
    find . -name "*.rs" -exec touch {} \;
    etime cargo kani --target-dir target_old ${kani_args[@]} --enable-unstable 2>&1 \
        | log_append tests/recomp_${pkg_name}_old.log

    # Things shouldn't fail after here.
    set -e

    export PATH="${ORIG_PATH}"
    popd > /dev/null
}

function print_total() {
    total=0
    for num in ${@}; do
      total=$(awk "BEGIN{print $total + $num}")
    done
    echo $total
}

function codegen_time() {
    log_file=$1
    print_total $(grep "Finished codegen in" "${log_file}" | sed 's/.*codegen in \([0-9.]*\)s/\1/')
}

function build_time() {
    log_file=$1
    print_total $(grep "Elapsed time:" "${log_file}" | sed 's/.*: \([0-9.]*\)/\1/')
}

function build_status() {
    log_file=$1
    print_total $(grep "Command exited with non-zero status" "${log_file}"  | sed 's/.*status \([0-9]*\)/\1/' || echo 0)
}

function num_harnesses() {
    log_file=$1
    print_total $(grep "Number of harnesses" "${log_file}"  | sed 's/.*: \([0-9]*\)/\1/' || echo 0)
}

# Arg 1: Path to cargo.toml
function post_process() {
    pkg_path=$1
    pushd ${pkg_path} > /dev/null
    # Package name does not work for workspaces... skip workspace only manifests.
    pkg_name=$(cargo get package.name || echo "skip")
    if [ "$pkg_name" == "skip" ]; then
        popd > /dev/null
        return
    fi
    pkg_data ${pkg_name}
    popd > /dev/null
}

function pkg_data() {
    pkg_name=$1

    # Things can fail in here.
    set +e
    new_status=$(build_status "${LOG_DIR}/tests/${pkg_name}_new.log")
    new_result=$(build_time "${LOG_DIR}/tests/${pkg_name}_new.log")
    new_codegen=$(codegen_time "${LOG_DIR}/tests/${pkg_name}_new.log")
    new_harnesses=$(num_harnesses "${LOG_DIR}/tests/${pkg_name}_new.log")
    old_status=$(build_status "${LOG_DIR}/tests/${pkg_name}_old.log")
    old_result=$(build_time "${LOG_DIR}/tests/${pkg_name}_old.log")
    old_codegen=$(codegen_time "${LOG_DIR}/tests/${pkg_name}_old.log")
    old_harnesses=$(num_harnesses "${LOG_DIR}/tests/${pkg_name}_old.log")

    # Things shouldn't fail after here.
    set -e

    echo "${pkg_name}, ${old_status}, ${old_result}, ${old_harnesses}, ${old_codegen}, \
          ${new_status}, ${new_result}, ${new_harnesses}, ${new_codegen}," \
    | log_append build_time.csv
}

function find_cargo_tests() {
    tests_dir=$1
    pushd ${tests_dir} > /dev/null
    for manifest_path in $(find . -name Cargo.toml); do
        echo "$(dirname ${manifest_path})"
    done
    popd > /dev/null
}

function collect_results() {
    echo "
    ===============================
    == Collect results:
    ==  RUN_DIR: ${RUN_DIR}
    ==  LOGS: ${LOG_DIR}
    ==
    == Compare:
    ==  OLD: ${OLD_ID}
    ==  NEW: ${NEW_ID}
    ===============================
    " | log inputs.log

    echo "Package, ${OLD_ID} status, ${OLD_ID} time (s), ${OLD_ID} harnesses, ${OLD_ID} codegen (s), \
     ${NEW_ID} status, ${NEW_ID} time (s), ${NEW_ID} harnesses, ${NEW_ID} codegen (s)," |
    log build_time.csv

    pushd ${LOG_DIR}/tests > /dev/null

    TESTS=$(ls *_new.log)
    for test_log in ${TESTS}; do
        test="${test_log%_new.log}"
        echo $test
        pkg_data "${test}"
    done

    popd > /dev/null


    echo "
    ===============================
    == Finished execution
    ==
    == Build times (in seconds)
    ===============================
    $(cat ${LOG_DIR}/build_time.csv)
    ===============================
    "

}

function run_tests() {
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

    TESTS=$(find_cargo_tests ${TEST_DIR})

    rm -rf "${LOG_DIR}/tests"
    mkdir -p "${LOG_DIR}/tests"

    echo "Package, ${OLD_ID} status, ${OLD_ID} time (s), ${OLD_ID} harnesses, ${OLD_ID} codegen (s), \
     ${NEW_ID} status, ${NEW_ID} time (s), ${NEW_ID} harnesses, ${NEW_ID} codegen (s)," |
    log build_time.csv

    for test in ${TESTS}; do
        echo "
    ===============================
    == ${test}
    ===============================
        "
        run_test "${TEST_DIR}/${test}" --only-codegen --tests
        post_process "${TEST_DIR}/${test}"
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

}

#============= Main ==================
# This can be used to only run the log analysis
# note that tests with the same package name will override each other.
# collect_results
run_tests
