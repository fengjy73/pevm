# SpecFence redesign v2 — location-level Bayesian Wait/Speculate

## Why region must be finer than a transaction

A transaction touches many `MemoryLocation`s (`Basic`, `CodeHash`, `Storage`). Conflict on
slots of txs 1–5 must repair **those** regions only. Other slots of the same txs, and
independent txs, must keep progressing.

v1 (cascade fence + EWMA account heat) still decided mostly at account/tx granularity and
kept whole-tx re-exec + full write-set ESTIMATE. That is necessary but insufficient: it
adds Wait overhead without true local repair, so SpecFence lost to OCC on mainnet @8.

## Bayesian region model

Per-region key: `MemoryLocationHash` (plus `Address` cold-start when the slot is unknown
pre-exec).

Beta-Bernoulli posterior on conflict probability:

- Prior: `α₀=1`, `β₀=9` → `P₀ = α/(α+β) ≈ 0.1` (low conflict).
- `observe_conflict(region)`: `α ← α+1`
- `observe_speculate_ok(region)`: `β ← β+1` (at most once per location per block)
- Mean: `P(conflict) = α/(α+β)`
- Inter-block decay: `(α−1),(β−1) ← λ·(·)` with `λ=0.95`, then re-center on prior floor.

### Decision rule

With threshold `τ` (default `0.30`):

```
if P(conflict | region) ≥ τ → Wait else Speculate
```

Equivalent cost form (same threshold when costs are constant):

```
if P(conflict)·reexec_cost > wait_cost → Wait else Speculate
```

Cascade fence remains a **correctness shield** only; it does not choose Wait vs Speculate.

## How region < tx is enforced

| Path | Behavior |
|------|----------|
| `vm::maybe_wait` | Queries bayes/`RegionTable` for **that location hash**. Blocks on `MvMemory::last_writer_before(location)`, not whole-account hint, once the location is known. Account hint prev is cold-start only. |
| One tx | May Wait on location A and Speculate on location B in the same execution. |
| `try_validate` abort | `observe_conflict` on each **invalid read location**; promote that location to Wait for the rest of the block; bump `wave_id`. |
| Successful validate | `observe_speculate_ok` on speculated read locations (once/block). |
| ESTIMATE | Full write-set ESTIMATE retained for serial equivalence. Selective higher-reader ESTIMATE (`convert_writes_to_estimates_selective`) broke equivalence when concurrent readers had not yet recorded a read set; kept for future work. |
| Beneficiary | Never Wait/learn on beneficiary gas location. |

## What is still missing (honest)

1. **Partial re-execution inside revm** — aborted incarnation still re-runs the whole tx.
2. Full **wave/DAG scheduler** — `wave_id` + promotions are signals, not a ready-queue.
3. Region-local **version switch / repair** without re-exec.
4. Cost-sensitive τ per block (currently a fixed threshold).

## Tests

See `crates/pevm/tests/specfence.rs`: location isolation, inter-block Bayes carry,
sequential equivalence, OCC abort metrics, fence metrics retained.
