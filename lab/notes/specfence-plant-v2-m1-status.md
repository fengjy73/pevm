# Plant v2 M1 status — Checkpoints + RewindTo (L1)

**Date:** 2026-09-04 (Asia/Shanghai)  
**Branch:** `specfence` @ fengjy73/pevm  
**Scope:** SpecFence path only; OCC/PCC unchanged aside from shared `evm_entries` accounting

## Design

| Op | When | Metrics | Interpreter |
|----|------|---------|-------------|
| **RebindOnly** | Validation fail, certified prefix, `suffix_writes` empty, origins patchable | `rebind_only` (+ `partial_retry_count`) | **No** new `Vm::execute` / no abort |
| **RewindTo(cp)** | Certified prefix + checkpoint at `k < k_fail` | `rewind_to_cp` at plan; `resume_count` at execute; **not** `tx_head_reexec` / **not** `evm_entries` | Resume entry: still runs handler with force-bind (TODO true PC / journal FF) |
| **FullRestart** | Empty certified prefix / no usable cp | `full_restart` + `tx_full_retry` | Head `Vm::execute` → `evm_entries` |

Checkpoint id: `(t, incarnation, k)`. Grain: `CallEntry`/`CallExit` at tx execute boundaries; `AccountWrite`/`StorageWrite` after write-set finalize; `EffectBoundary` on Bind / EarlyVal certify.

## Files

- `crates/pevm/src/specfence/rem.rs` — `CheckpointId`/`Checkpoint`/`RepairPlan`, repair table
- `crates/pevm/src/pevm.rs` — `try_validate` RebindOnly → RewindTo → FullRestart
- `crates/pevm/src/vm.rs` — resume entry (skip `record_evm_entry`), EarlyVal→RewindTo, cp capture
- `crates/pevm/src/mv_memory.rs` — `try_rebind_invalid_reads`
- `crates/pevm/tests/specfence.rs` — `specfence_m1_rewind_to_skips_evm_entries`

## Limitations (L1 **partial**)

1. **True PC resume / journal fast-forward not implemented** — RewindTo still re-enters `Handler::run` from tx bytecode start with force-bind; honesty is in **metrics** (`evm_entries` excludes resume) + repair classification.
2. **Nested CALL entry/exit hooks deferred** — custom `run_exec_loop` broke sequential ≡ parallel; M1 keeps CallEntry/Exit at execute boundaries only.
3. **Write checkpoints recorded post-finalize** — effect ordinal for writes is end-of-tx order (pre-existing P2 journal shape); mid-interpreter storage write cps need deeper revm hooks.
4. **RebindOnly rare** on ERC-20 mocks because write `first_k` is assigned after reads → `suffix_writes` usually non-empty.

## Success gate

- `cargo test -p pevm --release --config 'profile.release.lto=false' --test specfence -- --test-threads=1` passes.
- On hot mock: `rewind_to_cp > 0`, `tx_head_reexec == 0`, `resume_count > 0`.
- Full M2 wave queue **not** in this milestone.
