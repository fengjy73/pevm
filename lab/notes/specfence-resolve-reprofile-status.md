# SpecFence resolve — reprofile after ESTIMATE cascade + residual wall cut

**Date:** 2026-09-07 (Asia/Shanghai)  
**Branch:** `specfence`  
**Base tip:** `169eece`  
**Authority:** `specfence-native-resolve-protocol.md`, estimate-cascade + profile-strip status notes

## Mandate

Re-profile after abort→ESTIMATE→BlockingOther cascade cut; attack remaining ~21ms wall gap (best median ~24.9 vs OCC ~3.4). SoftWait scarce; do not restore Wait storms / SpecRead-through-writer / hung finish-under-lock.

---

## Phase 1 — Fresh profile (`SPECFENCE_PROFILE=1`, 597 @8, N=7)

Artifact: `lab/results/resolve-reprofile-597.json` (from `resolve-reprofile-sf-occ.json`).

### Bucket table (SF CPU-ms **sum across workers**)

| Bucket | ade501d profile | **169eece fresh** | Note |
|--------|----------------:|------------------:|------|
| Handler::run (incl. maybe_wait) | 157 | **124** | ↓ |
| maybe_wait / π meta | 125 | **102** | ≈82% of SF handler |
| Pure EVM (handler − maybe_wait) | 32 | **22** | Not the gap |
| validate + SuffixRepair/Rebind | 73 | **39** | Halved |
| scheduler next_task | ~1 | ~1 | |
| park idle **total** | 391 | **407** | Still dominates |
| — SoftWait Soft | (unsplit) | **44** / 53 parks | Scarce |
| — EarlyAbort | — | **0** | |
| — **BlockingOther** | (unsplit) | **363** / 185 parks | **Root** |
| Wall median (profile Instant tax) | 30.6 | 35.3 | Tax biases SF up |
| Non-profile wall (cascade best) | — | **~24.9** vs OCC **~3.4** | ~21ms gap |

### Current root of ~21ms gap

**Residual `park_idle_blocking_other`** (ESTIMATE / aborted-incarnation / cold Blocking while writer re-executes) — SoftWait Soft scarce; maybe_wait meta secondary; validate already improved.

Evidence: BO 363ms ≈89% of park idle; ready_steal 294 when Ready, residual idle = writer Executing after ESTIMATE; SoftWait arms 57≪428.

---

## Phase 2 — Structural cuts (landed)

1. **`last_data_before` truly skips ESTIMATE** (OrderedDirtyRead continue past marker + aborted).
2. **MV basic/storage:** SpecFence skip *leading* ESTIMATE/aborted → prior Data (mid-lazy-chain ESTIMATE still Blocks). Storage ESTIMATE with no prior → pre-state SpecRead (no BlockingOther).
3. **`maybe_wait` ESTIMATE short-circuit:** SpecRead — never SoftWait/BO on ESTIMATE writer.
4. **`maybe_wait` Await short-circuit:** unfinished live writer + sticky/prior/hot → `WaitHard` **without** `note_observe` / `try_revoke` / `choose_action` DashMap tax.

Not restored: cold-hint SpecRead strip (prior revert), SpecRead-through-writer, Wait storms, finish-under-lock.

---

## Validation

| Check | Result |
|-------|--------|
| `cargo test -p pevm --lib` | **92 passed** |
| `cargo test -p pevm --test specfence` | **23 passed**, 13 ignored |
| SoftWait 597 median | **125–132** ≪428 |
| Hang | **No** |
| seq≡par | green |

### 597 @8 vs `169eece`

| Metric | 169eece best | **this N=7** | **this N=11** |
|--------|-------------:|-------------:|--------------:|
| wall median | **24.9** | **22.1** | **22.6** |
| wall p90 | — | 23.4 | **24.7** |
| wall min | 22.7 | **21.7** | **21.3** |
| OCC median | ~3.4 | 3.5 | 3.6 |
| SoftWait med | 48 | 132 | **125** |
| abort med | 117 | 154 | 157 |
| BO idle ms (last) | 212–387 | **~2** | **~2** |
| park idle total (profile) | 407 | — | **~20** (after) |

**SUCCESS:** Median wall **clearly below ~25** (22.1–22.6 vs 24.9); BO idle collapsed; SoftWait scarce; tests green; no hang. Stretch &lt;15 **not** met — residual SoftWait Soft idle + maybe_wait meta (~91ms profiled) still own the OCC gap.

### After-cut profile buckets (597 last iter, Instant on)

| Bucket | before cut | after |
|--------|----------:|------:|
| handler | 124 | 115 |
| maybe_wait | 102 | 91 |
| validate | 39 | 32 |
| park total | 407 | **19.5** |
| BO idle | 363 | **0.7** |
| SoftWait Soft idle | 44 | 18.8 |

---

## Artifacts

- `lab/results/resolve-reprofile-597.json` — diagnosis + phase2
- `lab/results/resolve-reprofile-sf-occ.json` / `*-smoke7.run.log` — Phase 1 profile
- `lab/results/resolve-reprofile-wall-sf-occ.json` / `resolve-reprofile-wall11-sf-occ.json`
- `lab/results/resolve-reprofile-after-sf-occ.json` — post-ODR profile
- `lab/results/resolve-reprofile-final-sf-occ.json` / `*-meta-smoke7.run.log`

## Code

- `crates/pevm/src/mv_memory.rs` — `last_data_before` skips ESTIMATE
- `crates/pevm/src/vm.rs` — ODR past ESTIMATE/aborted; ESTIMATE + Await meta short-circuits
