# SpecFence resolve stabilize status

**Date:** 2026-09-07 (Asia/Shanghai)  
**Branch:** `specfence`  
**Base tip:** `c0117e9` (cold SpecRead + value-stable RebindOnly)  
**Authority:** `specfence-native-resolve-protocol.md`, `specfence-resolve-wall-push-status.md`

---

## Goals

1. Stabilize measurement: G7 reports **median + p90** over N≥5 (`SPECFENCE_G7_ITERS`).
2. Cut **median** 597 wall below tip-noisy ~34ms (stretch ≤22 / ≤15 not required for this landing).
3. Widen resolve that avoids EVM reexec; thin hot π; park→steal without SoftWait storms.

---

## What changed

### 1. Multi-run harness (measurement)

- `specfence_g7_smoke`: `SPECFENCE_G7_ITERS=N` runs each SF/OCC cores row N times with `reset_inter_prior` each run.
- Exports `wall_ms_median`, `wall_ms_p90`, `wall_ms_min`, `wall_ms_mean`, `soft_wait_arms_median`, `occ_aborts_median`, plus `multi_run_597`.
- Optimize / compare on **median wall**, not lucky single-run best.

### 2. Value-stable RebindOnly widen

- Storage value-stable unchanged.
- **Basic** value-stable when snap `balance`+`nonce` equal current live `MemoryValue::Basic` (Estimate → `None`, no skip). Lazy/multi-origin still refused by `try_rebind`.
- Prefer RebindOnly on Estimate→Data same-output without SuffixRepair when value-stable.

### 3. Hot π thin (DashMap thrash)

- `force_prefix` / sticky+Data / cold SpecRead decide **before** HotSet `note_observe` / `try_revoke` / full `choose_resolve`.
- Cold path no longer pays learner observe tax.

### 4. Bind-on-Data (SpecFence-native)

- `choose_action`: published MV **Data → Bind** always (Bind arm does **not** SoftWait; WaitHard remains the only Await verb).
- Removes EV_Bind branch that competed Spec/Wait when Data already existed.

### 5. Park→steal (no SoftWait increase)

- Validation abort: `wave.push_ready(aborted_tx)` so SoftWait/EarlyAbort park workers can steal Ready work.
- `execution_idx.fetch_min` only when index already past the aborted tx (avoid low-idx reexec stampede / abort cascades).
- Tried Estimate validation-deferral → **hung** unit tests; **reverted** (prefer RebindOnly once Data is live via value-stable path instead).

---

## Validation

| Check | Result |
|-------|--------|
| `cargo test -p pevm --lib` | **91 passed** |
| `cargo test -p pevm --test specfence` | **23 passed**, 13 ignored |
| Hang 597/599? | **No** |
| seq≡par | green |
| SoftWait 597 (median) | **33** ≪428 (≤80) |

### 597 @8 — multi-run vs tip `c0117e9`

| Metric | tip note (noisy) | **stabilize N=7** | Δ |
|--------|-----------------:|------------------:|---|
| wall median | ~34 | **29.2** | **↓ ~14%** |
| wall p90 | — | **33.9** | ≤ tip median |
| wall min | ~22 best / ~29 tip min | **23.3** | near best-case floor |
| OCC wall median | ~3.7 | **3.3** | — |
| SoftWait median | scarce | **33** | scarce ✓ |
| abort median | noisy ~331 | **263** | still schedule-noisy |
| SF/OCC mean (4 blocks) | — | **0.310** | |

**N=5** earlier same build: median **30.1** / p90 37.5 / min 23.9 — consistent with N=7.

Stretch median &lt;15 / &lt;10 **not** met; abort variance still large on bad schedules. Next levers: more RebindOnly hits (`rebind_only` still rare), cheaper park resume (cut `park_resume_full_retry`), further cascade damping.

---

## Artifacts

- `lab/results/resolve-stabilize-sf-occ.json` (iters=7)
- `lab/results/resolve-stabilize-flip.json`
- `lab/results/resolve-stabilize-smoke7.run.log` (and smoke3 N=5)

## Code

- `crates/pevm/examples/specfence_g7_smoke.rs` — multi-run median/p90
- `crates/pevm/src/pevm.rs` — Basic value-stable RebindOnly
- `crates/pevm/src/vm.rs` — thin force_prefix/sticky/cold before π DashMap
- `crates/pevm/src/specfence/resolve.rs` — Bind-on-Data
- `crates/pevm/src/scheduler.rs` — abort → `push_ready` steal
- `crates/pevm/src/specfence/rem.rs` — `steal_after_park_pending` helper
