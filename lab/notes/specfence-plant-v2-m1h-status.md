# Plant v2 M1h status — Write-prefix jump + valued CallOutcome

**Date:** 2026-09-05 (Asia/Shanghai)  
**Branch:** `specfence` @ fengjy73/pevm  
**Tip parent:** M1g `e793a97`  
**Scope:** SpecFence path only; OCC/PCC unchanged; M2 park/steal intact

## Dual goals — honest outcomes

### A — Write-prefix absolute jump

| Piece | Status |
|-------|--------|
| `ResumeContinuation.prefix_writes` / `write_replays` | **Landed** in rem (scaffold for post-write residual republish) |
| `StorageWriteReplay { address, slot, original, present, gas_remaining_after }` | **Landed** |
| Finalize `note_write_replay` from `changed_storage_slots` | **Landed** (`gas_remaining_after=0`; live post-SSTORE still needed for jump) |
| `arm_rewind_to(..., prefix_writes)` | **Landed** (pevm validation abort passes `plan.prefix_writes`) |
| Absolute jump when certified prefix includes SSTORE | **Not default-on** |
| Root cause | Live EffectBoundary snaps can carry **pre-SSTORE `gas_remaining`** while PC is already past SSTORE → undercharge (~20k) → seq≠par. `gas_remaining_after` clamp did not close all schedules. Journal-blob present_values still forbidden (M1f poison). |
| Integration | `specfence_m1h_write_prefix_*` **ignored** — SSTORE+BALANCE conflict schedule hits known `inspect_run` hang (same class as ERC-20 notes). |

### B — Valued CallOutcome

| Piece | Status |
|-------|--------|
| `CachedCallOutcome.value` capture | **Landed** |
| Zero-value short-circuit | **Unchanged** (M1g) |
| Non-zero `Journal::transfer` short-circuit | **Opt-in** `SPECFENCE_VALUED_CALL_CACHE=1` (default off — inspector override + valued transfer hung schedules) |
| Combine cache + absolute jump past CALL | **Not landed** (M1g forbid retained for non-empty `call_outcomes`) |
| Integration | `specfence_m1h_valued_*` **ignored** (inspect hang on valued nested + hot BALANCE). |

## What still works (regression)

- M1f BalanceProbe absolute jump default-on  
- M1g Storage-read absolute jump (no blob)  
- M1g zero-value nested CallOutcome cache  
- M2 WaitHard park / ready-queue steal  

## ERC-20 transfer coverage?

**No.** Full ERC-20 `transfer` needs write-prefix jump that is seq≡par-safe **and** valued/nested CALL combine without hang. M1h leaves both as scaffolds + opt-in experiments, not a mainnet TPS claim.

## Remaining gaps → M3 / full transfer L1

1. Fix write-prefix gas snap equality (post-SSTORE live capture only; or charge SSTORE via controlled host without blob poison).  
2. Default-on valued CallOutcome short-circuit without inspect hang.  
3. Safe combine: replay nested touches then absolute jump at CALL-boundary.  
4. Un-ignore M1h integration tests once schedules stop hanging.  
5. M3 prior Bind; M4 adaptive meta; re-measure 7-block sweep.

## Files

- `crates/pevm/src/specfence/rem.rs` — `StorageWriteReplay`, `prefix_writes` / `write_replays` on continuation  
- `crates/pevm/src/specfence/boundary.rs` — valued `CachedCallOutcome.value`; valued cache opt-in  
- `crates/pevm/src/pevm.rs` — pass `prefix_writes` into `arm_rewind_to`  
- `crates/pevm/src/vm.rs` — finalize `note_write_replay` + residual republish from `write_replays` on rewind_resume
- `crates/pevm/tests/specfence.rs` — M1h tests (ignored pending hang fix)  

## Success gate (this tip)

- Lib: `specfence::boundary::m1c_tests` green  
- M1f / M1g integration green  
- Full `specfence` suite green (ignored M1h ok)  
- Honest status: **no** ERC-20 transfer L1 claim  
