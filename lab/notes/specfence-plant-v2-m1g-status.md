# Plant v2 M1g status — Storage-safe jump + nested CallOutcome cache

**Date:** 2026-09-05 (Asia/Shanghai)  
**Branch:** `specfence` @ fengjy73/pevm  
**Tip parent:** M1f `8d506e2`  
**Scope:** SpecFence path only; OCC/PCC unchanged; M2 park/steal intact; default absolute jump remains default-on for the newly expanded safe set

## Dual goals

### A — Storage / ERC-20-like absolute jump (no Db poison)

| Piece | Behavior |
|-------|----------|
| `jump_is_safe` | Allows certified-prefix **Storage reads** (and/or Basic). Still forbids **Write** effects (mid-SSTORE). |
| Journal blob | **Never** restored on arm — storage `present_values` poison pevm Db / shadow MvMemory (M1f root cause). |
| Correctness | SpecFence FF cache + force-bind / MV origins only (same as M1f Basic path). |
| `bytecode_len` | ≤256 always (M1f). ≤4096 when Storage FF **or** `at_call_boundary` (careful relax). |
| Restore | PC / stack / memory / MemoryGas / refund as M1f. |
| Integration | `specfence_m1g_storage_absolute_jump_seq_eq_par` — dual-path StorageProbe (SSTORE writers / SLOAD readers), `absolute_jump_applied > 0`, seq≡par, resume steps < cold. |

### B — Nested CALL via CallOutcome cache

| Piece | Behavior |
|-------|----------|
| Capture | Inspector `call`/`call_end` record nested (`depth>1`) successful `CallOutcome`s into `PartialRetryState` / `ResumeContinuation.call_outcomes`. |
| Short-circuit | On RewindTo resume, `Inspector::call` returns cached outcome for matching nested calls **after** replicating `make_call_frame` journal side effects (`load_account`, `transfer_loaded`+checkpoint, zero-value only). |
| Absolute jump | **Forbidden** when `call_outcomes` non-empty — PC-skip past CALL also omits EIP-158 touch → seq≠par (same class of bug as bare CallOutcome override). |
| CALL-boundary snaps | `call_end` → parent `step_end` marks `at_call_boundary` and attaches live snap (for future boundary jumps / large bytecode preference). |
| Integration | `specfence_m1g_nested_call_resume_jump` — outer CALL inner then BALANCE(hot); asserts jump **or** `call_outcome_cache_hits > 0` with seq≡par. |

## Root-cause avoidance vs M1f

1. **MemoryGas** — still restored (M1f).  
2. **Journal-blob / storage present_values** — still never armed into revm journal (M1f); Storage jump does **not** reintroduce poison.  
3. **Storage-prefix livelock** — avoided by read-only gate + no blob; StorageProbe proves seq≡par with real absolute jump.  
4. **Nested CALL** — new: bare jump/short-circuit without journal touch broke seq≡par; fixed by (a) forbid jump over nested outcomes, (b) CallOutcome cache with journal side effects.  
5. **Anti-livelock** — `jump_disabled` after failed jumped resume unchanged.

## What jumps / what still fallbacks

| Case | Default path |
|------|----------------|
| Tiny Basic-only (BalanceProbe) | Absolute jump (M1f) |
| Tiny Storage-read prefix (StorageProbe) | Absolute jump (**M1g**) |
| Large bytecode ≤4096 with Storage FF or CALL-boundary | Eligible if other gates pass |
| Write-prefix / mid-SSTORE | Fallback |
| Nested CALL in certified prefix | CallOutcome short-circuit (not absolute jump) |
| depth>2 without cache | Fallback |
| Selfdestruct blob | Fallback |

## Honest L1 vs mainnet

- L1 absolute jump now covers **Storage-read** prefixes on small contracts — closer to ERC-20 *balanceOf*-style prefixes, **not** full `transfer` (writes still fallback).  
- Nested CALL gas on real contracts can use CallOutcome cache when certified nested frames are read-only / zero-value.  
- Full ERC-20 `transfer` / DEX routes with mid-tx SSTORE + depth>1 still mostly **credit-only / cache** — not claimed as mainnet TPS flip.  
- M3 prior Bind + M4 adaptive meta still required for SF/OCC ≫ 1 on 7-block sweeps.

## Remaining gaps

1. Write-prefix jump without Db-poisoning blob FF.  
2. Absolute jump that replays nested CALL touches then PC-skips (combine A+B).  
3. Non-zero value nested CallOutcome short-circuit.  
4. M3 prior Bind; M4 adaptive engagement; re-measure 7-block sweep.  
5. Intermittent `inspect_run` hang on some ERC-20 / fence schedules (retry once when isolated).

## Files

- `crates/pevm/src/specfence/boundary.rs` — `jump_is_safe` M1g, CallOutcome cache, CALL-boundary attach  
- `crates/pevm/src/specfence/rem.rs` — `call_outcomes` on state/continuation; `note_call_outcomes`  
- `crates/pevm/src/specfence/metrics.rs` — `call_outcome_cache_hits`  
- `crates/pevm/src/vm.rs` — arm CallOutcome cache on fallback resume  
- `crates/pevm/tests/specfence.rs` — M1g Storage + nested tests  

## Success gate

- Lib: `specfence::boundary::m1c_tests` (incl. Storage + depth2 unit tests)  
- Integration: `specfence_m1g_storage_absolute_jump_seq_eq_par`, `specfence_m1g_nested_call_resume_jump`  
- Full `specfence` suite: green per-test (retry hung inspect_run once)
