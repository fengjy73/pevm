# SpecFence v2, stage 2

Stage 2 starts from Stage 1d (`cursor/specfence-v2-stage1d-0a77`, `7a98152`, PR #70). SpecFence stays in `crates/pevm/src/specfence/` and compiles only with `--features specfence`. Upstream `vm.rs`, `mv_memory.rs`, `scheduler.rs`, and `pevm.rs` are byte-identical to that commit. The example accepts `--cpu-list` and `--workers`. This opcode table is still the stock mainnet table: no handler is installed in place of `static_gas()`.

The ict21 Stage 1d scan (CPUs 128–255, K=10) is the problem this stage answers. On block 15274915, `(to, selector)`, SF went from 10.30 ms at C=1 to 6.40 ms at C=4, then 21.4 ms at C=16 and 48.8 ms at C=32, while Ideal_C had already plateaued at 1.49 ms. The hot location `0xabd6bb3978815b97` (77 read-modify-write writers, 76 hops) left its owner: 75/76 hops stayed on one worker at C=8, 63/76 at C=16, 42/76 at C=32, with hop gaps of 13.7 ms and 26.7 ms. `delta_mismatch` was 1–2 on some timed rounds. SF C=1 was 1.55× OCC C=1.

## What changed

- **Persistent pinned pool.** Threads are created once per process (`prepare_workers`) and reused across blocks. Each thread is pinned with `sched_setaffinity` to one CPU from the list the harness passes. Learned scheduler state is cleared with the block. `workers == 1` runs on the calling thread and does not enter the pool.
- **Bounded active set.** The set starts at 1. It doubles when ready-queue depth exceeds twice the active count and the chain is not stalled, otherwise it grows by one. It shrinks by half when the latest hop gap exceeds `max(100 µs, 8 × exec EMA)`, and by one when the idle ratio exceeds an adaptive ceiling. Extra workers sleep on a condvar. The trace records the active count at the end of the block and the peak during the block.
- **Hot-chain affinity.** The next read-modify-write is claimed onto the publisher's private slot and run on that same worker before the worker looks for other work. The slot is not a Chase-Lev deque entry, so another worker cannot steal it. If a later transaction blocks on a chain whose owner is already inside the active set, it is moved onto that owner's slot and the owner's work condvar is signaled. An inactive owner is left parked; the blocked transaction waits instead of waking that thread.
- **Serial overhead.** With `workers == 1` and tracing off, the engine skips read-origin recording, the read index, validation, the final rescan, and chain construction. Profile and in-block trace runs still record origins so the ideal schedule has edges. The serial commit now marks the profile attempt `kind = 1`, which is the row the report treats as the committed execution.
- **Plain-transfer prediction.** The first plain credit to a location is an absolute write: the account is not yet in multi-version memory, so the VM stores `MemoryValue::Basic` rather than `LazyRecipient(tx.value)`. That writer is the anchor. Later plain credits stay deltas and wait until the anchor has executed. A self-call is not a predicted credit. If execution is not lazy, `note_lazy_decision` clears the delta bit. A remaining mismatch records reader, location, writer, and a reason byte (`DELTA_BASE` 1, `DELTA_NON_LAZY` 2, `DELTA_AMOUNT` 3, `DELTA_ESTIMATE` 4, `DELTA_SEALED` 5).
- **OCC on the same pool.** Omitted. `Scheduler::try_execute` and `try_validate` are private in the frozen upstream files. Upstream `execute_revm_parallel` remains the OCC baseline.

## Cloud VM

Host: this VM, 4 vCPUs, KVM, Intel Xeon family 6 model 207, CPUs 0–3, no SMT. C=8 is eight workers on four CPUs. C=16 and C=32 were not run. Release build, `lto=false`, codegen-units 1. K=10, no warm-up, fresh engine per round, pool reused, learned state reset each block. Median and bootstrap 95% CI (10,000 resamples) from `scripts/specfence_inflation_report.py`. SEQ is the workers=1 sequential median from the C=1 scan only.

Ideal_C is the list schedule of the workers=1 SpecFence profile (`SPECFENCE_INFLATION=1`, one round). That profile records origins, so its per-transaction costs are the tracked path. The critical path stops shrinking at C=4 on 15274915: 1.244 ms for `(to, selector)` and 1.158 ms for `(code_hash, selector)`. The Stage 1d ict21 plateau of 1.49 ms is a different host.

### `(to, selector)`

Block 15274915. SEQ 3.456 ms [3.417, 3.507].

| C | OCC ms | OCC 95% CI | SF ms | SF 95% CI | Ideal_C ms | SF/OCC | active / peak | hot same | hot gap µs | span/exec |
| ---: | ---: | --- | ---: | --- | ---: | ---: | --- | --- | ---: | ---: |
| 1 | 5.346 | [5.231, 5.880] | 4.069 | [4.002, 4.215] | 3.837 | 0.76 | 1 / 1 | — | — | — |
| 4 | 3.267 | [2.969, 3.556] | 6.711 | [6.454, 7.024] | 1.244 | 2.05 | 1 / 4 | 76/76 | 19.7 | 1.33 |
| 8 | 3.870 | [3.619, 4.085] | 7.672 | [7.103, 8.787] | 1.244 | 1.98 | 4.5 / 8 | 76/76 | 211 | 1.99 |

FullReplay medians: 0, 6, 7. `delta_mismatch` 0 on every round. Hottest location is `0xabd6bb3978815b97` on all 20 parallel rounds. Pipelined hops 0.

Block 3356896. SEQ 0.317 ms [0.258, 0.325].

| C | OCC ms | OCC 95% CI | SF ms | SF 95% CI | Ideal_C ms | SF/OCC | active / peak | hot same | hot gap µs | span/exec |
| ---: | ---: | --- | ---: | --- | ---: | ---: | ---: | --- | ---: | ---: |
| 1 | 0.793 | [0.759, 0.904] | 0.342 | [0.337, 0.389] | 0.265 | 0.43 | 1 / 1 | — | — | — |
| 4 | 0.504 | [0.493, 0.613] | 1.197 | [1.148, 1.349] | 0.067 | 2.37 | 4 / 4 | 15/15 | 10.7 | 1.26 |
| 8 | 0.600 | [0.568, 0.764] | 1.359 | [1.275, 1.422] | 0.046 | 2.27 | 8 / 8 | 15/15 | 64.2 | 1.73 |

FullReplay 0. `delta_mismatch` 0. Hottest location `0x5e72d1250f2f8e02`.

### `(code_hash, selector)`

Block 15274915. SEQ 3.458 ms [3.414, 3.571].

| C | OCC ms | OCC 95% CI | SF ms | SF 95% CI | Ideal_C ms | SF/OCC | active / peak |
| ---: | ---: | --- | ---: | --- | ---: | ---: | --- |
| 1 | 5.408 | [5.216, 5.537] | 4.123 | [4.070, 4.337] | 3.637 | 0.76 | 1 / 1 |
| 4 | 3.266 | [2.970, 3.587] | 7.106 | [6.839, 7.712] | 1.158 | 2.18 | 1 / 4 |
| 8 | 3.886 | [3.775, 4.136] | 7.794 | [7.406, 9.290] | 1.158 | 2.01 | 1 / 8 |

`delta_mismatch` 0. FullReplay medians 0, 6, 7.5. The trace keeps one hottest chain. On this key that chain is `0xabd6bb3978815b97` on 5 of 10 rounds at C=4 and 5 of 10 at C=8; the other rounds are a longer chain that includes re-executions (`0xe5f04107525d854b`, and once `0xc845036eeda5f4cc`). Median hottest gap 106 µs at C=4 and 135 µs at C=8. Span/exec of that hottest chain is 1.55 and 1.79.

Block 3356896. SEQ 0.316 ms [0.267, 0.349].

| C | OCC ms | OCC 95% CI | SF ms | SF 95% CI | Ideal_C ms | SF/OCC | active / peak | hot same | hot gap µs | span/exec |
| ---: | ---: | --- | ---: | --- | ---: | ---: | ---: | --- | ---: | ---: |
| 1 | 0.824 | [0.697, 0.988] | 0.356 | [0.331, 0.398] | 0.256 | 0.43 | 1 / 1 | — | — | — |
| 4 | 0.545 | [0.515, 0.592] | 1.183 | [1.140, 1.281] | 0.064 | 2.17 | 4 / 4 | 15/15 | 18.4 | 1.33 |
| 8 | 0.641 | [0.588, 0.698] | 1.466 | [1.403, 1.630] | 0.047 | 2.29 | 6 / 8 | 15/15 | 93.3 | 2.82 |

`delta_mismatch` 0. FullReplay median 0; one C=8 round reached 16. One C=8 round's hottest location was `0xdff71d59d972d654` instead of `0x5e72d1250f2f8e02`.

## C=1 overhead

ict21 Stage 1d: SF 10.30 ms, OCC 6.66 ms, about 3.6 ms extra. This VM's Stage 1d binary, same block and key, K=10: SF median 6.021 ms, OCC median 5.115 ms (ratio 1.18). After the serial cut, the same-scan ratio is 4.069 / 5.346 = 0.76.

`SPECFENCE_BUCKETS=1` on 15274915, C=1, `(to, selector)`. The first round is cold. The next two rounds:

| Bucket | Stage 1d ms | Stage 2 ms |
| --- | ---: | ---: |
| interpreter | 2.844, 2.649 | 2.604, 2.458 |
| mv_record | 0.593, 0.626 | 0.359, 0.349 |
| validate | 0.096, 0.095 | 0, 0 |
| rescan | 0.080, 0.088 | 0, 0 |
| sched | 0.056, 0.058 | 0, 0 |
| pre_interp | 0.244, 0.271 | 0.163, 0.162 |
| writeset | 0.220, 0.226 | 0.207, 0.203 |
| lazy_eval | 0.383, 0.367 | 0.343, 0.346 |
| publish | 0.032, 0.033 | 0.031, 0.031 |

The cut removes validation, the rescan, and the scheduler mutex, and about halves `mv_record` (the read-set map and `note_read`). The interpreter stays about 2.5 ms and is the EVM itself. One parallel C=4 bucket run (thread time, not wall) still pays interpreter 6.63 ms, `mv_record` 2.42 ms, publish 2.22 ms, `pre_interp` 1.34 ms, coordinate 0.50 ms, validate 0.33 ms. The hot-chain span on the untimed C=4 scan is 0.72 ms, so the 3.4 ms gap from OCC C=4 (3.27 ms) to SF C=4 (6.71 ms) is that bookkeeping, which the `workers == 1` path skips and the parallel path keeps.

## delta_mismatch

Stage 1d preseed marked every plain credit as a delta of `tx.value`. `is_lazy` is true only when the recipient has no code and the sender or the recipient is already in multi-version memory. The initial template contains the beneficiary. The first touch of a new recipient therefore writes `MemoryValue::Basic` (the absolute account). A reader who folded `tx.value` before that write fails with `DELTA_NON_LAZY`. High C made the race visible: 1–2 aborts per timed round on ict21.

The first plain writer is now the anchor (an RMW, not a folded delta). Later plain writers stay deltas and park until the anchor has executed, so they take the lazy path and write `LazyRecipient(tx.value)`. A self-call writes `LazySender` and is not predicted as a credit. `note_lazy_decision` clears a delta if that execution was not lazy. `first_plain_touch_is_not_a_predicted_delta` locks the anchor, the later deltas, and the clear. `predicted_delta_that_becomes_rmw_mismatches` still rejects a fold of a non-lazy write.

This VM's K=10 scan recorded `delta_mismatch = 0` and an empty `delta_notes` list in every cell, both blocks, both keys, C=1, 4, and 8. If ict21 still sees a mismatch, the note carries the reader, the location, the writer (`u32::MAX` when absent), and the reason byte above.

## Gates on this VM

ict21 at C=1, 4, 8, 16, 32 is the authoritative remeasure. C=8 here is oversubscribed. C=16 and C=32 were not measured.

| Gate | Result |
| --- | --- |
| SF(C) ≤ 1.1 × min of SF(C') for C' ≤ C | Fail. 15274915 `(to, selector)`: SF(4)/SF(1) = 1.65, SF(8)/SF(1) = 1.89. `(code_hash, selector)`: 1.72 and 1.89. 3356896 `(to, selector)`: 3.50 and 3.97. C>1 pays origin tracking, publish, and coordination for the whole block. The fast serial path is cheaper than that parallel path on four vCPUs. The active set often ends at 1 on the big block after the hop-gap shrink, and the parallel bookkeeping stays on. |
| Hot-chain span ≤ 2 × RMW exec on 15274915 | Pass at the C values measured for `(to, selector)`: 1.33 at C=4, 1.99 at C=8. C=1 is the untracked serial path and records no hops. |
| Hops on the same worker, gap under 0.1 ms | Pass at C=4: 76/76, gap 19.7 µs. At C=8 the chain is still 76/76 and the span gate holds; the longest hop gap is 211 µs, above 0.1 ms, on an oversubscribed machine. |
| SF C=1 ≤ 1.2 × OCC C=1 on 15274915 | Pass. 0.76 for both class keys. |
| SF C=4 ≤ OCC C=4 on 15274915 | Fail. 6.711 vs 3.267 (2.05×) for `(to, selector)`, 7.106 vs 3.266 (2.18×) for `(code_hash, selector)`. Attribution is the parallel bucket above. The hot-chain span is 0.72 ms. |
| `sf_matches_onchain_focus_blocks`, `sf_seq_par_repeat`, `sload_static_gas_matches_chain_header`, Stage 1d delta unit tests | Pass. `specfence::` 25 tests, then the three integration tests (0.51 s and 3.79 s), on the final tree. SF output matches sequential execution inside those tests at C=1, 4, and 8. The K=10 rows all have `ok=true` and the header gas (29928443 and 4033966). |

## Reproduce

Build:

```bash
cargo +stable build -p pevm --release --features specfence \
  --config 'profile.release.lto=false' --example specfence_inflation_dig
```

This VM (4 vCPUs, C=8 oversubscribed). One class key and one worker count; repeat for `code_hash` and for `--workers` 1, 4, 8. No warm-up. SEQ is inside the workers=1 scan.

```bash
SPECFENCE_CLASS_KEY=to SPECFENCE_INFLATION_WHICH=scan SPECFENCE_INFLATION_K=10 \
  SPECFENCE_INFLATION_OUT=/tmp/sf-stage2/to-c4.jsonl \
  taskset -c 0-3 target/release/examples/specfence_inflation_dig \
  --workers 4 --cpu-list 0-3
python3 scripts/specfence_inflation_report.py \
  --wall /tmp/sf-stage2/to-c4.jsonl --cores 4
```

ict21, both keys, C=1, 4, 8, 16, 32, CPUs 128–255, no SMT siblings:

```bash
scripts/soft0_percore_scan.sh --cpu-list 128-255 --c-list 1,4,8,16,32 --k 10 --profile-k 1
SPECFENCE_CLASS_KEY=code_hash scripts/soft0_percore_scan.sh \
  --cpu-list 128-255 --c-list 1,4,8,16,32 --k 10 --profile-k 1 --skip-build
```

Ideal_C from one workers=1 profile (origins on) plus the wall file:

```bash
SPECFENCE_INFLATION=1 SPECFENCE_INFLATION_DUMP=1 SPECFENCE_INFLATION_K=1 \
  SPECFENCE_CLASS_KEY=to SPECFENCE_INFLATION_OUT=/tmp/ideal-to.jsonl \
  taskset -c 0-3 target/release/examples/specfence_inflation_dig \
  --workers 1 --cpu-list 0-3
python3 scripts/specfence_inflation_report.py \
  --wall /tmp/sf-stage2/to-c4.jsonl --profile /tmp/ideal-to.jsonl \
  --seq-profile /tmp/ideal-to.jsonl --cores 4
```

Correctness:

```bash
cargo +stable test -p pevm --release --features specfence --lib specfence:: -- --test-threads=1
cargo +stable test -p pevm --release --features specfence \
  --test specfence_stage1 --test sload_static_gas -- --test-threads=1 \
  sf_matches_onchain_focus_blocks sf_seq_par_repeat sload_static_gas_matches_chain_header
```
