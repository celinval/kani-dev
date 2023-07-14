#!/usr/bin/env bash
# Copyright Kani Contributors
# SPDX-License-Identifier: Apache-2.0 OR MIT
# Run compiler benchmarks (only build time)

set -o pipefail
set -o nounset

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" >/dev/null 2>&1 && pwd )"
source ${SCRIPT_DIR}/kani-perf-setup.sh
prep_build_perf
run_benchmarks
exit_code=$?
cleanup_perf

echo
if [ $exit_code -eq 0 ]; then
  echo "All Kani perf tests completed successfully."
else
  echo "***Kani perf tests failed."
fi
echo
exit $exit_code
