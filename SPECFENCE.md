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

1. **Predict / avoid** — inter-block heat and hints seed an initial wave plan (hot regions more likely Wait).
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

- `crates/pevm/src/specfence/` — heat, region table, metrics; wave/repair/local validation to be expanded.
- `scheduler.rs` / `mv_memory.rs` / `vm.rs` / `pevm.rs` — admission, validation, abort accounting.

## SpecFence v1 status (this branch)

Incremental step toward the redesign (see `lab/notes/specfence-redesign-v1.md`):

- **OCC abort metrics** instrumented for all modes (`occ_aborts` counts successful validation aborts).
- **Validation cascade fence** (SpecFence only): on abort, rewind `validation_idx` to the first higher tx that read an aborted write — not blindly `aborted_idx+1` for the whole suffix. Metrics: `cascade_validations_scheduled`, `independent_txs_skipped_by_fence`.
- **Hint-only Wait promotion gated** in SpecFence; Wait still seeds from inter-block heat and from **observed** invalid / WW locations.
- Still **whole-tx** re-execution on abort (revm); ESTIMATE still covers the aborted write set. Not yet region-local repair / wave re-form.

## Lab

See `lab/README.md`. Mainnet multi-core sweeps and VLDB-style TPS / abort figures live under `lab/`.

```sh
cargo test -p pevm --release --config 'profile.release.lto=false' --test specfence -- --test-threads=1
cargo run -p pevm --release --config 'profile.release.lto=false' --example specfence_mainnet_sweep -- \
  --out lab/results/mainnet-sweep.json
```
