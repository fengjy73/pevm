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

## SpecFence v2 status (this branch)

See `lab/notes/specfence-redesign-v2.md` for equations and remaining gaps.

- **Unit of control**: `MemoryLocationHash` (slot/Basic/CodeHash), not whole-tx.
- **Bayesian feedback**: Beta-Bernoulli `(α,β)` per region; `observe_conflict` / `observe_speculate_ok`; inter-block decay; `decide` with `τ≈0.30`. EWMA heat subsumed for SpecFence decisions (PCC stays conservative).
- **Location Wait**: `vm` blocks on `last_writer_before(location)`; one tx can Wait on A and Speculate on B.
- **Validation feedback**: invalid reads → conflict update + Wait promotion + `wave_id` bump; successful validate → success update.
- **ESTIMATE**: full write-set ESTIMATE kept for serial equivalence (selective higher-reader ESTIMATE prototyped, unsafe under concurrent unrecorded readers).
- **Cascade fence** retained as correctness shield.
- Still **whole-tx** re-exec on abort (revm). Not yet partial reexec / full wave DAG.

Metrics: `bayes_wait_decisions`, `bayes_speculate_decisions`, `bayes_conflict_updates`,
`bayes_success_updates`, `wave_promotions`, `mean_wait_posterior`, plus fence/OCC counters.

## Lab

See `lab/README.md`. Mainnet multi-core sweeps and VLDB-style TPS / abort figures live under `lab/`.

```sh
cargo test -p pevm --release --config 'profile.release.lto=false' --test specfence -- --test-threads=1
cargo run -p pevm --release --config 'profile.release.lto=false' --example specfence_mainnet_sweep -- \
  --out lab/results/mainnet-sweep-v3.json
```
