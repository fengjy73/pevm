#!/usr/bin/env bash
# Soft=0 per-core scan for SEQ / OCC / SpecFence.
#
# Pins `workers` to C distinct physical cores (one cpu per core, SMT siblings
# skipped via `lscpu -p`). Refuses C above the physical-core count.
# 8-on-4 is only available as an explicit oversubscription contrast:
#   scripts/soft0_percore_scan.sh --oversub 8:0,1,2,3
#
# Timed region is the engine entry after the tx list is built:
#   seq  execute_revm_sequential
#   occ  execute_revm_parallel / Occ
#   sf   execute_revm_parallel / SpecFence / run_sf_block
# No untimed warm-up. Every one of K runs is a fresh engine. Oracle reuse is
# off unless --oracle-k is set, and those rows are labeled oracle.
#
# Focus blocks only: 15274915 and 3356896.
#
# Usage:
#   scripts/soft0_percore_scan.sh
#   scripts/soft0_percore_scan.sh --c-list 1,2,4 --k 10 --profile-k 10
#   scripts/soft0_percore_scan.sh --oversub 8:0,1,2,3 --k 10
#   scripts/soft0_percore_scan.sh --extra-probes --c-list 1,4 --k 10 --profile-k 3
#
# Step offsets: one OCC workers=1 trace (SPECFENCE_STEP_TRACE=1) is written to
# $OUT/step-trace.jsonl. Ideal_step(C) is simulated from that file plus the
# same-timer per-tx costs. --skip-existing reuses wall/profile/trace files.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

C_LIST="1,2,4,8,16,32,64,128,256"
K=10
PROFILE_K=10
ORACLE_K=0
OVERSUB=""
EXTRA=0
OUT="results/soft0-execute-inflation/scan"
SKIP_BUILD=0
FORCE=1
STEP_K=3
TIMEOUT_S=180

while [[ $# -gt 0 ]]; do
  case "$1" in
    --c-list) C_LIST="$2"; shift 2 ;;
    --k) K="$2"; shift 2 ;;
    --profile-k) PROFILE_K="$2"; shift 2 ;;
    --oracle-k) ORACLE_K="$2"; shift 2 ;;
    --oversub) OVERSUB="$2"; shift 2 ;;
    --extra-probes) EXTRA=1; shift ;;
    --out) OUT="$2"; shift 2 ;;
    --skip-build) SKIP_BUILD=1; shift ;;
    --skip-existing) FORCE=0; shift ;;
    --step-k) STEP_K="$2"; shift 2 ;;
    --timeout-s) TIMEOUT_S="$2"; shift 2 ;;
    -h|--help)
      sed -n '2,28p' "$0"
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
    cargo +stable build -p pevm --release --config 'profile.release.lto=false' --example specfence_inflation_dig
  else
    cargo build -p pevm --release --config 'profile.release.lto=false' --example specfence_inflation_dig
  fi
fi

if [[ ! -x "$BIN" ]]; then
  echo "missing binary $BIN" >&2
  exit 1
fi

physical_cpus() {
  if command -v lscpu >/dev/null 2>&1; then
    # First hardware thread of each (socket, core). Skips SMT siblings.
    lscpu -p=CPU,CORE,SOCKET | awk -F, 'NF && $1 !~ /^#/ { k=$3":"$2; if (!seen[k]++) print $1 }'
    return
  fi
  local cpu core sib first
  for cpu in /sys/devices/system/cpu/cpu[0-9]*; do
    [[ -f "$cpu/topology/thread_siblings_list" ]] || continue
    sib="$(cat "$cpu/topology/thread_siblings_list")"
    first="${sib%%,*}"
    id="${cpu##*cpu}"
    if [[ "$id" == "$first" ]]; then
      echo "$id"
    fi
  done
}

mapfile -t PHYS < <(physical_cpus)
if [[ "${#PHYS[@]}" -eq 0 ]]; then
  echo "no physical cpus discovered" >&2
  exit 1
fi

{
  echo "physical_cpus=${PHYS[*]}"
  echo "nproc=$(nproc)"
  echo "loadavg=$(cat /proc/loadavg 2>/dev/null || true)"
  echo "model=$(awk -F: '/model name/{print $2; exit}' /proc/cpuinfo | sed 's/^ //')"
  if [[ -r /sys/devices/system/cpu/cpu0/cpufreq/scaling_governor ]]; then
    echo "governor=$(cat /sys/devices/system/cpu/cpu0/cpufreq/scaling_governor)"
  else
    echo "governor=unavailable"
  fi
  lscpu 2>/dev/null || true
} > "$OUT/host.txt"

join_by_comma() {
  local IFS=,
  echo "$*"
}

clear_probe_env() {
  unset SPECFENCE_INFLATION || true
  unset SPECFENCE_INFLATION_READS || true
  unset SPECFENCE_INFLATION_OS || true
  unset SPECFENCE_INFLATION_PERF || true
  unset SPECFENCE_INFLATION_DAG || true
  unset SPECFENCE_INFLATION_DUMP || true
  unset SPECFENCE_INFLATION_ALLOC || true
  unset SPECFENCE_INTERP_SPLIT || true
  unset SPECFENCE_STEP_TRACE || true
  unset SPECFENCE_INFLATION_BLOCKS || true
  unset SPECFENCE_INFLATION_ENGINES || true
}

run_bin() {
  local workers="$1"
  local cpus_csv="$2"
  local outfile="$3"
  local k="$4"
  local mode="$5" # wall | profile | probes
  local seq_cpu="${cpus_csv%%,*}"
  clear_probe_env
  export SPECFENCE_COMPARE_CORES="$workers"
  export SPECFENCE_PIN_CPUS="$cpus_csv"
  export SPECFENCE_INFLATION_SEQ_CPU="$seq_cpu"
  export SPECFENCE_INFLATION_WHICH=scan
  export SPECFENCE_INFLATION_K="$k"
  if [[ "$mode" == wall ]]; then
    export SPECFENCE_INFLATION_ORACLE_K="$ORACLE_K"
  else
    export SPECFENCE_INFLATION_ORACLE_K=0
  fi
  export SPECFENCE_INFLATION_OUT="$outfile"
  export SPECFENCE_INFLATION_SEED=1
  case "$mode" in
    wall)
      ;;
    profile)
      export SPECFENCE_INFLATION=1
      export SPECFENCE_INFLATION_DAG=1
      export SPECFENCE_INFLATION_OS=1
      export SPECFENCE_INFLATION_DUMP=1
      ;;
    probes)
      export SPECFENCE_INFLATION=1
      export SPECFENCE_INFLATION_DAG=1
      export SPECFENCE_INFLATION_OS=1
      export SPECFENCE_INFLATION_READS=1
      export SPECFENCE_INFLATION_PERF=1
      export SPECFENCE_INFLATION_DUMP=1
      export SPECFENCE_INTERP_SPLIT=1
      ;;
    *)
      echo "bad mode $mode" >&2
      exit 2
      ;;
  esac
  echo "RUN mode=$mode workers=$workers cpus=$cpus_csv k=$k out=$outfile"
  set +e
  timeout --foreground "$TIMEOUT_S" taskset -c "$cpus_csv" "$BIN"
  local rc=$?
  set -e
  if [[ "$rc" -eq 124 ]]; then
    echo "TIMEOUT mode=$mode workers=$workers cpus=$cpus_csv k=$k out=$outfile after ${TIMEOUT_S}s" | tee -a "$OUT/hang.txt"
  elif [[ "$rc" -ne 0 ]]; then
    echo "FAIL rc=$rc mode=$mode workers=$workers out=$outfile" | tee -a "$OUT/hang.txt"
    return "$rc"
  fi
}

collect_step_trace() {
  local trace="$OUT/step-trace.jsonl"
  if [[ -s "$trace" && "$FORCE" -eq 0 ]]; then
    echo "reuse step trace $trace"
    return
  fi
  local cpu="${PHYS[0]}"
  clear_probe_env
  export SPECFENCE_STEP_TRACE=1
  export SPECFENCE_INFLATION_WHICH=steptrace
  export SPECFENCE_COMPARE_CORES=1
  export SPECFENCE_PIN_CPUS="$cpu"
  export SPECFENCE_INFLATION_SEQ_CPU="$cpu"
  export SPECFENCE_INFLATION_K="$STEP_K"
  export SPECFENCE_INFLATION_OUT="$trace"
  echo "STEPTRACE cpu=$cpu k=$STEP_K out=$trace"
  set +e
  timeout --foreground $(( TIMEOUT_S > 1200 ? TIMEOUT_S : 1200 )) taskset -c "$cpu" "$BIN"
  local rc=$?
  set -e
  if [[ "$rc" -ne 0 ]]; then
    echo "STEPTRACE rc=$rc" | tee -a "$OUT/hang.txt"
  fi
}

emit_json() {
  local workers="$1"
  local wall="$2"
  local profile="$3"
  local seq_profile="$4"
  local out_json="$5"
  local seq_arg=()
  local step_arg=()
  if [[ -n "$seq_profile" && -f "$seq_profile" ]]; then
    seq_arg=(--seq-profile "$seq_profile")
  fi
  if [[ -s "$OUT/step-trace.jsonl" ]]; then
    step_arg=(--step-trace "$OUT/step-trace.jsonl")
  fi
  python3 "$ROOT/scripts/specfence_inflation_report.py" \
    --wall "$wall" \
    --profile "$profile" \
    --cores "$workers" \
    --block-dir "$ROOT/data/ethereum/blocks" \
    --out "$out_json" \
    "${seq_arg[@]}" \
    "${step_arg[@]}"
}

# Oversubscription contrast (question D only). Not part of the TPS curve.
if [[ -n "$OVERSUB" ]]; then
  workers="${OVERSUB%%:*}"
  cpus_csv="${OVERSUB#*:}"
  tag="oversub-w${workers}-c${cpus_csv//,/-}"
  run_bin "$workers" "$cpus_csv" "$OUT/${tag}-wall.jsonl" "$K" wall
  run_bin "$workers" "$cpus_csv" "$OUT/${tag}-profile.jsonl" "$PROFILE_K" profile
  emit_json "$workers" "$OUT/${tag}-wall.jsonl" "$OUT/${tag}-profile.jsonl" "$OUT/profile-c1.jsonl" "$OUT/${tag}.json"
  echo "oversub json: $OUT/${tag}.json"
  exit 0
fi

collect_step_trace

SEQ_PROFILE=""
for c in ${C_LIST//,/ }; do
  if (( c > ${#PHYS[@]} )); then
    echo "SKIP C=$c : host has ${#PHYS[@]} physical cores; refusing oversubscription"
    continue
  fi
  mapfile -t chosen < <(printf '%s\n' "${PHYS[@]:0:c}")
  cpus_csv="$(join_by_comma "${chosen[@]}")"
  wall="$OUT/wall-c${c}.jsonl"
  profile="$OUT/profile-c${c}.jsonl"
  if [[ -s "$wall" && "$FORCE" -eq 0 ]]; then
    echo "reuse $wall"
  else
    run_bin "$c" "$cpus_csv" "$wall" "$K" wall
  fi
  if [[ -s "$profile" && "$FORCE" -eq 0 ]]; then
    echo "reuse $profile"
  else
    run_bin "$c" "$cpus_csv" "$profile" "$PROFILE_K" profile
  fi
  if [[ "$c" -eq 1 ]]; then
    SEQ_PROFILE="$profile"
  fi
  emit_json "$c" "$wall" "$profile" "${SEQ_PROFILE:-$profile}" "$OUT/C${c}.json"
  if [[ "$EXTRA" -eq 1 ]]; then
    run_bin "$c" "$cpus_csv" "$OUT/probes-c${c}.jsonl" "$PROFILE_K" probes
  fi
done

python3 "$ROOT/scripts/specfence_inflation_report.py" \
  --curves "$OUT" \
  --out "$OUT/curves.json"

# Product seq==par check. This one uses Pevm::execute (includes the gas / n_tx gate).
if (( ${#PHYS[@]} >= 1 )); then
  mapfile -t pin4 < <(printf '%s\n' "${PHYS[@]:0:$(( ${#PHYS[@]} < 4 ? ${#PHYS[@]} : 4 ))}")
  cpus_csv="$(join_by_comma "${pin4[@]}")"
  clear_probe_env
  export SPECFENCE_INFLATION_WHICH=seqcheck
  export SPECFENCE_COMPARE_CORES="${#pin4[@]}"
  export SPECFENCE_PIN_CPUS="$cpus_csv"
  export SPECFENCE_INFLATION_SEQCHECK_N=10
  for b in 15274915 3356896; do
    export SPECFENCE_COMPARE_BLOCK="$b"
    echo "SEQCHECK block=$b workers=${#pin4[@]} cpus=$cpus_csv"
    taskset -c "$cpus_csv" "$BIN" | tee "$OUT/seqcheck-${b}.txt"
  done
fi

echo "curves: $OUT/curves.json"
echo "host: $OUT/host.txt"
