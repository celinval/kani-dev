#!/usr/bin/env bash
# Copyright Kani Contributors
# SPDX-License-Identifier: Apache-2.0 OR MIT

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
check-cbmc-version.py --major 5 --minor 64
check-cbmc-viewer-version.py --major 3 --minor 5

# Formatting check
${SCRIPT_DIR}/kani-fmt.sh --check

# Build all packages in the workspace
cargo build --workspace

# Unit tests
cargo test -p cprover_bindings
cargo test -p kani-compiler
cargo test -p kani-driver

# Check prototype
echo "Check prototype"
time "$KANI_DIR"/tests/prototype/run.sh

# Skip cargo tests for this propotype
# Declare testing suite information (suite and mode)
TESTS=(
    "kani kani"
    "expected expected"
    "ui expected"
    "firecracker kani"
    "prusti kani"
    "smack kani"
    "kani-fixme kani-fixme"
)

# Extract testing suite information and run compiletest
for testp in "${TESTS[@]}"; do
  testl=($testp)
  suite=${testl[0]}
  mode=${testl[1]}
  echo "Check compiletest suite=$suite mode=$mode"
  # Note: `cargo-kani` tests fail if we do not add `$(pwd)` to `--build-base`
  # Tracking issue: https://github.com/model-checking/kani/issues/755
  cargo run -p compiletest --quiet -- --suite $suite --mode $mode --quiet
done


echo
echo "Prototype passed"
echo
