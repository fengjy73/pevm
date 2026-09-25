#!/usr/bin/env bash
# One fresh SpecFence run plus the carry-over run, per block and core count.
# Prints the in-block consult summary. Does not redesign SpecFence.
set -euo pipefail
cd "$(dirname "$0")/.."
BIN="${BIN:-target/release/examples/specfence_inflation_dig}"
export SPECFENCE_INBLOCK_TRACE=1
export SPECFENCE_INFLATION_WHICH=inblock
PIN="${SPECFENCE_PIN_CPUS:-0-3}"
for cores in 4 8; do
  for block in 15274915 3356896; do
    echo "===== block=${block} cores=${cores} ====="
    SPECFENCE_COMPARE_CORES="$cores" \
      SPECFENCE_COMPARE_BLOCK="$block" \
      taskset -c "$PIN" "$BIN"
  done
done
