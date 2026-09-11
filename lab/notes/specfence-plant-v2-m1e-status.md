# Plant v2 M1e status — journal-blob FF + safe absolute PC jump

**Date:** 2026-09-04 (Asia/Shanghai)  
**Branch:** `specfence` @ fengjy73/pevm  
**Tip parent:** M1d `ab5afe4`  
**Scope:** SpecFence path only; OCC/PCC unchanged; M1b FF + M2 park/steal intact

## Design

### Safety gate (`jump_is_safe`)
Absolute PC jump is armed only when **all** hold:

1. Live Inspector snap present (`jump_snap.is_live_capture()`)
2. Top-level frame only (`call_depth ≤ 1`) — nested CALL → fallback
3. PC in bytecode range (`pc < bytecode_len`)
4. Non-empty certified prefix / work to skip
5. Non-empty **journal blob** (touched `EvmState` + logs) so post-jump world-state + receipts can match sequential prefix
6. Same-contract check in `initialize_interp` (code hash)
7. Anti-livelock: if a jumped resume still fails validation, `jump_disabled` for that tx

### Side channel (critical)
Live snaps + journal blobs are **not** written into EffectBoundary repair checkpoints (that livelocked M1d schedules).  
They travel on `ResumeContinuation.{jump_snap, journal_blob}` only. Lite `boundary` stays for skip-credit accounting (M1c/M1d).

### Journal blob capture / restore
- **Capture:** on EffectBoundary, after `step_end`, clone touched accounts from `JournalExt::evm_state` + `JournalTr::logs()` into `JournalBlob`.
- **Restore:** in `SpecFenceInspector::initialize_interp` before `absolute_jump`, merge blob state into journal and re-`log()` prefix events (ERC-20 Transfer must not be dropped).
- **Default production:** capture + arm are behind `SPECFENCE_ABSOLUTE_JUMP=1`. Without the flag, RewindTo uses M1d credit-only resume and records `absolute_jump_fallback`.

| Op | Interpreter / DB | Metrics |
|----|------------------|---------|
| **RewindTo (default)** | inspect_run; journal FF; skip **credit**; no PC jump | `absolute_jump_fallback`, `pc_resume_count`, `prefix_opcodes_skipped`, `inspector_steps_resume` |
| **RewindTo (opt-in jump)** | restore journal blob; `absolute_jump` + stack/gas | `absolute_jump_applied`, `live_pc_resume_count`, `journal_blob_ff_accounts` |
| **Unsafe / nested CALL** | non-jump fallback | `absolute_jump_fallback` |
| **FullRestart** | Head execute | `full_restart` + `evm_entries` |

## When jump applies vs falls back

| Condition | Result |
|-----------|--------|
| No `SPECFENCE_ABSOLUTE_JUMP` | **Fallback** (default production) |
| Missing live snap / lite only | Fallback |
| `call_depth > 1` | Fallback (nested CALL gap) |
| Empty journal blob | Fallback |
| Code hash / PC range fail at init | Fallback (+ credit) |
| Prior jumped resume failed validation | Fallback (`jump_disabled`) |
| Flag set + gate passes | **Absolute jump applied** |

## Why production jump is opt-in (honest)

Arming absolute jump on conflict schedules (even with blob+logs+circuit breaker) still **broke sequential ≡ parallel** on ERC-20 clusters in this milestone. Root cause class: mid-tx PC/stack/journal restore is not yet fully equivalent to re-executing the certified prefix under pevm’s MvMemory/force-bind.  

M1e therefore ships the **full gate + blob + side-channel + metrics**, keeps default path **seq≡par-safe** (M1d resume), and leaves opt-in for research (`SPECFENCE_ABSOLUTE_JUMP=1`).

## Remaining gaps

1. Make production jump default without seq≠par (stronger blob / gas / warm-account equivalence).
2. Nested CALL short-circuit via cached `CallOutcome`.
3. Rise SpecFenceInspector + jump (still run-only).
4. Pre-existing intermittent inspect_run hang on some conflict schedules (observed on M1d tip too).

## Files

- `crates/pevm/src/specfence/boundary.rs` — `JournalBlob`, `jump_is_safe`, `try_arm_safe_absolute_jump`, restore in `initialize_interp`
- `crates/pevm/src/specfence/rem.rs` — `jump_snap` / `journal_blob` side channel; `jump_disabled` circuit breaker
- `crates/pevm/src/specfence/metrics.rs` — `absolute_jump_applied`, `absolute_jump_fallback`, `journal_blob_ff_accounts`
- `crates/pevm/src/vm.rs` — RewindTo arm/fallback
- `crates/pevm/src/chain.rs` — `JournalExt` bound for blob access
- `crates/pevm/tests/specfence.rs` — `specfence_m1e_absolute_jump_or_safe_fallback`

## Honest L1 status after M1e: **partial (gate+blob landed; production jump opt-in)**

- Default RewindTo: still not `evm_entries` / `tx_head_reexec`; skip credit + M1b FF; `absolute_jump_fallback` recorded.
- Absolute jump + journal-blob restore: implemented and unit-gated; production default off until seq≡par proven with jump on.
- Inspector step proof on default path: `inspector_steps_resume < resume + prefix_opcodes_skipped` (M1d/M1e test).

## Success gate

- `cargo test -p pevm --release --config 'profile.release.lto=false' --test specfence -- --test-threads=1` — green (retry hung M1d-era schedules if needed).
- Lib: `jump_is_safe_*`, `apply_to_interp_jumps_pc_and_restores_stack`.
- M1e test: rewind + resume + `absolute_jump_fallback > 0` + skip credit; `tx_head_reexec == 0`.
