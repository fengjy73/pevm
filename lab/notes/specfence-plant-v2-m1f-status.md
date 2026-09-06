# Plant v2 M1f status — default-safe absolute PC jump

**Date:** 2026-09-04 (Asia/Shanghai)  
**Branch:** `specfence` @ fengjy73/pevm  
**Tip parent:** M1e `f1a222f`  
**Scope:** SpecFence path only; OCC/PCC unchanged; M2 park/steal intact

## Root cause analysis (why M1e opt-in still broke seq≡par)

Arming absolute jump on conflict schedules failed for several independent reasons:

1. **MemoryGas not restored** — copying memory bytes without restoring `Gas::memory().words_num` / `expansion_cost` made post-jump MLOAD/MSTORE re-charge expansion from 0 → wrong `gas_used` / OOG / seq≠par.
2. **Journal-blob restore poisoned pevm Db** — inserting prior-incarnation storage `present_values` into revm journal made later SLOADs hit stale journal instead of pevm MvMemory/force-bind → wrong stack/control-flow → hang or silent wrong Success.
3. **Storage-prefixed / large-contract jumps** — even with MemoryGas fixed and **no** blob restore, absolute jump on ERC-20-scale frames livelocked or diverged under pevm MV (mid-tx PC/stack not ≡ sequential certified prefix).
4. **RewindTo often landed on CallEntry `k=0`** — SpecRead paths recorded no EffectBoundary, so `jump_snap` was missing and production never jumped (credit-only only).
5. **Full `EvmState` clone at every EffectBoundary** under always-on capture worsened conflict `inspect_run` hangs.

## What M1f changed

| Piece | Behavior |
|-------|----------|
| `absolute_jump_env_enabled` | **Default on** (env unset). `SPECFENCE_ABSOLUTE_JUMP=0` force-disables. |
| `BoundarySnapshot` | Restores `gas_refunded`, `memory_words`, `memory_expansion_cost` with PC/stack/memory. |
| `jump_is_safe` | Live snap, `call_depth≤1`, PC in range, read-only prefix, **no Storage FF**, ≥1 Basic FF, `opcode_steps∈(0,128]`, **`bytecode_len≤256`**, no selfdestruct blob. |
| Arm path | Restores PC/stack/memory/MemoryGas only — **never** journal blob. |
| SpecRead | Records EffectBoundary so RewindTo can leave CallEntry with live `jump_snap`. |
| Capture | Snap-only by default; full journal blob only if `SPECFENCE_ABSOLUTE_JUMP=blob` (research). |
| Circuit breaker | Validation abort still `disable_jump_after_failed_resume` (pevm.rs). |
| Side channel | Prefer exact `cp.k` live snap; fallback nearest `k≤cp.k`. Live-tip floor in `last_checkpoint_before` when cps missing. |

## Default jump set (honest)

**Yes, L1 absolute jump is real on the default path for depth≤1**, but only for **tiny Basic-only** contracts (≤256-byte bytecode, no Storage FF in certified prefix). Integration proof: BalanceProbe (`CALLER/BALANCE` + hot-account writers) → `absolute_jump_applied > 0`, inspector skip, seq≡par via `run_mode`.

ERC-20 / storage-prefixed RewindTo remains **credit-only** (`absolute_jump_fallback`) — safe seq≡par, not PC-skip L1.

## Remaining gaps

1. Nested CALL (`call_depth > 1`) — still fallback (CallOutcome cache TODO).
2. ERC-20 / Storage-prefix absolute jump — not default-safe under pevm MV.
3. Write-prefix jump (needs correct blob FF without Db poisoning).
4. Intermittent `inspect_run` hang on some ERC-20 schedules (also seen on M1e tip) — suite green when tests run isolated with retry; monolithic `--test-threads=1` can still stall mid-run.

## Files

- `crates/pevm/src/specfence/boundary.rs` — MemoryGas restore, default-on env, `jump_is_safe`, snap capture
- `crates/pevm/src/specfence/rem.rs` — live-tip rewind floor; jump side channel
- `crates/pevm/src/vm.rs` — SpecRead EffectBoundary; commit jumped Success when gated
- `crates/pevm/tests/specfence.rs` — `specfence_m1f_default_absolute_jump_seq_eq_par` (BalanceProbe)

## Success gate

- Lib: `specfence::boundary::m1c_tests` (incl. `m1f_arm_applies_absolute_jump_metric`)
- Integration: `specfence_m1f_default_absolute_jump_seq_eq_par` with env unset → `absolute_jump_applied > 0`
- Full `specfence` suite: green per-test (retry hung inspect_run schedules once)

## Honest L1 status after M1f: **partial but default-on for safe set**

- Default RewindTo: absolute jump when `jump_is_safe` (tiny Basic-only, depth≤1).
- Unsafe / Storage / large bytecode / nested CALL: fallback, no livelock from jump arming.
- Committed-path L1 for ERC-20 still not claimed.
