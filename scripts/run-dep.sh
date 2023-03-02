# !/usr/bin/env bash
set -e
export KANI_LOG=debug,kani_driver=trace
for i in {1..100}; do
    echo "==== Attempt $i/100 ==="
    cargo run -p compiletest -- --mode cargo-kani --suite cargo-kani/dependencies --force-rerun
done
