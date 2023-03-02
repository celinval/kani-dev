# !/usr/bin/env bash
set -e
export KANI_LOG=debug,kani_driver=trace
for i in {1..50}; do
    echo "==== Attempt $i/50 ==="
    cargo run -p compiletest -- --mode cargo-kani --suite cargo-kani --force-rerun
done
