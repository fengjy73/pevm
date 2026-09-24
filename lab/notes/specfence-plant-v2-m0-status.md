# Plant v2 M0 status — metrics plant

**Date:** 2026-09-04  
**Branch:** `specfence`  
**Scope:** metrics only (OCC/PCC semantics unchanged aside from counter increments)

## What landed

| Counter | When incremented | Notes |
|---------|------------------|-------|
| `evm_entries` | Every `Vm::execute` immediately before handler `run` | Shared OCC + SpecFence path |
| `tx_head_reexec` | SpecFence PartialRetry plan **or** EarlyVal force-bind Retry | Today’s “PartialRetry” still restarts from tx head |
| `full_restart` | OCC/PCC validation abort **or** SpecFence FullRetry fallback | Last-resort head restart |
| `resume_count` / `rebind_only` / `rewind_to_cp` | (not yet) | M1 hooks; stay 0 |

## Baseline expectation

SF `evm_entries ≈ n_tx + head-reexecs` (`tx_head_reexec` + `full_restart`), **not** better than OCC.
Semantic PartialRetry does **not** reduce `evm_entries` (L1 still unmet).

## Next (M1) — DONE (partial L1)

See `lab/notes/specfence-plant-v2-m1-status.md`.
