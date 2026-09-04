# SpecFence

Runtime-fact-driven **adaptive fine-grained** concurrency control for the blockchain execution layer.

This is **not** “faster OCC”, “finer MVCC alone”, or “account-hint Wait on top of Block-STM”. The first `specfence` branch prototype (account-level Wait + whole-tx abort) is an incomplete baseline and is expected to lose to OCC when it only adds waiting without region-local repair.

## Goal

Raise useful parallelism and useful work share so peak TPS approaches the concurrency ceiling set by the **true dependency DAG critical path**. Keep (or raise) TPS at high core counts without claiming linear scaling. Preserve determinism and sequential equivalence.

## Unit of control: region

A **region** (account basic / storage slot, later tighter clusters) is the unit for:

- access tracking and version management
- dependency edges
- validation
- commit readiness
- repair / targeted re-execution

A transaction may touch many regions. Conflict on a subset of regions of txs 1–5 must be confined to regions that actually depend on those versions: wait, reorder, version-switch, or locally repair **inside** that dependency component. Do **not** invalidate an entire independent transaction, and do **not** cascade to tx 6 when it shares no dependent regions.

## Closed loop (same block)

1. **Predict / avoid** — inter-block Bayesian posteriors (and hints for cold-start) seed Wait vs Speculate per region.
2. **Speculate** on uncertain / cold regions (OCC-style).
3. **Detect** conflicts from exact RW version origins at validation time.
4. **Confirm dependencies** and update the live region dependency graph.
5. **Local block / Wait** only where dependency is confirmed.
6. **Version select / targeted re-exec / repair** only for affected regions (or the minimal dependent set).
7. **Re-form waves** from the executed prefix (intra-block learning).

Learning improves schedule quality only. Correctness comes from exact RW version relations, commit conditions, and serial equivalence to the preset block order.

## Why pure OCC / PCC fail the target

- **OCC (Block-STM):** late detection, whole-tx rollback, repeated full re-execution, and ESTIMATE cascade. Even with DAG width 4, realized speedup can be far below 4×.
- **PCC:** needs accurate pre-execution RW sets / DAG that smart contracts do not provide; wrong predictions over-serialize.

SpecFence discovers the true DAG **during** execution and dynamically fuses optimistic work, necessary waits, and local pessimism from confirmed deps, remaining uncertainty, and resource pressure.

## Modes in this fork

| Mode | Role |
|------|------|
| `Occ` | Baseline Block-STM (instrumented abort metrics). |
| `Pcc` | Conservative hinted Wait baseline (over-serialization upper bound). |
| `SpecFence` | Target adaptive region/wave controller (WIP redesign). |

## Hooks

- `crates/pevm/src/specfence/` — bayes, region table, metrics, cascade fence; wave/repair/local validation to be expanded.
- `scheduler.rs` / `mv_memory.rs` / `vm.rs` / `pevm.rs` — admission, validation, abort accounting.

## Spec status

- **Spec v1 frozen:** `lab/notes/specfence-rem-spec-v1.md` (authoritative contract).
- **P1a in progress / landed plant:** region events, per-location validate API, `readers[ℓ]`,
  selective invalidate (+ aborted-incarnation detection), revokeable Bayes Wait/SpecRead/Bind
  (placeholder from prior incarnation write-set). See `lab/notes/specfence-p1a-status.md`.

### P1a plant (this branch)

- **Unit of control**: `MemoryLocationHash` (slot/Basic/CodeHash).
- **π**: `WaitHard` / `Bind` / `SpecRead` with `τ_w=0.35`, `τ_s=0.50`, `τ_revoke=0.20`
  (block-start seed still uses `DEFAULT_TAU=0.30` for inter-block carry).
- **Revoke**: sticky Wait cleared when `P_conflict < τ_revoke` (no forever Wait).
- **Selective invalidate**: ESTIMATE only locations with higher `readers[ℓ]`; otherwise keep
  Data + aborted-incarnation stamp so late readers cannot silently accept.
- **Cascade fence** retained; fence prefers selectively-invalidated locations' readers.
- Still **FullRetry** (whole-tx re-exec). PartialRetry / wave ready-queue = Phase-2 / P1b+.

Metrics: prior bayes/fence counters plus `region_validate_fail`, `tx_full_retry`, `bind_hits`,
`wait_hard_count`, `spec_read_count`, `selective_invalidate_count`, `cascade_revalidate_count`,
`soft_edge_revokes`, `selective_fallback_full`, `checkpoint_opportunities`.

## Design specs

- First principles: `lab/notes/specfence-cc-architecture-v4-first-principles.md`
- **Implementable contract (frozen):** `lab/notes/specfence-rem-spec-v1.md`

## Lab

See `lab/README.md`. Mainnet multi-core sweeps and VLDB-style TPS / abort figures live under `lab/`.

```sh
cargo test -p pevm --release --config 'profile.release.lto=false' --test specfence -- --test-threads=1
cargo run -p pevm --release --config 'profile.release.lto=false' --example specfence_mainnet_sweep -- \
  --out lab/results/mainnet-sweep-v3.json
```
