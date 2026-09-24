# Adaptive CC Redesign v1 — R0+R1+R2 implementation status

**Date:** 2026-09-06 (Asia/Shanghai)  
**Branch:** `specfence`  
**Spec:** `lab/notes/specfence-adaptive-cc-redesign-v1.md`  
**Parent tip:** `8f3fde5` (redesign freeze)

## What landed

### R0 — Strip default tax
- SpecFence production execute uses `Handler::run` unless `SPECFENCE_ENABLE_INSPECT=1`.
- Absolute jump / valued CallOutcome SC **off by default**; opt-in via dedicated flags or research inspect.
- `inspector_steps=0` on default mainnet path (confirmed in smoke).

### R1 — HotSet + default LeanOCC
- Engagement always starts LeanOCC (no prove-quiet gates). Fixes M4 `lean_mode_txs=0`.
- `HotSet` (`crates/pevm/src/specfence/hotset.rs`): insert on ≥`H_w=3` in-block writers, ≥`H_a=2` abort/ESTIMATE events, or process multi-writer prior.
- `ℓ ∉ HotSet` → OCC-style SpecRead only (no Bayes WaitHard).
- Metrics: `lean_mode_txs`, `hot_local_reads`, `hotset_size`.

### R2 — HotLocal policy
- `ℓ ∈ HotSet`: Bind if Data ready (M3 prior bind kept); else WaitHard+park (M2) when EV says wait; else SpecRead.
- Selective invalidate retained on lean abort; RewindTo/jump remain research-only.
- WaitHard only on HotSet.

### Tests
- SpecFence suite: **22 passed, 13 ignored** (M1* inspect/jump research-only; hang risk).
- New: `specfence_r1_wide_block_stays_lean`, `specfence_r1_hot_multiwriter_hotset`.
- Adapted M4/Bayes/P2 for default-Lean + HotSet.

## Smoke numbers (@8)

### Wide-like 15199017 (n=1)
| mode | TPS | evm_entries | lean_mode_txs | wait_hard | inspector | hot_local | hotset |
|------|-----|-------------|---------------|-----------|-----------|-----------|--------|
| OCC | 143523 | 894 | 0 | 0 | 0 | 0 | 0 |
| SpecFence | **37125** | **889** | **889** | **0** | **0** | 687 | 24 |

- SF/OCC ≈ **0.26** (v7 was **0.067**). Gate SF/OCC≥0.95 **not met**.
- lean high, wait_hard=0, inspector=0, evm_entries ≈ OCC — R0/R1 signals good.
- HotSet still non-empty (24) → some Bind/hot_local on multi-writer Basic locs; WaitHard killed.

### Wide-like 13217637 (n=2 mean)
| mode | TPS | evm_entries | lean | wait_hard | inspector | hotset |
|------|-----|-------------|------|-----------|-----------|--------|
| OCC | 231736 | 1379 | 0 | 0 | 0 | 0 |
| SpecFence | **33404** | **1124** | **1124** | **≈0** | **0** | 16 |

- SF/OCC ≈ **0.14**. Completes; lean high; wait_hard≈0.

### Hot 19807137 @8 (completes — **no hang**)
Smoke1 (n=1): OCC cold outlier TPS=253; SF=10037, evm 4006 vs OCC 4871.

Smoke2 (n=2): OCC r0 cold / r1=60496; SF mean TPS≈**8934**, lean=3511, wait_hard≈1649, hotset≈1068, hot_local≈6624, inspector=0.

| | SF | OCC (r1 stable) | note |
|--|----|-----------------|------|
| TPS | ~9k | ~60k | SF/OCC ≈ **0.15** (v7 **0.085**, and v7 hung) |
| evm_entries (smoke2 mean) | **3511** | 3776 | SF ≤ OCC — head-reexec not worse |
| hang | **no** | — | R0 inspect-off removes livelock |

## Gates checklist

| Gate | Status |
|------|--------|
| SpecFence@8 completes (no inspect livelock) | **PASS** on smoked blocks |
| Wide: lean/n ≥ 0.95, wait_hard≈0 | **PASS** (lean≈all incarnations; wait_hard=0) |
| Wide: SF/OCC@8 ≥ 0.95 | **MISS** (~0.14–0.26) |
| Hot: evm_entries_sf ≤ 1.05×OCC | **PASS** on smoke2 mean |
| Hot: SF/OCC ≥ 1.0 on ≥1 hot | **MISS** (~0.15) |
| inspector_steps=0 default | **PASS** |

## Remaining gaps → R3 full 7-block v8

1. **SF/OCC still ≪ 1** on wide+hot — LeanOCC removes inspect tax but HotLocal Bind/meta + remaining abort reexec still trail Block-STM OCC wall-clock.
2. Tune `H_w`/`H_a` (and Bind aggressiveness) so wide blocks keep `hotset_size` near 0 without losing hot-chain Wait/Bind.
3. Full **7-block sweep v8** (1/2/4/8 × repeats) with new metrics columns; include 19434587 hang regression check.
4. Optional: location-grain steal scheduling / further cut meta on Bind path.
5. M1* RewindTo/jump stays behind `SPECFENCE_ENABLE_INSPECT` until hang@8=0 **and** net evm_entries win.

## Files

- `crates/pevm/src/specfence/hotset.rs` — HotSet
- `crates/pevm/src/specfence/{engagement,mod,metrics,boundary}.rs` — R0/R1/R2
- `crates/pevm/src/{vm,pevm}.rs` — wire HotSet / inspect gate / abort
- `crates/pevm/tests/specfence.rs` — adapted + R1 tests
- `crates/pevm/examples/specfence_mainnet_sweep.rs` — export hot_* metrics
- `lab/results/mainnet-sweep-r0r2-smoke{,2}.json` — smoke artifacts
