#!/usr/bin/env bash
# Copyright Kani Contributors
# SPDX-License-Identifier: Apache-2.0 OR MIT

# To enable "unsound_experimental features, run as follows:
# `KANI_ENABLE_UNSOUND_EXPERIMENTS=1 scripts/kani-regression.sh`

if [[ -z $KANI_REGRESSION_KEEP_GOING ]]; then
  set -o errexit
fi
set -o pipefail
set -o nounset

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" >/dev/null 2>&1 && pwd )"
export PATH=$SCRIPT_DIR:$PATH
EXTRA_X_PY_BUILD_ARGS="${EXTRA_X_PY_BUILD_ARGS:-}"
KANI_DIR=$SCRIPT_DIR/..

# This variable forces an error when there is a mismatch on the expected
# descriptions from cbmc checks.
# TODO: We should add a more robust mechanism to detect python unexpected behavior.
export KANI_FAIL_ON_UNEXPECTED_DESCRIPTION="true"

# Required dependencies
check-cbmc-version.py --major 5 --minor 77
check-cbmc-viewer-version.py --major 3 --minor 5
check_kissat_version.sh

# Formatting check
${SCRIPT_DIR}/kani-fmt.sh --check

# Build all packages in the workspace
if [[ "" != "${KANI_ENABLE_UNSOUND_EXPERIMENTS-}" ]]; then
  cargo build-dev -- --features unsound_experiments
else
  cargo build-dev
fi

# Unit tests
cargo test -p cprover_bindings
cargo test -p kani-compiler
cargo test -p kani-driver
cargo test -p kani_metadata

# Declare testing suite information (suite and mode)
TESTS=(
    "script-based-pre exec"
    "kani kani"
    "expected expected"
    "ui expected"
    "firecracker kani"
    "prusti kani"
    "smack kani"
    "cargo-kani cargo-kani"
)

if [[ "" != "${KANI_ENABLE_UNSOUND_EXPERIMENTS-}" ]]; then
  TESTS+=("unsound_experiments kani")
else
  TESTS+=("no_unsound_experiments expected")
fi

export KANI_LOG=debug,kani_driver=trace

# Build compiletest and print configuration. We pick suite / mode combo so there's no test.
echo "--- Compiletest configuration"
cargo run -p compiletest --quiet -- --suite kani --mode cargo-kani --dry-run --verbose
echo "-----------------------------"

# Extract testing suite information and run compiletest
for i in {1..10}; do
    echo "==== Attempt $i/10 ==="
    for testp in "${TESTS[@]}"; do
      testl=($testp)
      suite=${testl[0]}
      mode=${testl[1]}
      echo "Check compiletest suite=$suite mode=$mode"
      cargo run -p compiletest --quiet -- --suite $suite --mode $mode \
          --quiet --no-fail-fast
    done
done

echo
echo "All Kani regression tests completed successfully."
echo
