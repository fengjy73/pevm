#!/usr/bin/env bash
# Stage-1 scan: SEQ, upstream OCC, and SpecFence on the same whole-block boundary.
#
# Pins the process to a configurable CPU list. The real host uses CPUs 128–255
# (C up to 128). This VM passes whatever list it has, for example:
#   scripts/soft0_percore_scan.sh --cpus 0,1,2,3 --c-list 1,4,8 --k 7
# C above the list length needs --oversub C:cpu,cpu,...
#
# Every round is a fresh engine. No same-block warm-up.
# One extra SpecFence run per (block, C, class key) records the in-block trace.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

C_LIST="1,4,8"
K=7
CPUS=""
OVERSUB=""
OUT="results/specfence-v2-stage1"
KEYS="to,code_hash"
BLOCKS="15274915,3356896"
SKIP_BUILD=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --c-list) C_LIST="$2"; shift 2 ;;
    --k) K="$2"; shift 2 ;;
    --cpus) CPUS="$2"; shift 2 ;;
    --oversub) OVERSUB="$2"; shift 2 ;;
    --out) OUT="$2"; shift 2 ;;
    --keys) KEYS="$2"; shift 2 ;;
    --blocks) BLOCKS="$2"; shift 2 ;;
    --skip-build) SKIP_BUILD=1; shift ;;
    -h|--help)
      sed -n '2,16p' "$0"
      exit 0
      ;;
    *)
      echo "unknown arg: $1" >&2
      exit 2
      ;;
  esac
done

mkdir -p "$OUT"
BIN="$ROOT/target/release/examples/specfence_inflation_dig"

if [[ "$SKIP_BUILD" -eq 0 ]]; then
  if command -v rustup >/dev/null 2>&1 && rustup toolchain list 2>/dev/null | grep -q '^stable'; then
    cargo +stable build -p pevm --release --features specfence \
      --config 'profile.release.lto=false' --config 'profile.release.codegen-units=16' \
      --example specfence_inflation_dig --offline
  else
    cargo build -p pevm --release --features specfence \
      --config 'profile.release.lto=false' --config 'profile.release.codegen-units=16' \
      --example specfence_inflation_dig --offline
  fi
fi

discover_cpus() {
  if command -v lscpu >/dev/null 2>&1 && lscpu -p=CPU,CORE,SOCKET >/dev/null 2>&1; then
    lscpu -p=CPU,CORE,SOCKET | awk -F, 'NF && $1 !~ /^#/ { k=$3":"$2; if (!seen[k]++) print $1 }'
    return
  fi
  nproc
}

if [[ -z "$CPUS" ]]; then
  mapfile -t PHYS < <(discover_cpus)
  CPUS=$(IFS=,; echo "${PHYS[*]}")
fi
IFS=',' read -r -a CPU_ARR <<< "$CPUS"
if [[ "${#CPU_ARR[@]}" -eq 0 ]]; then
  echo "empty cpu list" >&2
  exit 1
fi

{
  echo "cpus=${CPUS}"
  echo "nproc=$(nproc)"
  echo "loadavg=$(cat /proc/loadavg 2>/dev/null || true)"
  echo "model=$(awk -F: '/model name/{print $2; exit}' /proc/cpuinfo | sed 's/^ //')"
  lscpu 2>/dev/null || true
} > "$OUT/host.txt"

run_pinned() {
  local cpus="$1"
  shift
  if command -v taskset >/dev/null 2>&1; then
    taskset -c "$cpus" "$@"
  else
    "$@"
  fi
}

pin_for() {
  local c="$1"
  if [[ "$c" -le "${#CPU_ARR[@]}" ]]; then
    local i picked=()
    for ((i = 0; i < c; i++)); do
      picked+=("${CPU_ARR[i]}")
    done
    (IFS=,; echo "${picked[*]}")
    return
  fi
  if [[ -n "$OVERSUB" ]]; then
    local spec="${OVERSUB%%:*}"
    local list="${OVERSUB#*:}"
    if [[ "$spec" == "$c" ]]; then
      echo "$list"
      return
    fi
  fi
  echo "C=$c exceeds cpu list (${#CPU_ARR[@]}); pass --oversub $c:cpu,cpu,..." >&2
  exit 1
}

IFS=',' read -r -a KEY_ARR <<< "$KEYS"
for c in ${C_LIST//,/ }; do
  pin="$(pin_for "$c")"
  seq_cpu="${pin%%,*}"
  # SEQ is the 1-core baseline, timed only at C=1. OCC is timed at this C.
  engines="occ"
  if [[ "$c" == "1" ]]; then
    engines="seq,occ"
  fi
  out="$OUT/wall-c${c}.jsonl"
  echo "SCAN c=$c engines=$engines pin=$pin -> $out"
  run_pinned "$pin" env \
    SPECFENCE_INFLATION_WHICH=scan \
    SPECFENCE_COMPARE_CORES="$c" \
    SPECFENCE_INFLATION_K="$K" \
    SPECFENCE_INFLATION_ENGINES="$engines" \
    SPECFENCE_INFLATION_SEQ_CPU="$seq_cpu" \
    SPECFENCE_PIN_CPUS="$pin" \
      SPECFENCE_DATA_DIR="$ROOT/data/ethereum" \
      SPECFENCE_INFLATION_BLOCKS="$BLOCKS" \
      SPECFENCE_INFLATION_OUT="$out" \
    "$BIN"
  for key in "${KEY_ARR[@]}"; do
    sf="$OUT/wall-c${c}-${key}.jsonl"
    echo "SCAN c=$c key=$key pin=$pin -> $sf"
    run_pinned "$pin" env \
      SPECFENCE_INFLATION_WHICH=scan \
      SPECFENCE_COMPARE_CORES="$c" \
      SPECFENCE_INFLATION_K="$K" \
      SPECFENCE_INFLATION_ENGINES=sf \
      SPECFENCE_INFLATION_SEQ_CPU="$seq_cpu" \
      SPECFENCE_PIN_CPUS="$pin" \
      SPECFENCE_CLASS_KEY="$key" \
      SPECFENCE_DATA_DIR="$ROOT/data/ethereum" \
      SPECFENCE_INFLATION_BLOCKS="$BLOCKS" \
      SPECFENCE_INFLATION_OUT="$sf" \
      "$BIN"
    trace="$OUT/trace-c${c}-${key}.jsonl"
    echo "TRACE c=$c key=$key -> $trace"
    run_pinned "$pin" env \
      SPECFENCE_INBLOCK_TRACE=1 \
      SPECFENCE_INFLATION_WHICH=scan \
      SPECFENCE_COMPARE_CORES="$c" \
      SPECFENCE_INFLATION_K=1 \
      SPECFENCE_INFLATION_ENGINES=sf \
      SPECFENCE_INFLATION_SEQ_CPU="$seq_cpu" \
      SPECFENCE_PIN_CPUS="$pin" \
      SPECFENCE_CLASS_KEY="$key" \
      SPECFENCE_DATA_DIR="$ROOT/data/ethereum" \
      SPECFENCE_INFLATION_BLOCKS="$BLOCKS" \
      SPECFENCE_INFLATION_OUT="$trace" \
      "$BIN"
  done
done

python3 "$ROOT/scripts/specfence_inflation_report.py" --scan-dir "$OUT" | tee "$OUT/report.txt"
