# SpecFence redesign v1 — gap vs paper claim, and the first real step

## Gap (why the first `specfence` prototype is a false negative)

The branch that landed as “SpecFence” was still:

1. **Account-hint Wait** seeded from `from`/`to` multi-writer heat / writer_count.
2. **Whole-tx Block-STM abort** — validation failure aborts the incarnation, marks **all** of its writes ESTIMATE, and rewinds `validation_idx = aborted_idx + 1` so the **entire suffix** re-validates.

That adds Wait overhead on hot accounts **without** region-local repair. On mainnet sweeps it lost to OCC at 8 cores on every block — expected for a Wait wrapper, **not** evidence against the paper claim.

True SpecFence (authoritative):

| Claim | Meaning |
|-------|---------|
| Region unit | Access tracking, versioning, deps, validation, commit, repair are **per region**, not per tx. |
| Dynamic waves | Block forms/splits/advances execution waves from exposed true dependencies. |
| Closed loop | Predict → detect → confirm deps → local block → version select / targeted re-exec / repair → re-form waves. Continuous, not a static OCC/PCC pick. |
| Inter-block learning | Heat / access relations improve the **initial** schedule only. |
| Intra-block learning | Executed prefix corrects predictions and reorganizes later waves. |
| Correctness | Exact RW version relations + commit conditions + serial ≡ preset order. Learning ≠ correctness. |
| Confinement | Conflict on regions of txs 1–5 stays inside the dependent component. Do **not** invalidate independent txs; do **not** cascade to tx6 with no dependent regions. |
| Goal | Peak TPS → true-DAG critical-path ceiling; high cores without saturation/negative scaling. **Not** linear scaling. |

## What PEVM cannot do yet (document honestly)

- **Partial re-execution of one tx** inside revm is not available: an aborted incarnation still re-runs the **whole** transaction.
- The collaborative scheduler still has a **single** `validation_idx` cursor (suffix-oriented), not a per-region ready queue.
- Region-local **version switch / state repair** without re-exec is future work.

So v1 cannot yet claim full region-local repair. It **can** stop treating SpecFence as pure account-Wait and can **fence the abort cascade**.

## SpecFence v1 (implemented)

### 1. OCC abort metrics (prerequisite)

`record_occ_abort()` runs on **every** successful `try_validation_abort`, including `ConcurrencyMode::Occ`. Region promotion stays SpecFence/PCC-only. Previously OCC always reported `occ_aborts=0`, hiding real Block-STM aborts.

### 2. Region-fence validation cascade

On SpecFence validation abort:

1. Collect write locations of the aborted incarnation.
2. Still `convert_writes_to_estimates` (readers of those writes must see ESTIMATE — correctness).
3. Find `min_higher_reader` = lowest `tx_idx > aborted` whose last read set intersects those writes.
4. Call `finish_validation_fenced(..., rewind_to: min_higher_reader)`:
   - Rewind `validation_idx` only to that reader (or **not at all** if none).
   - Independent higher txs with disjoint reads are **not** forced into the cascade queue.
5. Metrics: `cascade_validations_scheduled`, `independent_txs_skipped_by_fence`.

OCC/PCC keep classic `finish_validation` (`aborted_idx + 1`).

### 3. Heat / Wait promotion fix

- **Removed for SpecFence:** aggressive intra-block `writer_count >= 2` → Wait promotion from hints alone (`promote_if_multi_writer` no-ops in SpecFence).
- **Kept:** inter-block EWMA heat seeding Wait at block start; promote from **observed** invalid read locations; promote from **observed** WW contention in `MvMemory::record`.
- PCC still promotes from multi-writer hints (conservative baseline).

### Remaining cascade / whole-tx limits

- Aborted tx still fully re-executes.
- ESTIMATE on **all** writes of the aborted tx (not only invalid-overlapping writes) — readers of any of those writes still abort/block; we only fence the **scheduler rewind**, not ESTIMATE granularity.
- First validation of not-yet-validated independent txs still happens via `finish_execution` paths; fence only avoids needless **re**-validation of the independent suffix after an unrelated abort.
- No wave re-formation / version-switch repair yet.

## Next increments (not v1)

1. ESTIMATE only on writes that have higher readers (or only invalid-dependent locations).
2. Explicit dependent-set re-validation queue (replace single cursor for SpecFence).
3. Region-local repair / version select without full tx re-exec (needs revm/engine support).
4. Intra-block wave reordering from confirmed region DAG.
5. Richer inter-block predictors (storage-slot heat, co-access).

## Correctness stance

Fence must never skip re-validation of a tx that read an aborted write. `min_higher_reader_of` scans recorded read sets; if a higher tx has not recorded a read set yet, it is not marked Validated from a stale read of those writes — it will validate on first completion or hit ESTIMATE on execute. Serial equivalence tests remain mandatory.
