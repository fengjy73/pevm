# SpecFence v2, stage 2e

Stage 2e starts from local stage 2d (`9c828fc` on `cursor/specfence-v2-stage2d-033e`). SpecFence stays in `crates/pevm/src/specfence/` and compiles only with `--features specfence`. Upstream `vm.rs`, `mv_memory.rs`, `scheduler.rs`, and `pevm.rs` are byte-identical to that commit. Opcode `static_gas()` is unchanged. Hooks run only on the SpecFence path.

**The commit prefix is taken before later work, from whichever queue holds it.** An owner pops the lowest index in its queue. A thief steals the lowest index anywhere else. At every task boundary, including an immediate chain handoff, any worker claims the lowest uncommitted runnable transaction. A predecessor that is not inside the interpreter, and whose owner is idle or busy on a different index, is claimed at park time instead of waited on.

**On this 4-vCPU host the trace gates that the host can see are met, and the wall-clock gates are not.** Block 15274915, C=4, timeline on: the longest commit-to-commit gap is 0.275 ms, and no gap above 0.5 ms has a runnable frontier. Parks opened after workers enter all overlap the predecessor's execution, or the predecessor starts within 80 µs. The 76-hop hot chain (`0xabd6bb3978815b97`) stays on one worker, span 1.401 ms, execution 0.749 ms, longest hop gap 154 µs. Active-set samples stay at 4. Worker drive spans cover 95.8% of the parallel phase. Clean K=10 `(to, selector)` SF is 5.311 ms at C=1 and 7.393 ms at C=4, against OCC C=4 at 4.025 ms (1.84×). SF equals SEQ (`delta_mismatch` 0).

ict21 stage 2d (CPUs 128–255, fat LTO, K=10, no warm-up) is the input this stage answers: 15274915 `(to, selector)` SF 5.15 / 7.09 / 6.55 / 8.40 / 10.84 ms at C=1/4/8/16/32 against OCC 6.45 / 2.62 / 2.24 / 3.61 / 4.22, SEQ 4.16. This host has four CPUs. C=8 is oversubscribed. C=16 and C=32 are not in the tables.

## Scheduler

Stage 2d already claimed a `Ready` frontier, and it stole an `Executing` frontier only when the owner was idle. Two paths still left the frontier behind later work.

- The owner's pop was LIFO, so a busy worker finished a burst of higher indexes and only then ran the low index sitting in its own queue (ict21: worker 0 ran tx 51, 52, 76, 70, 94, 96–99, then tx 15).
- An immediate sticky handoff skipped `claim_commit_frontier` for the whole burst. `claim_commit_frontier` also returned none for a `Sticky` frontier and for an `Executing` task whose owner was busy on a different index. That is the C=16 park of tx 102 on tx 101 for 1575 µs while 101 was not in the interpreter.

Ready indexes now live in an `IndexQueue` (a set plus the minimum as an atomic hint). `LocalDeque` remains for its own tests and is not on the transaction path.

- The owner pops its own minimum. A thief takes the minimum of every other queue and the injector.
- `claim_commit_frontier` runs before an immediate handoff. If it returns a different transaction, the handoff goes back to that worker's sticky slot and the frontier runs.
- `Ready` and `Sticky` frontiers are removed from every queue and bound to the claimant. An `Executing` frontier that is not in the interpreter is stolen when the owner is idle or `worker_task` names a different transaction. The pre-check (owner bound to this index, not idle, not yet in `handler.run`) still waits, so the task is not bounced.
- `claim_blocker` uses the same rule, so a park on a non-executing predecessor claims that predecessor wherever it is queued.
- Parking removes the task from the ready queues. Leaving those copies in place made `SCHED_SKIPS` about 1000 per block; after the removal the C=4 bucket rounds show 0–2 skips.
- A ready chain hop is still the sticky handoff, taken after the frontier and before a general pop. Early chain members are the low indexes, so they start with the prefix instead of collecting at the tail.

## Publish and the read path

`publish` thread time on ict21 was 0.42 / 4.13 / 9.12 ms at C=4/16/32. The single `seen` mutex covered every first sighting. It is now 32 shards, selected by the location hash. `Chain` is aligned to 64 bytes so neighboring chain headers do not share a line. Each transaction's membership mutex is padded to its own line.

On this host the C=4 publish bucket is 0.437 ms of thread time (1229 calls) against 0.032 ms at C=1. The ict21 blow-up is not visible at four workers. The shard split is what C=16/32 will hit.

Interpreter time is split. `db_read` is the `Database` callback (`basic`, `storage`, `code_by_hash`): multi-version lookup, the worker cache, and the storage fallback. That elapsed time is excluded from `interpreter` once, at the outermost callback. Wait guards nested inside the callback do not exclude again. After the split, `interpreter` is opcode time.

The shared structures on a miss are `BaseShare` (32 `RwLock` shards of account, code-hash, and slot maps), `CodeShare` / `new_bytecodes` (`DashMap`), and `SfMv::data` (`DashMap`, already under `wait_lock`). The per-worker `basic_cache`, `code_hash_cache`, and `slot_cache` are consulted first, including when the shared cache is on, and filled from the shared map or from storage. A code-cache hit no longer calls `DashMap::len`, which walked every shard.

Median bucket round, block 15274915, `(to, selector)`, K=10, `SPECFENCE_BUCKETS=1` (the wall clock on these rounds is not the clean scan):

| Bucket | C=1 | C=4 |
| --- | ---: | ---: |
| interpreter (opcode) | 2.735 ms / 1226 | 3.946 ms / 1242 |
| db_read | 0.326 ms / 3172 | 1.442 ms / 3367 |
| class_plain | 0.548 ms / 1046 | 0.942 ms / 1046 |
| class_other | 2.266 ms / 180 | 2.922 ms / 171 |
| wait_lock | 0.106 ms / 2636 | 0.214 ms / 1405 |
| publish | 0.032 ms / 1226 | 0.437 ms / 1231 |
| sched | 0 | 0.889 ms / 1118 |
| mv_record | 0.457 ms / 1226 | 1.804 ms / 1231 |
| pre_interp | 0.284 ms / 1226 | 1.183 ms / 1242 |

`db_read` at C=4 is thread-sum. Divided across four workers it is about the C=1 callback cost. The remaining C=4 thread growth on this host is `mv_record`, `pre_interp`, and `sched`, not the wait buckets (those stay under 0.3 ms).

## Timeline

`park` opens a park span if the caller has not opened one. A second open keeps the first start and reason. `hold_chains` therefore records the initial chain holds (about 1168 spans on this trace). Parks opened after the workers enter are the in-run waits: on the C=4 trace, none of those wait on a predecessor that is outside the interpreter.

Each worker records one `POST`/`WORK` span for its whole drive closure. Setup now includes `hold_chains`, so the parallel phase starts when the workers are dispatched. Union of exec, validate, idle, spin, post, and queue spans is 95.8% of that phase (`loop_cover` in `scripts/specfence_timeline_attrib.py`).

## Clean scan

4 vCPUs, CPUs 0–3, release, `lto=false`, `codegen-units=16`, system malloc, shared code and shared cache on, no buckets, no timeline, no warm-up, fresh engine each round, K=10. Median is the sorted sample at index K/2. The 95% interval is a bootstrap of that median, 10,000 resamples, seed 0. SEQ is timed on the C=1 rounds only. `delta_mismatch` is 0 and `ok` is true in every cell. SEQ and OCC do not use the class key; both class-key processes timed them, so those rows have n=20. SF rows have n=10.

### 15274915

| Key | C | SEQ | OCC | SF |
| --- | ---: | ---: | ---: | ---: |
| `(to, selector)` | 1 | 3.968 [3.906, 4.141] | 5.959 [5.691, 6.192] | 5.311 [4.956, 5.726] |
| `(to, selector)` | 4 |  | 4.025 [3.776, 4.241] | 7.393 [7.244, 7.613] |
| `(code_hash, selector)` | 1 |  |  | 5.334 [5.007, 5.790] |
| `(code_hash, selector)` | 4 |  |  | 7.737 [7.365, 8.638] |

SF C=4 / SF C=1 = 7.393 / 5.311 = 1.39. SF C=4 / OCC C=4 = 7.393 / 4.025 = 1.84.

### 3356896

| Key | C | SEQ | OCC | SF |
| --- | ---: | ---: | ---: | ---: |
| `(to, selector)` | 1 | 0.362 [0.332, 0.377] | 0.849 [0.825, 0.899] | 0.449 [0.432, 0.496] |
| `(to, selector)` | 4 |  | 0.569 [0.548, 0.598] | 1.251 [1.222, 1.339] |
| `(code_hash, selector)` | 1 |  |  | 0.439 [0.424, 0.482] |
| `(code_hash, selector)` | 4 |  |  | 1.247 [1.226, 1.446] |

## Gates

| Gate | Result on this host |
| --- | --- |
| No commit-frontier gap above 0.5 ms while the frontier is runnable | C=4 trace, longest commit gap 0.275 ms. Met on that trace. |
| No park on a non-running predecessor | In-run parks overlap the predecessor's execution or the predecessor starts within 80 µs. Met on that trace. |
| No tail where one chain runs alone after earlier hops were deferred | Hot chain span 1.401 ms, hop gap 154 µs, active samples stay 4. Met on that trace. |
| Worker timeline covers at least 95% of worker time | `loop_cover` 0.958. Met on that trace. |
| 15274915 `(to, selector)`: SF(4) < SF(1) | 7.393 vs 5.311. Not met. |
| SF C=4 ≤ 1.5× OCC C=4 | 7.393 vs 4.025 (1.84×). Not met. |
| SF C=4 ≤ OCC C=4 | Not met. |
| specfence tests, focus blocks, seq/par repeat, SLOAD static gas | 45 `specfence::` tests. `sf_matches_onchain_focus_blocks`, `sf_seq_par_repeat`, `sload_static_gas_matches_chain_header`. Met. |
| SF equals SEQ at every C | `delta_mismatch` is 0 in every cell above. Met on the cells this host ran. |

## Reproduce

Pinned host, fat LTO, the default binary (no `specfence-mimalloc`). No warm-up. K=10. SEQ is the C=1 rounds only. Do not set `SPECFENCE_TIMELINE` on a timed run.

```bash
scripts/soft0_percore_scan.sh --cpu-list 128-255 --c-list 1,4,8,16,32 --k 10 --profile-k 1
SPECFENCE_CLASS_KEY=code_hash scripts/soft0_percore_scan.sh \
  --cpu-list 128-255 --c-list 1,4,8,16,32 --k 10 --profile-k 1 --skip-build
```

This host, C=1 and C=4, both blocks:

```bash
SPECFENCE_INFLATION_WHICH=scan SPECFENCE_INFLATION_K=10 \
  SPECFENCE_INFLATION_BLOCKS=15274915,3356896 SPECFENCE_CLASS_KEY=to \
  taskset -c 0-3 target/release/examples/specfence_inflation_dig \
  --workers 4 --cpu-list 0,1,2,3
```

Release build used here: `cargo +stable build --release --features specfence --example specfence_inflation_dig` with `profile.release.lto="off"`, `codegen-units=16`, `strip=false`.
