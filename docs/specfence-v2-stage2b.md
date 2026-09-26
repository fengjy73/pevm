# SpecFence v2, stage 2b

Draft PR: https://github.com/fengjy73/pevm/pull/72

Stage 2b starts from Stage 2 (`cursor/specfence-v2-stage2-afe4`, `3164d51`, PR #71). SpecFence stays in `crates/pevm/src/specfence/` and compiles only with `--features specfence`. Upstream `vm.rs`, `mv_memory.rs`, `scheduler.rs`, and `pevm.rs` are byte-identical to that commit. Opcode `static_gas()` is unchanged. Hooks run only on the SpecFence path.

ict21 Stage 2 (CPUs 128–255, fat LTO, K=10, no warm-up) is the problem. On block 15274915, `(to, selector)`, SF was 4.88 ms at C=1 and 9.07 / 7.77 / 8.96 / 11.8 ms at C=4/8/16/32, against OCC 3.03 ms at C=4. Every C>1 was slower than SF(1). Thread time at C=4 put the interpreter at 8.7–10.2 ms against about 2.5 ms at C=1. The learned active set ended at 1. The longest gap over all chains was 9.8 ms at C=4, while the hot chain `0xabd6bb3978815b97` stayed 76/76 on one worker. SF matched SEQ. `delta_mismatch` was 0.

## What changed

- **Active set follows ready width.** The hop-gap shrink from Stage 2 is gone. The width is an exponential moving average (3/4 previous, 1/4 new sample). One sample above the set grows it, up to the worker count. Shrink waits for two decisions where the average sits at least two below the set and the live queue is shorter than the set. A chain hop does not enter the decision. The trace keeps the sample series (capped at 64, then every other sample).
- **Sticky handoff spills the deque.** When a worker takes the next read-modify-write on its private slot, it moves the rest of its Chase-Lev deque onto a shared injector. Only the owner pops the bottom, so the move is safe. Other workers pop the injector after a local pop and a steal miss. Status stays Ready until pop.
- **Predecessor boost.** A read that parks on a lower transaction pushes that predecessor onto this worker's deque before parking. A duplicate already queued elsewhere is skipped, because only Ready starts. Anchor, admission, and class-head parks do the same.
- **Cold base-state reads.** A location absent from both the chain bloom and the multi-version bloom is read from base state into a worker-local cache (account, code hash, bytecode, storage slot). The storage origin is recorded only when a lower transaction is still uncommitted. If the bloom flips during that read, the origin is dropped and the chain path runs. Repeated `Basic(caller)` accounts are unioned with repeated `Basic(to)` into one armed chain, so a later transaction from the same sender waits instead of taking a stale account. Writes with no chain slot and no reader stay in multi-version memory until commit.
- **Attribution.** `SPECFENCE_FORCE_PARALLEL=1` builds the parallel path at `workers == 1` and runs it on the calling thread. `SPECFENCE_BUCKETS=1` splits interpreter time into plain transfers, the largest contract class, other transactions, and re-executions. `SPECFENCE_ABORT_DIAG=1` prints the value kind of each full replay.
- **OCC spawn cost.** `SPECFENCE_INFLATION_WHICH=spawn` times an empty `thread::scope` of C threads. It does not enter upstream `execute_revm_parallel`.

Three cuts were measured and taken back out. A commit window that hid transactions past the prefix left distance-3 races in place and stretched the hot-chain gap. Ordering every contract behind the previous contract's finality drove full replays to 0 and the active set to 1, with wall time about 9.4 ms. Retrying inside the same incarnation when a storage origin was already stale livelocked `sf_seq_par_repeat` at C=8 (`committed=31` of 176).

## Interpreter inflation

This VM, 4 vCPUs, CPUs 0–3, `lto=false`, codegen-units 1. `perf` is not installed. Block 15274915, `(to, selector)`, `SPECFENCE_BUCKETS=1`, K=3, last round. Thread time, not wall. The fast path does not install classes, so every contract is "other" and the hot class is empty. Re-execution time is also inside the class buckets.

| Bucket | Fast C=1 | Force parallel C=1 | C=4 |
| --- | ---: | ---: | ---: |
| interpreter | 2.435 ms / 1226 | 3.696 ms / 1241 | 5.275 ms / 1243 |
| plain transfer | 0.457 ms / 1046 | 0.462 ms / 1046 | 0.746 ms / 1046 |
| hot contract | 0 | 0.107 ms / 14 | 0.159 ms / 14 |
| other | 2.056 ms / 180 | 2.925 ms / 174 | 4.262 ms / 173 |
| re-execution | 0 | 0.501 ms / 8 | 1.004 ms / 7 |
| read origin | 0 | 0.247 ms / 2603 | 0.415 ms / 2652 |
| read base | 0 | 0.101 ms / 957 | 0.178 ms / 1074 |
| read mv | 0 | 0.014 ms / 322 | 0.037 ms / 338 |
| coordinate | 0 | 0.025 ms / 331 | 0.140 ms / 346 |
| cold reads (count) | 0 | 2554 | 2551 |
| pre_interp | 0.175 ms | 0.472 ms | 1.085 ms |
| mv_record | 0.382 ms | 0.566 ms | 1.809 ms |
| publish | 0.032 ms | 0.166 ms | 0.419 ms |

Per transaction, plain transfers are 0.44 µs on the fast path, 0.44 µs on the forced parallel path, and 0.71 µs at C=4 (1.63×). Other transactions are 11.4 µs, 16.8 µs, and 24.6 µs (2.16×). The hot class is 14 transactions, 11.4 µs each at C=4. The fast path cannot name that class.

The C=4 interpreter is 5.275 / 2.435 = 2.17× the fast path. The forced one-worker parallel path is 3.696 / 2.435 = 1.52×, of which 0.50 ms is re-execution. Cross-core time is the rest of the C=4 gap: the same block at one worker does not pay it. Cold reads are 2551 calls and are not an `Instant` bucket; they skip the multi-version map. The remaining full replays are not those cold misses.

Stage 2's single C=4 bucket run on this VM was interpreter 6.63 ms, `mv_record` 2.42 ms, publish 2.22 ms, `pre_interp` 1.34 ms, coordinate 0.50 ms. Publish is the bucket that moved (2.22 ms to 0.42 ms): a write with no chain and no reader stays in the multi-version map.

### What the full replays are

`SPECFENCE_ABORT_DIAG=1`, one C=4 round, block 15274915. Nine aborts, all with `delta_mismatch` 0. The repeated location is `0xbef034365ca24581`, a basic account. The live value is `MemoryValue::Basic`. For some writers that hash is `Basic(to)`; for others it is not, so the account is touched from inside the contract, not as the transaction's caller or callee. The first reader records a storage origin (`read=NONE`, the cold path) or is told there is no lower chain writer. A closer writer then publishes. Later readers accept that published value (`ready_published`) and abort again when the writer re-executes. One abort was a storage slot and one was a lazy sender. Arming the location stops further aborts on it (`full_replay_after_arm` stays 0 on the timed scans). The first wave is the cost.

A preseed of `tx.to` and `tx.caller` cannot name an account the contract discovers while it runs. Waiting for every lower contract to be final removes the wave and also removes the other workers.

## Cloud VM

Host: 4 vCPUs, CPUs 0–3. C=8 is eight workers on four CPUs. C=16 and C=32 were not scanned. Release build, `lto=false`, codegen-units 1. K=10, no warm-up, fresh engine per round, pool reused, learned state reset each block. Median and bootstrap 95% CI, 10,000 resamples, seed 0. SEQ is timed on every C=1 round; the table uses that median. `delta_mismatch` is 0 and `ok` is true in every cell below.

### `(to, selector)`

Block 15274915. SEQ 3.414 ms [3.394, 3.440].

| C | OCC ms | OCC 95% CI | SF ms | SF 95% CI | reexec | full replay |
| ---: | ---: | --- | ---: | --- | --- | --- |
| 1 | 5.302 | [4.989, 5.828] | 4.498 | [4.113, 4.741] | 0 | 0 |
| 4 | 3.073 | [2.989, 3.312] | 6.442 | [6.199, 6.825] | 10–16 | 7–13 |
| 8 | 3.881 | [3.780, 4.182] | 6.408 | [6.300, 7.196] | 5–13 | 4–9 |

Block 3356896. SEQ 0.304 ms [0.264, 0.333].

| C | OCC ms | OCC 95% CI | SF ms | SF 95% CI | reexec | full replay |
| ---: | ---: | --- | ---: | --- | --- | --- |
| 1 | 0.736 | [0.630, 0.780] | 0.351 | [0.342, 0.373] | 0 | 0 |
| 4 | 0.478 | [0.441, 0.522] | 1.065 | [1.021, 1.262] | 0 | 0 |
| 8 | 0.633 | [0.592, 0.931] | 1.154 | [1.089, 1.229] | 0–2 | 0–2 |

### `(code_hash, selector)`

Block 15274915. SEQ 3.441 ms [3.414, 3.619].

| C | OCC ms | OCC 95% CI | SF ms | SF 95% CI | reexec | full replay |
| ---: | ---: | --- | ---: | --- | --- | --- |
| 1 | 5.369 | [5.029, 5.964] | 4.578 | [4.181, 4.683] | 0 | 0 |
| 4 | 3.165 | [3.056, 3.483] | 6.259 | [5.936, 7.031] | 8–19 | 5–13 |
| 8 | 3.654 | [3.534, 3.877] | 7.175 | [6.695, 7.832] | 5–14 | 4–11 |

Block 3356896. SEQ 0.297 ms [0.266, 0.319].

| C | OCC ms | OCC 95% CI | SF ms | SF 95% CI | reexec | full replay |
| ---: | ---: | --- | ---: | --- | --- | --- |
| 1 | 0.786 | [0.737, 0.853] | 0.347 | [0.336, 0.380] | 0 | 0 |
| 4 | 0.489 | [0.461, 0.527] | 1.052 | [1.010, 1.216] | 0 | 0 |
| 8 | 0.584 | [0.543, 0.621] | 1.213 | [1.152, 1.341] | 0–17 | 0–9 |

## Controller

On 15274915, `(to, selector)`, C=4, the median round's active samples are 4 for the body of the block and 1 for the tail (57 samples, peak 4, end 1). Parked and woken are 21 and 21. The tail is the ready queue draining, not a hop-gap shrink: the hop gap is no longer an input, and the set stays at 4 while other work is queued. C=8 on this host reaches peak 8 and also ends at 1. Stage 2 on the same host ended at 1 with peak 4 because the hop-gap rule fired during the block.

## Non-hot gaps

The hot chain is not the long gap. On the C=4 median round it is 76/76 on one worker, hot gap 5.9 µs, hot exec 0.40–0.76 ms across rounds. The longest gap in that round is 273 µs, transaction 94, previous worker 3, reason `DELAY_BLOCK` (1), location `0x23e73edfbbb8a5ef`. Other rounds land on `0x693c76ebc9d30041` or the conflict account `0xbef034365ca24581`, with reasons `DELAY_BLOCK`, `DELAY_NONE`, or `DELAY_ADMIT`.

Before the boost, a steal took a high index, the transaction parked on a predecessor that was still sitting in another worker's stride, and the gap was several milliseconds (the ict21 C=4 figure was 9.8 ms over all chains; an earlier build on this VM was 3–4 ms). The boost runs that predecessor on the waiting worker. The duplicate copy, if the owner also had it, does not start twice.

## OCC spawn and join

Empty `thread::scope` plus `scope.spawn`, K=10, this VM. Median.

| C | spawn+join ms |
| ---: | ---: |
| 4 | 0.053 |
| 8 | 0.092 |
| 16 | 0.221 |
| 32 | 0.405 |

C=16 and C=32 are more threads than CPUs. OCC's C=4 median on 15274915 is 3.073 ms, so the spawn inside each timed OCC call is about 0.05 ms of that. Subtracting it does not put SF C=4 (6.442 ms) under OCC. The SpecFence pool is created before the timed loop, so SF does not pay this per round.

## Gates on this VM

ict21 at C=1, 4, 8, 16, 32 is the authoritative remeasure. C=8 here is oversubscribed. C=16 and C=32 were not scanned. Ratios use the `(to, selector)` medians unless noted.

| Gate | Result |
| --- | --- |
| 15274915 SF(4) < SF(1) | Fail. 6.442 vs 4.498. `(code_hash, selector)` 6.259 vs 4.578. |
| 15274915 SF(8) ≤ SF(4) × 1.1 | Pass for `(to, selector)`: 6.408 ≤ 7.086. Fail for `(code_hash, selector)`: 7.175 vs 6.885. |
| C=4 interpreter ≤ 1.5× fast-path interpreter | Fail. 5.275 / 2.435 = 2.17×. Forced parallel at one worker is 1.52×, and 0.50 ms of that is re-execution. |
| SF C=4 ≤ OCC C=4 on 15274915 | Fail. 6.442 vs 3.073. Spawn cost is 0.053 ms. |
| SpecFence tests, focus blocks, seq/par repeat, static gas | Pass. `specfence::` 29 tests. `sf_matches_onchain_focus_blocks`, `sf_seq_par_repeat` (4.17 s), `sload_static_gas_matches_chain_header` (0.59 s). |
| SF equals SEQ on both focus blocks at every measured C | Pass on this scan: every timed row has `ok=true` and `delta_mismatch=0`. The integration tests compare receipts at C=1, 4, and 8. |

## Reproduce

```bash
cargo +stable build -p pevm --release --features specfence \
  --config 'profile.release.lto=false' --example specfence_inflation_dig
```

Wall scan. One class key and one worker count. Repeat for `code_hash` and for `--workers` 1, 4, 8. SEQ is inside the workers=1 scan. No warm-up.

```bash
SPECFENCE_CLASS_KEY=to SPECFENCE_INFLATION_WHICH=scan SPECFENCE_INFLATION_K=10 \
  SPECFENCE_INFLATION_OUT=/tmp/sf-stage2b/to-c4.jsonl \
  taskset -c 0-3 target/release/examples/specfence_inflation_dig \
  --workers 4 --cpu-list 0-3
```

Interpreter split. `SPECFENCE_FORCE_PARALLEL=1` with `--workers 1` is the parallel path without a second core.

```bash
SPECFENCE_BUCKETS=1 SPECFENCE_CLASS_KEY=to SPECFENCE_INFLATION_WHICH=scan \
  SPECFENCE_INFLATION_BLOCKS=15274915 SPECFENCE_INFLATION_ENGINES=sf \
  SPECFENCE_INFLATION_K=3 \
  taskset -c 0-3 target/release/examples/specfence_inflation_dig \
  --workers 4 --cpu-list 0-3
```

Abort kinds, one round:

```bash
SPECFENCE_ABORT_DIAG=1 SPECFENCE_CLASS_KEY=to SPECFENCE_INFLATION_WHICH=scan \
  SPECFENCE_INFLATION_BLOCKS=15274915 SPECFENCE_INFLATION_ENGINES=sf \
  SPECFENCE_INFLATION_K=1 \
  taskset -c 0-3 target/release/examples/specfence_inflation_dig \
  --workers 4 --cpu-list 0-3
```

OCC spawn, without entering the upstream scheduler:

```bash
SPECFENCE_INFLATION_WHICH=spawn SPECFENCE_INFLATION_K=10 \
  SPECFENCE_SPAWN_LIST=4,8,16,32 \
  taskset -c 0-3 target/release/examples/specfence_inflation_dig \
  --workers 1 --cpu-list 0-3
```

ict21, both keys, C=1, 4, 8, 16, 32, CPUs 128–255:

```bash
scripts/soft0_percore_scan.sh --cpu-list 128-255 --c-list 1,4,8,16,32 --k 10 --profile-k 1
SPECFENCE_CLASS_KEY=code_hash scripts/soft0_percore_scan.sh \
  --cpu-list 128-255 --c-list 1,4,8,16,32 --k 10 --profile-k 1 --skip-build
```

Correctness:

```bash
cargo +stable test -p pevm --release --features specfence \
  --config 'profile.release.lto=false' --lib specfence:: -- --test-threads=1
cargo +stable test -p pevm --release --features specfence \
  --config 'profile.release.lto=false' \
  --test specfence_stage1 --test sload_static_gas -- --test-threads=1 \
  sf_matches_onchain_focus_blocks sf_seq_par_repeat sload_static_gas_matches_chain_header
```
