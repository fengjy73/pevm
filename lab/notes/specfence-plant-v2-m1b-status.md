# Plant v2 M1b status — Journal fast-forward / boundary resume (L1)

**Date:** 2026-09-04 (Asia/Shanghai)  
**Branch:** `specfence` @ fengjy73/pevm  
**Scope:** SpecFence path only; OCC/PCC unchanged; M2 park/steal untouched

## Design

On RewindTo, the failed incarnation's SpecFence effect journal + bound values are
serialized into a `ResumeContinuation`. The next incarnation:

1. **Restores** SpecFence journal/checkpoints/certified set to `cp` (`replay_continuation`) — not a cold empty journal.
2. **Fast-forwards** certified-prefix reads via an FF value cache: when the MV origin
   version is unchanged, `VmDb::{storage,basic}` returns the snapped value and
   **skips** the MV lazy-walk / cold-storage heavy path (`journal_ff_hits`).
3. Still **re-enters** `Handler::run` from tx bytecode start with force-bind
   (true PC / CALL-frame resume deferred). Prefix **DB/pi** wall-clock drops;
   opcode dispatch for the prefix is **not** yet skipped.

| Op | Interpreter / DB | Metrics |
|----|------------------|---------|
| **RewindTo + FF** | Handler re-enter; SpecFence journal restored; prefix DB FF cache | `rewind_to_cp`, `resume_count`, `journal_ff_entries`, `journal_ff_hits`; **not** `evm_entries` / `tx_head_reexec` |
| **FullRestart** | Head `Vm::execute` | `full_restart` + `evm_entries` |

Checkpoint grain unchanged: CallEntry/Exit, AccountWrite/StorageWrite (post-finalize), EffectBoundary on Bind/EarlyVal. Checkpoints now store `journal_len`.

## What is truly skipped vs still re-run

| Work | On M1b RewindTo resume |
|------|-------------------------|
| SpecFence effect journal 0..=cp | **Restored** (not rebuilt from scratch via note_access) |
| Certified-prefix storage/basic reads with stable origin | **FF cache hit** — no MV lazy walk / storage I/O (`journal_ff_hits`) |
| SpecFence pi / bayes on force-bind locations | Already short-circuited (M1 force-bind) |
| Bytecode / opcode dispatch for prefix | **Still re-run** (no PC resume) |
| revm journal mutations for prefix SSTORE/etc. | **Still re-run** via interpreter |
| Suffix after k* / failed location | Re-executed normally |

## Files

- `crates/pevm/src/specfence/rem.rs` — `FfValue`, `ResumeContinuation`, `arm_rewind_to`, `replay_ff_if_armed`, unit test
- `crates/pevm/src/specfence/metrics.rs` — `journal_ff_entries`, `journal_ff_hits`, `db_heavy_ops`
- `crates/pevm/src/vm.rs` — capture bound values; FF fast path in `storage`/`basic`; replay on `set_tx`
- `crates/pevm/src/pevm.rs` — validation RewindTo arms FF continuation
- `crates/pevm/tests/specfence.rs` — `specfence_m1b_journal_ff_skips_prefix_db_work`

## Honest L1 status: **partial (stronger than M1)**

- Real prefix DB work reduction on resume (proven by `journal_ff_hits > 0` vs `db_heavy_ops` elsewhere).
- Not full L1: bytecode still starts at tx head; no live Interpreter PC / nested CALL resume.

## Remaining gaps to full PC resume

1. **True PC resume** — capture interpreter PC/stack/gas at effect boundary; inject frame mid-`run_exec_loop`.
2. **Nested CALL entry/exit hooks** — safe Inspector / handler loop without breaking sequential equivalence (next hook if CALL-boundary mid-tx needed).
3. **revm journal blob FF** — apply prefix Account/Storage writes into `JournalTr` before resume so SSTORE opcodes for prefix can be skipped once PC jumps.
4. Write checkpoints remain post-finalize ordinals (pre-existing).

## Success gate

- `cargo test -p pevm --release --config 'profile.release.lto=false' --test specfence -- --test-threads=1` passes.
- M1b test: `rewind_to_cp` + `resume_count` + `journal_ff_entries` + `journal_ff_hits` > 0; `tx_head_reexec == 0`.
