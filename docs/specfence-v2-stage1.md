# SpecFence v2, stage 1

Stage 1 adds a SpecFence engine beside upstream Block-STM. The base is risechain/pevm `e94b0e3` (`ci: replace hand-rolled cache with Swatinem/rust-cache`), branch `specfence-v2`. SpecFence lives in `crates/pevm/src/specfence/` and is compiled only with `--features specfence`.

## Upstream OCC stays untouched

`vm.rs`, `mv_memory.rs`, `scheduler.rs`, and `pevm.rs` are byte-identical to `e94b0e3`. The only shared edits are:

- `Cargo.toml`: feature `specfence` and the `specfence_inflation_dig` example, which itself requires the feature.
- `lib.rs`: `#[cfg(feature = "specfence")] pub mod specfence;`

`cargo check -p pevm` without the feature does not compile the module. `Pevm::execute` and `execute_revm_parallel` do not call it. SpecFence owns its multi-version memory, its VM, and its scheduler. It reuses upstream types (`MemoryValue`, `MemoryEntry`, `PevmChain`) but does not patch them.

## Opcode gas

The old fork's on-chain miss (PR #66) was `Instruction::new(sload_handler, 0)` installed from `PevmEthereum::build_evm`. `insert_instruction` replaces static gas with the handler. The interpreter charges that static gas before the handler, and stock `SLOAD` does not charge it again, so every `SLOAD` lost 200 on Spurious Dragon and the warm 100 on London. Sequential execution used the same builder, so `seq == par` still failed the header.

This tree does not replace opcodes. `build_evm` is the stock mainnet builder. A later wrapper has to stay on the SpecFence EVM and copy the table's `static_gas()`.

`sload_keeps_spec_static_gas` reads that table: Spurious Dragon `SLOAD` is 200, London is 100. `sload_static_gas_matches_chain_header` runs sequential execution and upstream OCC, and with `--features specfence` also SpecFence, on 3356896 and 15274915. Gas matches the header on both. London also matches the receipt root. Spurious Dragon receipts still embed the post-state root (pre-EIP-658), which this engine does not rebuild, so that block checks gas and the logs bloom. The test passed with the feature off (0.58s) and on (0.57s).

## What was rewritten

The old fork (`cursor/specfence-exit-validation-47a7`) had patched the shared OCC files. This stage rewrites the pieces the v1.1 note keeps:

- **Read origins carry the value the interpreter consumed.** Validation walks the origin and requires transaction index, incarnation, and `MemoryValue` equality. An estimate or a mismatch fails the commit. Unit tests cover a same-incarnation rewrite and an estimate replaced under the same incarnation.
- **Exit.** The block returns only when `committed_upto == n` and a final scan finds every read set still matching. There is no `PartialAbortRebind` and no QuietExit.
- **Ordered writer chain.** A location enters the directory on the second writer, on the first read-then-write of a repeated class, or when validation fails. Readers wait for the chain-nearest lower writer. Blind and lazy writes are visible to readers and are not admission edges, so writers do not wait on each other. Read-then-write does. Cross-block input is radar only and is empty on a fresh run.
- **Class keys.** `(to, selector)` and `(code_hash, selector)` are selected per run (`SfClassKey`, `SPECFENCE_CLASS_KEY`). Chain budget starts at `clamp(n/32, 8, 64)`, moves inside `[8, 256]`, and the wait threshold moves inside safety bounds from abort rate and idle rate every 32 finished transactions.
- **Scheduling.** Chase-Lev deques. Worker `w` is seeded with `w, w+C, w+2C, ...`, pushed so the owner runs the low index first. The first wave is transactions `0..C`.

## Clean base

Before any SpecFence code, `specfence-v2` at `e94b0e3` was checked on the two focus blocks (sequential vs `execute_revm_parallel`, receipt root, logs bloom, `gas_used`). Both matched the header.

| Block | Transactions | Header gas |
| --- | ---: | ---: |
| 15274915 | 1226 | 29928443 |
| 3356896 | 176 | 4033966 |

The old fork's `3356896` gas mismatch (executed 4014166 vs header 4033966) is not present on this base.

## On-chain equivalence

`sf_matches_onchain_focus_blocks` runs sequential, unmodified OCC, and SpecFence for both blocks, at C=1, 4, and 8, for both class keys. It checks sequential = OCC = SpecFence, the receipt root, the logs bloom, and `gas_used`. That test passed.

`sf_seq_par_repeat` then ran SpecFence 12 times against one sequential result for each of C=4 and C=8, both keys, both blocks (96 SpecFence executions). All matched.

## In-block trace

Old fresh-run numbers from the previous fork, and the v1.1 targets:

| Block | Old re-exec | Old FullReplay | Old chain length | FullReplay target |
| --- | ---: | ---: | ---: | ---: |
| 15274915 | 122 | 50 | 0 | ≤ 12 |
| 3356896 | 20 | 20 | (carry 16) | ≤ 5 |

Chain length here is the longest non-beneficiary writer chain at the end of the block. The beneficiary is not inserted: every transaction writes it lazily, and that chain would have length `n`.

One fresh harness run (`SPECFENCE_INBLOCK_TRACE=1`, no warm-up) on this VM:

| Block | C | Class key | Re-exec | FullReplay | Reads after arm | FullReplay after arm | Chain length |
| --- | ---: | --- | ---: | ---: | ---: | ---: | ---: |
| 15274915 | 4 | to+selector | 83 | 83 | 149 | 78 | 997 |
| 15274915 | 4 | code_hash+selector | 87 | 87 | 119 | 84 | 77 |
| 15274915 | 8 | to+selector | 14 | 14 | 107 | 11 | 997 |
| 15274915 | 8 | code_hash+selector | 76 | 76 | 206 | 76 | 77 |
| 3356896 | 4 | to+selector | 28 | 28 | 34 | 15 | 17 |
| 3356896 | 4 | code_hash+selector | 18 | 18 | 30 | 17 | 17 |
| 3356896 | 8 | to+selector | 37 | 37 | 36 | 18 | 17 |
| 3356896 | 8 | code_hash+selector | 6 | 6 | 27 | 6 | 17 |

Three more fresh runs of the same binary, same flag. FullReplay on 15274915 with `code_hash+selector` was 7, 8, 82 at C=4 and 12, 13, 79 at C=8. `(to, selector)` stayed in the 77–91 band, and its longest chain was usually the 997-transaction plain transfer to `0x6262…98ced`. `code_hash+selector` kept the 77-writer spine (`0x7758…0c50`, empty calldata) when that location was not evicted. On 3356896 the longest chain was 17, next to the carry-round length 16, and FullReplay stayed about 8–39.

The low band (FullReplay 7–14 on the big block) meets the ≤12 target. The high band does not, and it is the common outcome. Most of those aborts happen after the location is already armed (`full_replay_after_arm` is close to `full_replay`). Stage 1b shows that counter was sampled after the abort armed the location. The caps hold after the chain fix.

## Harness

The scan and report are the PR #62 scripts (`scripts/soft0_percore_scan.sh`, `scripts/specfence_inflation_report.py`, `scripts/specfence_step_ideal.py`). OCC is native `Pevm::execute_revm_parallel`. There is no separate upstream crate. The build passes `--features specfence` because Cargo does not enable an example's `required-features` on its own.

`TPS_SEQ` is the workers=1 sequential median, copied onto every C. `TPS_OCC` is that native path. `TPS_SF` is `run_sf_block`. `TPS_ideal` is the report's critical-path list schedule. Native OCC records no per-transaction probe, so the schedule uses the workers=1 SpecFence profile (`SPECFENCE_INFLATION=1`): committed-incarnation `total_ns`, with beneficiary and lazy writes dropped. Wall rows leave `attempts` empty. There is no opcode hook, so the step-trace file is a meta line and `TPS_ideal_step` is absent.

The old fork's regressed hunks that sat on the SpecFence path are not in this tree:

- `SfVm::execute` calls `Instant::now` only when `SPECFENCE_INBLOCK_TRACE` or `SPECFENCE_INFLATION` is set.
- `NoBeneficiaryHandler` only skips the beneficiary reward. It does not note frame depth per opcode.
- Account reads walk the multi-version map in place.
- `record` stores the read set and the write locations. It does not keep a reader index.
- `execute_revm_parallel` does not build SpecFence tables. SpecFence is a separate entry point.

## Wall clock

Host: this VM, not ict21. 4 vCPUs, KVM, Intel Xeon family 6 model 207, one thread per core, CPUs 0–3, L3 320 MiB, governor unavailable. Release build with `lto=false` (codegen-units stays 1). Ten fresh wall rounds, one profile round, bootstrap of 10,000. No warm-up. C=8 is `--allow-oversub` onto CPUs 0–3, so it does not add cores.

Command: `scripts/soft0_percore_scan.sh --cpu-list 0-3 --allow-oversub --c-list 1,4,8 --k 10 --profile-k 1 --step-k 1`. The second class key is the same command with `SPECFENCE_CLASS_KEY=code_hash` and `--skip-build`. Sequential and OCC do not read the class key; each scan still remeasures them, so the two tables are separate samples.

### `(to, selector)`

Block 15274915, `TPS_SEQ` 344919, median 3.554 ms, 95% CI [3.507, 3.813].

| C | TPS_OCC | OCC ms | OCC 95% CI | TPS_SF | SF ms | SF 95% CI | TPS_ideal |
| ---: | ---: | ---: | --- | ---: | ---: | --- | ---: |
| 1 | 255851 | 4.792 | [4.574, 5.391] | 192717 | 6.362 | [6.123, 6.687] | 345861 |
| 4 | 379762 | 3.229 | [3.074, 3.598] | 212740 | 5.763 | [5.617, 6.087] | 1040559 |
| 8 | 336283 | 3.646 | [3.543, 3.738] | 155960 | 7.861 | [7.579, 8.276] | 1040559 |

Block 3356896, `TPS_SEQ` 664210, median 0.265 ms, 95% CI [0.263, 0.267].

| C | TPS_OCC | OCC ms | OCC 95% CI | TPS_SF | SF ms | SF 95% CI | TPS_ideal |
| ---: | ---: | ---: | --- | ---: | ---: | --- | ---: |
| 1 | 453393 | 0.388 | [0.371, 0.398] | 276634 | 0.636 | [0.626, 0.643] | 636717 |
| 4 | 329567 | 0.535 | [0.477, 0.710] | 156490 | 1.126 | [0.944, 1.233] | 2530299 |
| 8 | 301059 | 0.585 | [0.563, 0.632] | 134898 | 1.305 | [1.257, 1.413] | 4185593 |

### `(code_hash, selector)`

Block 15274915, `TPS_SEQ` 330903, median 3.705 ms, 95% CI [3.582, 3.899].

| C | TPS_OCC | OCC ms | OCC 95% CI | TPS_SF | SF ms | SF 95% CI | TPS_ideal |
| ---: | ---: | ---: | --- | ---: | ---: | --- | ---: |
| 1 | 272182 | 4.508 | [4.307, 5.375] | 162577 | 7.543 | [7.366, 8.281] | 265987 |
| 4 | 375366 | 3.266 | [3.059, 3.420] | 141284 | 8.678 | [8.250, 9.169] | 1012037 |
| 8 | 302556 | 4.053 | [3.697, 4.634] | 152853 | 8.026 | [7.203, 8.779] | 1012037 |

Block 3356896, `TPS_SEQ` 648196, median 0.272 ms, 95% CI [0.266, 0.277].

| C | TPS_OCC | OCC ms | OCC 95% CI | TPS_SF | SF ms | SF 95% CI | TPS_ideal |
| ---: | ---: | ---: | --- | ---: | ---: | --- | ---: |
| 1 | 447612 | 0.393 | [0.372, 0.407] | 244100 | 0.721 | [0.708, 0.741] | 447037 |
| 4 | 304229 | 0.579 | [0.530, 0.729] | 123323 | 1.427 | [1.357, 1.454] | 1784157 |
| 8 | 278692 | 0.632 | [0.575, 0.682] | 118466 | 1.486 | [1.404, 1.609] | 2674162 |

On this 4-core VM, unmodified OCC at C=4 is the first point that beats sequential on the big block. SpecFence is slower than both. C=8 does not add CPUs. On 15274915 the ideal makespan already equals the critical path at C=4 (1.178 ms for `to+selector`, 1.211 ms for `code_hash+selector`), so `TPS_ideal` does not rise from C=4 to C=8. The profile covered every transaction (`missing_txs=0`). `seqcheck` and `occcheck` both reported `diverge=0` and the header gas (29928443 and 4033966).

## Deviations from the design

- **No opcode hook.** Predicted locations are marked running when the transaction starts. The concrete write set is published when the interpreter returns. Stage 1 read the bimodal FullReplay as “the next writer has not published yet.” Stage 1b measured a different cause: the reader was not waiting on a chain entry that already existed. See that section. Stage 6 (publish before the interpreter returns) is not in this stage.
- **Stage 1 scheduling only.** No persistent pool, no learned `C_eff`, no Ideal-ready pool, no late split, no IntraPatch, no QuietExit. The two ablations that need a persistent pool (OCC on that pool, SpecFence with the chain off) are later.
- **Seeding is strided index order**, not critical-path order. Contiguous chunks made the tail of the block run before the head had published; the stride keeps the first wave at transactions `0..C`.
- **Beneficiary writes are skipped** by the chain. They are still applied as lazy rewards in multi-version memory.
- **The online controller is a small AIMD tick**, not the full Part 2 concurrency controller. Safety bounds are the constants `K` in `[8, 256]`, abort-cost in `[5µs, 2ms]`, and class-hit floor 0.15. A plain-transfer class can be hundreds of transactions; its eviction priority is capped at 32 so it does not displace a location that has already failed validation.
- **`TPS_ideal` is the PR #62 critical-path schedule of SpecFence workers=1 profile attempts.** Native OCC has no per-transaction probe, and this stage does not record an opcode step trace, so `TPS_ideal_step` is absent. Beneficiary and lazy writes are dropped from the DAG, matching that report.
- **Nonce and balance waits use the previous same-sender transaction**, not `tx-1`. Blocking on `tx-1` after that unrelated transaction had committed spun the worker and stalled the commit prefix.

## Open issues

The FullReplay caps and the C=1 overhead are closed in [Stage 1b](#stage-1b). What remains:

- At C=4 and C=8, SpecFence is still slower than unmodified OCC on this VM. C=8 oversubscribes four vCPUs. A wall-clock comparison with the 128-core host is not meaningful here.
- A few FullReplay remain from cross-class storage overlap. They sit under the caps.
- `TPS_ideal` is still the workers=1 profile schedule. There is no opcode step trace.

## Stage 1b

Both Stage 1 gates pass on fresh runs. A reader of an armed location now waits until the chain-nearest lower writer has committed its final write. At C=1, SpecFence is within 1.10× of upstream OCC on both blocks, for both class keys. Upstream `vm.rs`, `mv_memory.rs`, `scheduler.rs`, and `pevm.rs` are still byte-identical to `e94b0e3`. No opcode is replaced.

### Abort root cause

`full_replay_after_arm` was sampled at abort time, after `arm_failure`. It counted locations that became armed because the read failed, not locations that were armed when the reader read. The design wait never ran, for two measured reasons.

**The invalidating writer was not in the chain at the read.** On a fresh pre-fix run (`SPECFENCE_ABORT_DIAG=1`, `(to, selector)`, C=4, block 15274915) FullReplay was 78 and the longest chain was 997. The abort log, taken before `arm_failure`:

| Counter | Count |
| --- | ---: |
| FullReplay | 78 |
| `armed_at_read` | 17 |
| nearest chain writer is the invalidating writer | 12 |
| invalidating writer is not the nearest | 66 |
| reason `no_lower_writer` | 61 |
| reason `ready_published` | 17 |

72 of the 78 aborts are location `0xabd6bb3978815b97`, the basic account of the 997-recipient plain transfer. At the read, that location was not armed and `nearest_lower` was `None` (`read_armed=false`, `read_nearest` absent, reason `no_lower_writer`). By commit the location was armed and the invalidating writer was `Published`. The reader had nothing to wait for.

The chain produced that hole on purpose:

- The first publisher was held in `seen` until a second publisher. A reader of the first write found an empty directory.
- A classmate that was predicted but had not started was absent until someone published that location.
- Eviction could drop the spine. Under `(to, selector)` the plain-transfer class is 997 writers, and a chain that is not pinned is an eviction candidate.
- Sibling prediction on any repeated-class write, including a lazy sender balance, would have marked the whole class as writers of one account. That path was measured and removed. Blind and lazy writes stay reader-visible and are not admission edges.

**`Published` is not the final write.** Validation runs only when the transaction is the commit head, so a published incarnation can still fail and store a different value. On 3356896, `(code_hash, selector)`, C=8, reader 31 read location `0xdff71d59d972d654` with origin none and live writer 4, `read_armed=false`, reason `no_lower_writer`. Readers 66, 67, 69, and 70 then accepted transaction 31's `Published` value (`ready_published`, `read_state=3`) and aborted when transaction 31 rewrote it. The origin index matched the live index; the incarnation and the value did not. A 16-abort mode on that block was the largest class that read the location before the slot was armed. Releasing the wait on the running mark, or on `Published` of an uncommitted incarnation, commits the same stale read.

### What changed in the chain

A reader of an armed location proceeds only when the nearest lower chain writer is committed and published, or has committed without writing that location (a hole). An unarmed location still treats `Published` as enough. Commit is a prefix, so waiting for that writer to commit makes every earlier write final too.

Chain insertion happens before the reader needs it:

- Before workers start, every repeated `Basic(to)` (count ≥ 2, beneficiary skipped) is an armed pinned chain of `Predicted` members. Those members are not admission edges.
- The first publisher takes a free directory slot immediately. It is not held in `seen` until a second publisher.
- A read-then-write of a repeated class predicts the other open classmates and sets the admit bit. A blind or lazy publish inserts only the publisher.
- Contract classes (calldata length ≥ 4) park later classmates on the class head until that head has executed. Plain transfers do not: the 997-recipient chain is already preseeded, and a head barrier would serialize it. Waits go only to a lower index.
- Validation failure arms the location, notes the conflict writer while it is still open, backfills the previous write set, and keeps the abort residual. Pinned chains (preseed, class open, arm-on-failure, abort residual, two or more writers) are not eviction candidates.

`Phase::Parked` carries `until_commit`. An admission or class-head park ends when the predecessor has executed. An armed-read park ends when the predecessor has committed. `finish_ok` wakes the first kind. `try_mark_committed` wakes the second.

### FullReplay after the fix

Ten fresh runs, no warm-up, timers off, on the binary that has the chain fix. No run landed in the old 77–91 band or the 16-abort band.

| Block | C | Class key | FullReplay, 10 runs | Max |
| --- | ---: | --- | --- | ---: |
| 15274915 | 4 | to+selector | 3, 4, 4, 6, 4, 3, 4, 4, 4, 5 | 6 |
| 15274915 | 8 | to+selector | 7, 5, 6, 4, 4, 4, 6, 3, 3, 3 | 7 |
| 15274915 | 4 | code_hash+selector | max 4, mostly 3 | 4 |
| 15274915 | 8 | code_hash+selector | max 4 | 4 |
| 3356896 | 4 | to+selector | 0 × 10 | 0 |
| 3356896 | 8 | to+selector | one 1, nine 0 | 1 |
| 3356896 | 4 | code_hash+selector | max 1 | 1 |
| 3356896 | 8 | code_hash+selector | max 1 | 1 |

Caps are ≤ 12 and ≤ 5. Both keys meet them. `(code_hash, selector)` has the lower FullReplay on the big block. `(to, selector)` has the lower wall clock once C > 1.

The remaining aborts are cross-class storage overlap: different `(to, selector)` pairs writing the same pool. The class head cannot see that slot until its own interpreter returns, and it does not cover a different class. The width is about C, under the cap.

A later C=1 path (below) does not change the chain when `workers > 1`, except that the deadline clock is read every 64 scheduler spins. Three fresh runs after that change stayed in the same band: 15274915 `(to, selector)` was 5, 5, 4 at C=4 and 2, 4, 6 at C=8; `(code_hash, selector)` at C=8 was 3, 3, 3. The small block was 0 or 1.

### C=1 attribution

`perf` is not installed. The kernel is `6.12.94+` and `linux-tools` for that version is not in the apt index. Attribution uses scoped timers (`SPECFENCE_BUCKETS=1`). The flag is off in the wall-clock scan, and `Guard::start` does not call `Instant::now` when it is off.

The split below is one instrumented pass of block 15274915 at C=1, before the fast path. Timers inflate the wall (10.705 ms here, about 7–7.5 ms untimed on that binary). Read the column as composition, not as the scan median.

| Bucket | ms | Calls | What it is |
| --- | ---: | ---: | --- |
| interpreter | 3.781 | | shared with OCC |
| mv_record | 1.142 | | read-set and write-set record |
| lazy_eval | 0.538 | | beneficiary rewards |
| publish | 0.461 | | chain insert on every write |
| alloc_chain | 0.443 | | 256 chains plus membership |
| coordinate | 0.182 | 2593 | directory lock and nearest-lower |
| rescan | 0.149 | | final read-set scan |
| validate | 0.123 | | origin compare at commit |
| deadline_clock | 0.081 | 1226 | `Instant::now` once per transaction |
| mark_running | 0.074 | | predicted → running |
| sched | 0.070 | | deque pop and park |
| class_key | 0.066 | | `(to, selector)` hashing |
| preseed | 0.051 | | repeated-recipient chains |
| runtime_seed | 0.048 | | phase table and stride seed |

Chain and scheduling (alloc, publish, coordinate, deadline, mark, class, preseed, runtime, sched) are about 1.5 ms. The unbucketed remainder on that binary was the `thread::scope` spawn of the single worker. Shared with OCC: the interpreter, the multi-version record, lazy evaluation, and one validation. SpecFence keeps, when `workers > 1`, a value-carrying origin and a final rescan. OCC stores only the transaction index and the incarnation.

After the C=1 path, the same instrumented pass (wall 6.651 ms on the big block, 0.679 ms on 3356896, still inflated) shows `alloc_chain`, `class_key`, `preseed`, `coordinate`, `mark_running`, and `mv_template` at 0. `publish` is 0.039 ms of empty-loop guard. What remains on the big block: interpreter 3.226, mv_record 0.429, lazy_eval 0.412, writeset 0.285, pre_interp 0.255, validate 0.117, rescan 0.093, sched 0.067. The lazy walk is not quadratic: pre-interpreter guards were 1324 against 1226 transactions, about 98 extra origin steps.

### What the C=1 path skips

When `workers == 1` the previous transaction is committed before the next read, so the chain cannot change a result. That path:

- Builds `LiveChain::untracked`: no directory, and `coordinate` returns before the lock.
- Skips class hashing, recipient preseed, and the upstream beneficiary-estimate template. The beneficiary is still lazy, so rewards evaluate at the end.
- Stores `SfReadOrigin::MvId { tx_idx, incarnation }` instead of cloning `MemoryValue` into every origin. `workers > 1` still stores the value and compares it.
- Reads the deadline every 64 scheduler spins, not once per transaction.
- Runs the worker inline on the calling thread. `thread::scope` is used only when `workers > 1`. This was the last large gap: the same-process ratio against OCC moved from about 1.15 to about 1.00 before the fresh scan below.
- Skips admission and the class-head barrier. The published-location vector is empty.

Validation in the commit loop and the final rescan still run at C=1. Identity is enough there because one worker cannot rewrite an incarnation under a later read.

### Wall clock after the fix

Host for this scan: 4 vCPUs, KVM, Intel Xeon family 6 model 143, one thread per core, CPUs 0–3, L3 105 MiB, governor unavailable. The Stage 1 tables were family 6 model 207 with L3 320 MiB, and their absolute milliseconds are lower. The gate is the SF/OCC ratio inside one scan. Release build, `lto=false`, codegen-units 1. K=10 fresh rounds, no warm-up, bootstrap 10,000. SEQ is one workers=1 baseline and is not repeated per C. OCC is `execute_revm_parallel`. C=8 is `--allow-oversub` onto CPUs 0–3.

Command: `scripts/soft0_percore_scan.sh --cpu-list 0-3 --allow-oversub --c-list 1,4,8 --k 10 --profile-k 1 --step-k 1 --skip-build`. The second key sets `SPECFENCE_CLASS_KEY=code_hash`. `seqcheck` and `occcheck` reported `diverge=0` and header gas 29928443. They compare sequential with OCC only. SpecFence equivalence is the Rust test below.

Stage 1 C=1 ratios, for the before column: 15274915 was 6.362/4.792 = 1.33 (`to+selector`) and 7.543/4.508 = 1.67 (`code_hash+selector`). 3356896 was 0.636/0.388 = 1.64 and 0.721/0.393 = 1.83.

#### `(to, selector)`

Block 15274915, `TPS_SEQ` 295576, median 4.148 ms, 95% CI [4.007, 4.299]. C=1 SF/OCC = 5.933/5.654 = 1.05.

| C | TPS_OCC | OCC ms | OCC 95% CI | TPS_SF | SF ms | SF 95% CI | TPS_ideal |
| ---: | ---: | ---: | --- | ---: | ---: | --- | ---: |
| 1 | 216822 | 5.654 | [5.539, 6.309] | 206686 | 5.933 | [5.650, 6.185] | 322309 |
| 4 | 322116 | 3.806 | [3.523, 4.183] | 160115 | 7.657 | [7.306, 7.931] | 885743 |
| 8 | 283978 | 4.317 | [4.094, 4.606] | 115232 | 10.669 | [7.752, 12.875] | 885743 |

Block 3356896, `TPS_SEQ` 550074, median 0.320 ms, 95% CI [0.308, 0.330]. C=1 SF/OCC = 0.527/0.636 = 0.83.

| C | TPS_OCC | OCC ms | OCC 95% CI | TPS_SF | SF ms | SF 95% CI | TPS_ideal |
| ---: | ---: | ---: | --- | ---: | ---: | --- | ---: |
| 1 | 276943 | 0.636 | [0.510, 0.745] | 334091 | 0.527 | [0.511, 0.543] | 683569 |
| 4 | 288212 | 0.611 | [0.545, 0.667] | 124545 | 1.414 | [1.355, 1.499] | 2731435 |
| 8 | 271702 | 0.648 | [0.584, 0.697] | 112589 | 1.564 | [1.464, 1.669] | 3842291 |

#### `(code_hash, selector)`

Block 15274915, `TPS_SEQ` 302475, median 4.054 ms, 95% CI [3.869, 4.147]. C=1 SF/OCC = 5.697/5.322 = 1.07.

| C | TPS_OCC | OCC ms | OCC 95% CI | TPS_SF | SF ms | SF 95% CI | TPS_ideal |
| ---: | ---: | ---: | --- | ---: | ---: | --- | ---: |
| 1 | 230399 | 5.322 | [5.081, 6.282] | 215208 | 5.697 | [5.527, 5.788] | 327039 |
| 4 | 317190 | 3.865 | [3.646, 4.110] | 131056 | 9.355 | [9.154, 9.828] | 869604 |
| 8 | 291178 | 4.211 | [4.064, 4.315] | 114410 | 10.716 | [10.066, 11.471] | 869604 |

Block 3356896, `TPS_SEQ` 550717, median 0.320 ms, 95% CI [0.305, 0.326]. C=1 SF/OCC = 0.510/0.644 = 0.79.

| C | TPS_OCC | OCC ms | OCC 95% CI | TPS_SF | SF ms | SF 95% CI | TPS_ideal |
| ---: | ---: | ---: | --- | ---: | ---: | --- | ---: |
| 1 | 273459 | 0.644 | [0.470, 0.671] | 345356 | 0.510 | [0.494, 0.542] | 662459 |
| 4 | 257339 | 0.684 | [0.637, 0.730] | 100798 | 1.746 | [1.598, 1.772] | 2634731 |
| 8 | 239381 | 0.735 | [0.695, 0.831] | 89326 | 1.970 | [1.888, 2.026] | 3805241 |

On the big block the SF 95% CI upper is also within 1.09× of the OCC median (6.185/5.654 and 5.788/5.322). On 3356896 the OCC interval is wide; SF's CI upper is below the OCC median for both keys.

### Equivalence

`sf_matches_onchain_focus_blocks` was re-run on the final binary: sequential, unmodified OCC, and SpecFence, both blocks, C=1, 4, and 8, both class keys. Receipt root, logs bloom, and gas matched (3.89s). `sf_seq_par_repeat` (12 runs at C=4 and C=8, both keys, both blocks) passed on that same binary (1.17s). `sload_static_gas_matches_chain_header` still matches header gas 29928443 and 4033966 (0.66s).

### Still open

- At C=4 and C=8 SpecFence remains well above OCC. On 15274915 with `(to, selector)`, C=4 is 7.657 ms against OCC 3.806 ms. That is outside this stage's gate.
- `(code_hash, selector)` pays a storage lookup while building classes, so it is slower than `(to, selector)` once C > 1.
- Cross-class RAW still produces a few FullReplay, under the cap.
- `TPS_ideal` is the workers=1 profile critical path. The step-trace file is a meta line.
- Absolute milliseconds moved with the host (model 143, L3 105 MiB versus the Stage 1 model 207, L3 320 MiB). Quote the same-scan ratio.

## Stage 1c

The C=4 gate is not met. On a fresh K=10 scan of the binary below, SpecFence at C=4 is slower than upstream OCC at C=4, and slower than SpecFence at C=1, on both blocks and both class keys. FullReplay on 15274915 stays at most 8. On 3356896 it is 0 in 39 of the 40 multi-worker rounds; one `(to, selector)` C=8 round is 16, above the cap of 5.

What the timelines showed, and what the code now does, is still the useful result. Armed reads were waiting for the producer's commit. They now wait for that incarnation's read-from finality. The wall stays on the preseeded nearest-lower chain: 997 members, 77 real writers, and every next writer still starts only after the previous writer has finished executing.

Upstream `vm.rs`, `mv_memory.rs`, `scheduler.rs`, and `pevm.rs` are byte-identical to `e94b0e3`. No opcode is replaced.

### Attribution before the wake-rule change

One instrumented run per cell, `SPECFENCE_TIMELINE=1`, fresh state, no warm-up. The internal `wall_ns` excludes rdtsc calibration and the JSON dump. Timers are off in the K=10 scan below; these walls are the before column on this host, and they are higher than an untimed run of the same binary.

Host: 4 vCPUs, KVM, Intel Xeon family 6 model 207, one thread per core, CPUs 0–3, L3 320 MiB (cache size 327680 KB), L2 8 MiB. This is the Stage 1 host, not the Stage 1b host (family 6 model 143, L3 105 MiB). Absolute milliseconds are not comparable across those two machines.

The list schedule built from this run's per-transaction `(end − start)` costs sits close to the wall (ratio 1.03–2.36 in the timeline report). Those costs already include in-transaction chain waits, and the report's critical-path walker over-counts (`cp_cover` from 1.59 to 84). That makespan is not Ideal_C. The segment that explains the wall is the writer chain, measured directly from publish order.

#### Writer chain

Non-lazy writers of the hottest location. A hop is the gap from one writer's execution end to the next writer's execution start. Commit lag is the time that writer sat executed and not yet committed.

| Block | C | Class key | Wall ms | Writers | Exec sum ms | Span ms | Hops after previous commit | Gap ms | Commit lag ms |
| --- | ---: | --- | ---: | ---: | ---: | ---: | --- | ---: | ---: |
| 15274915 | 4 | to+selector | 7.091 | 77 | 0.596 | 4.182 | 76/76 | 3.586 | 3.495 |
| 15274915 | 8 | to+selector | 11.372 | 77 | 0.665 | 7.305 | 74/76 | 6.639 | 6.561 |
| 15274915 | 4 | code_hash+selector | 11.136 | 77 | 0.695 | 7.448 | 76/76 | 6.754 | 6.651 |
| 15274915 | 8 | code_hash+selector | 12.377 | 77 | 0.645 | 9.010 | 76/76 | 8.365 | 8.261 |
| 3356896 | 4 | to+selector | 1.595 | 17 | 0.209 | 0.896 | 16/16 | 0.687 | 0.489 |
| 3356896 | 8 | to+selector | 2.102 | 16 | 0.199 | 0.541 | 13/15 | 0.342 | 0.323 |
| 3356896 | 4 | code_hash+selector | 2.301 | 17 | 0.274 | 1.404 | 15/16 | 1.130 | 0.044 |
| 3356896 | 8 | code_hash+selector | 2.691 | 17 | 0.296 | 1.831 | 15/16 | 1.535 | 0.191 |

On 15274915 the location is `0xabd6bb3978815b97`, the basic account of the 997-recipient plain transfer (`chain_len` 997). On 3356896 it is `0xdff71d59d972d654`. DAG edges on the big block, `(to, selector)`: 112 RAW, about 1146 WAW, about 110 WAR, 15 sender, and 2 lazy WAW. Lazy beneficiary updates are not on this chain.

Of the 3.586 ms between consecutive writers at C=4 `(to, selector)`, 3.495 ms (97%) is the previous writer waiting to commit. The time from that commit to the next writer's start is 0.091 ms. At C=8 the same split is 6.561 of 6.639 ms. The 77 writers' own execution is 0.60–0.70 ms. Raising C lengthens the span (4.18 ms to 7.31 ms) because commit is one prefix and every hop waits for it.

#### Worker time and top waits

Thread-sum milliseconds. Park time overlaps across workers, so it is not a share of the wall. Validation is the compare, not the commit lag.

| Block | C | Key | Exec | Validate | Idle | Spin | Queue | Park thread-sum (reason: ms / n) |
| --- | ---: | --- | ---: | ---: | ---: | ---: | ---: | --- |
| 15274915 | 4 | to | 12.082 | 0.342 | 5.748 | 0.089 | 1.689 | armed 206.4/114, nonce 9.90/8, class-head 6.31/8 |
| 15274915 | 8 | to | 23.140 | 0.702 | 38.798 | 2.420 | 3.620 | armed 2570/490, nonce 19.1/11, admission 16.5/12, class-head 15.5/13 |
| 15274915 | 4 | code_hash | 11.977 | 0.575 | 18.638 | 0.524 | 2.471 | admission 4220/1064, armed 347/111, class-head 6.53/13 |
| 15274915 | 8 | code_hash | 13.364 | 0.590 | 50.424 | 7.045 | 1.950 | admission 5276/1070, armed 463/112, class-head 15.4/21 |
| 3356896 | 4 | to | 1.708 | 0.080 | 1.035 | 0.026 | 0.385 | nonce 13.1/32, armed 10.7/38, class-head 0.56/3 |
| 3356896 | 8 | to | 2.433 | 0.106 | 4.766 | 0.663 | 0.163 | nonce 24.7/36, armed 18.0/37, admission 9.59/15 |
| 3356896 | 4 | code_hash | 1.693 | 0.102 | 3.382 | 0.067 | 0.325 | admission 132/151, armed 14.3/22 |
| 3356896 | 8 | code_hash | 2.154 | 0.118 | 8.555 | 1.512 | 0.358 | admission 142/150, armed 18.6/26 |

Per-worker execute on 15274915 `(to, selector)` C=4 is 3.02, 3.10, 2.85, and 3.11 ms. The parallel phase is about 5.4 ms against a 7.09 ms wall. Commit-lag median on that cell is 2.36 ms (p90 2.76 ms).

Top waits by `(reason, location, class)`, thread-sum:

| Cell | Reason | Location | Class | ms | Parks |
| --- | --- | --- | --- | ---: | ---: |
| 15274915 C=4 to | armed read | `abd6bb3978815b97` | 6 | 167.1 | 76 |
| 15274915 C=4 to | armed read | `de1644810b012b46` | 10 | 11.1 | 4 |
| 15274915 C=4 to | armed read | `693c76ebc9d30041` | 2 | 9.3 | 11 |
| 15274915 C=4 to | nonce / sender | `483a65c8273c8219` | — | 5.0 | 3 |
| 15274915 C=4 code_hash | admission | — | 6 | 4220 | 1064 |
| 15274915 C=4 code_hash | armed read | `abd6bb3978815b97` | 2 | 327.1 | 76 |
| 3356896 C=4 to | armed read | `5e72d1250f2f8e02` | 24 | 4.1 | 15 |
| 3356896 C=4 to | nonce / sender | `8d66eb7a3b2d4996` | 37 | 2.5 | 9 |
| 3356896 C=4 code_hash | admission | — | 3 | 129.2 | 146 |
| 3356896 C=4 code_hash | armed read | `5e72d1250f2f8e02` | 2 | 13.5 | 13 |

Class-head parks with no true read-from or sender edge: 5 of 8 (3.06 of 6.31 ms) on 15274915 `(to, selector)` C=4, and 9 of 13 (4.04 of 6.53 ms) on `(code_hash, selector)` C=4. There is no admission park on `(to, selector)` C=4. `(code_hash, selector)` hashes an empty code with selector 0 into one class, so transfers and EOAs share class 6 and the same-class RMW prediction admits them.

Cycle time inside execution on 15274915 `(to, selector)` C=4: publish 1.92 ms, record 1.75 ms, pre-interpreter 1.34 ms, scheduler 1.14 ms, coordinate 0.73 ms. The 77-writer execution sum is 0.60 ms. Bookkeeping is real thread time. It is not the 4.18 ms span.

#### Hypotheses

- **(a) Confirmed.** On 15274915 `(to, selector)` C=4, all 76 writer hops start after the previous writer's commit. Commit lag is 3.495 of the 3.586 ms inter-hop gap, 97%. At C=8 it is 6.561 of 6.639 ms. On 3356896 `(to, selector)` C=4 it is 16/16 hops and 0.489 of 0.687 ms. Armed-read parks that start only after the producer has finished are 15.6 of 206 ms (7.6%) on the big-block C=4 cell; the rest of that thread-sum is readers parked before the producer finishes, which is the same commit-prefix queue forming early.
- **(b) The class-head barrier is not the wall. Same-class admission is, for one key.** Class-head thread-sum is 6.3 ms on the big block at C=4, and about half of those parks have no true edge. `(code_hash, selector)` admission on class 6 is 4220 ms across 1064 parks. That is a serialized classmate queue, and it is why that key's writer span is 7.45 ms rather than 4.18 ms.
- **(c) Confirmed as the mechanism of (a).** Validation CPU is 0.34 ms on the big block at C=4. The delay is that the compare runs when the transaction becomes the commit head, so a published write is not final and every armed reader waits for the prefix. Commit-lag p50 is 2.36 ms.
- **(d) Real, and smaller than the chain.** Publish, record, and coordinate are inside the execution sum. They do not account for a 4.18 ms span built from 0.60 ms of writer execution. Value-carrying origins stay: the compare in PR #65 is the soundness check, and deleting them was not justified by this split.

### Root cause

Commit is a single prefix, and an armed read treated "the nearest lower chain writer has committed" as the signal that the write was final. On `0xabd6bb3978815b97` the chain is preseeded with 997 predicted recipients. The 77 transactions that actually write it then run one after another, each waiting out the commit lag of the previous one. Four workers do not shorten that chain. Eight workers, oversubscribed on four CPUs, make the prefix slower, so the span grows from 4.18 ms to 7.31 ms and the wall grows with it.

The C=1 path never enters this machinery. `workers == 1` uses `LiveChain::untracked`, skips class keys and preseed, and stores index origins. C=1 parity with OCC does not say the multi-worker chain is cheap.

### What changed

- **Dependency-closed finality.** An incarnation's write is final when that incarnation has finished, every read it consumed is storage or a final incarnation, and the value-and-identity compare passed. `validated[tx]` holds that incarnation, or `usize::MAX` after an abort or an estimate replacement. An armed reader waits until the nearest lower chain member is a final write or a final hole. `until_commit` is gone. Admission and the class head still wake on execution. Commit stores the same validated incarnation so a committed transaction is final, and then wakes anyone still parked.
- **Early validation off the commit head.** `close_from` runs when an incarnation finishes and again when a dependency becomes final. Origins that are final, and no unresolved chain member between the origin and the reader, mark the incarnation and walk its readers. A mismatch aborts immediately and cascades executed readers whose origins no longer match. The commit loop skips the compare when the incarnation is already final. If it is not, the loop compares once, and a passing transaction is marked final so the prefix does not wait. After the commit lock drops, each newly final transaction fans out to readers that were chain-blocked. The final rescan is unchanged.
- **A storage read does not block on lower preseed members.** The read already observed no lower write. A member that publishes later revokes the reader. Blocking that read on every preseeded index below it tied finality back to the commit prefix.
- **Class head and same-class sibling prediction start off.** A validation failure sets the barrier on the reader class and the writer class, then inserts still-open classmates with `admit = false`. Repeated `Basic(to)` preseed stays. Those members are known writers of one account, and removing the preseed brought FullReplay back to 15–21 without lowering the wall.
- **Hole clearing walks the whole prefix of final non-writers.** A cap of eight holes requeued the reader once per handful of preseeded members and put that chain back on the critical path. The loop stops if it would clear more holes than the reader's index.

`workers == 1` still skips the chain, the class hash, preseed, and value-carrying origins.

### Finality

**Finality is the read-from closure, and the graph has no cycles.** Storage is final. An incarnation is final only after execution, a passing value-and-identity compare, and every consumed origin being final. Reads name a lower transaction index, so the edges point backward. Abort and estimate replacement clear `validated[tx]` and bump the incarnation. A reader that observed the old incarnation fails the compare.

**Commit stays a prefix for receipts and state.** It is not the wakeup. `try_mark_committed` writes the validated incarnation, so commit implies final. A transaction can be final while lower transactions are still uncommitted.

**The chain-nearest member is the wait, including a member that has not started.** A published member is ready when that incarnation is final. A predicted or running member that finishes without a write is a final hole and leaves the chain. Members below the read-from origin belong to that writer; they do not block this reader. A predicted member between the origin and the reader still blocks, because it may publish a write the reader has not seen. Unit tests cover a cascade that closes before any commit, a final hole, a lower member that does not block, a predicted gap that does block until it finishes, and a reader that wakes while the commit prefix is still below it.

**A writer that appears later revokes higher executed readers.** Publish walks readers above the new writer. An executed reader whose origin no longer matches aborts, and its readers cascade, under the same lock as commit. A reader that has not finished is left to its own read. A higher index cannot be committed before every lower index, so a revoked reader is not already in the output. Estimates and aborts clear finality before the re-execution.

**WAR adds no ordering edge.** RAW and WAW use the nearest lower chain writer. A writer inserted between the recorded origin and the reader fails the value compare or the publish-time revoke.

**Early validation is the same compare.** It may run as soon as the origins are final. The commit head repeats it when the flag is unset. The rescan after the prefix still walks every transaction.

### Wall clock after the change

The post-fix timeline (same internal clock, 15274915, `(to, selector)`, C=4) has wall 7.685 ms and FullReplay 5. All 87 armed parks start before the producer finishes executing. None start after the producer's commit. The 77 writers still span 4.549 ms against 0.649 ms of execution, and all 76 hops start after the previous writer's execution end. 47 of 76 also happen to start after that writer's commit, because the preseeded holes in between take longer than the commit; the commit is no longer what the hop waits for. The small block's 35 armed parks likewise all start before the producer finishes.

K=10 scan of this binary, timers off, no warm-up, bootstrap 10,000. SEQ is one workers=1 baseline and is not repeated per C. OCC is `execute_revm_parallel`. Build is `lto=false`, codegen-units 1. C=8 is `--allow-oversub` onto CPUs 0–3.

```bash
scripts/soft0_percore_scan.sh --cpu-list 0-3 --allow-oversub --c-list 1,4,8 --k 10 --profile-k 1 --step-k 1 --out results/stage1c-scan-to
SPECFENCE_CLASS_KEY=code_hash scripts/soft0_percore_scan.sh --cpu-list 0-3 --allow-oversub --c-list 1,4,8 --k 10 --profile-k 1 --step-k 1 --skip-build --out results/stage1c-scan-code
```

`seqcheck` and `occcheck` reported `diverge=0` on both blocks for both output directories. They compare sequential with OCC. Ideal_C is `ideal_seq_ms`, the list-schedule makespan of the workers=1 profile. On 15274915 it equals the critical path at C=4 (1.163 ms and 1.173 ms), so it does not fall further at C=8.

#### `(to, selector)`

Block 15274915, `TPS_SEQ` 348231, median 3.521 ms, 95% CI [3.436, 3.710].

| C | OCC ms | OCC 95% CI | SF ms | SF 95% CI | Ideal_C ms | SF/OCC | SF/Ideal_C |
| ---: | ---: | --- | ---: | --- | ---: | ---: | ---: |
| 1 | 4.895 | [4.689, 5.500] | 5.580 | [5.447, 5.720] | 3.063 | 1.14 | 1.82 |
| 4 | 3.376 | [3.207, 3.685] | 6.049 | [5.850, 6.325] | 1.163 | 1.79 | 5.20 |
| 8 | 3.891 | [3.762, 4.073] | 7.093 | [6.755, 7.868] | 1.163 | 1.82 | 6.10 |

Block 3356896, `TPS_SEQ` 665722, median 0.264 ms, 95% CI [0.260, 0.273].

| C | OCC ms | OCC 95% CI | SF ms | SF 95% CI | Ideal_C ms | SF/OCC | SF/Ideal_C |
| ---: | ---: | --- | ---: | --- | ---: | ---: | ---: |
| 1 | 0.491 | [0.432, 0.528] | 0.517 | [0.506, 0.557] | 0.217 | 1.05 | 2.39 |
| 4 | 0.603 | [0.520, 0.650] | 1.189 | [1.086, 1.296] | 0.055 | 1.97 | 21.8 |
| 8 | 0.613 | [0.585, 0.634] | 1.264 | [1.184, 1.573] | 0.035 | 2.06 | 36.0 |

SF C=4 / SF C=1 is 6.049/5.580 = 1.08 on 15274915 and 1.189/0.517 = 2.30 on 3356896.

#### `(code_hash, selector)`

Block 15274915, `TPS_SEQ` 344890, median 3.555 ms, 95% CI [3.475, 3.727].

| C | OCC ms | OCC 95% CI | SF ms | SF 95% CI | Ideal_C ms | SF/OCC | SF/Ideal_C |
| ---: | ---: | --- | ---: | --- | ---: | ---: | ---: |
| 1 | 4.905 | [4.631, 5.448] | 5.690 | [5.419, 5.837] | 3.080 | 1.16 | 1.85 |
| 4 | 3.200 | [3.008, 3.805] | 6.645 | [6.325, 6.936] | 1.173 | 2.08 | 5.66 |
| 8 | 3.822 | [3.746, 4.119] | 7.214 | [7.014, 7.680] | 1.173 | 1.89 | 6.15 |

Block 3356896, `TPS_SEQ` 659488, median 0.267 ms, 95% CI [0.263, 0.279].

| C | OCC ms | OCC 95% CI | SF ms | SF 95% CI | Ideal_C ms | SF/OCC | SF/Ideal_C |
| ---: | ---: | --- | ---: | --- | ---: | ---: | ---: |
| 1 | 0.463 | [0.423, 0.519] | 0.512 | [0.506, 0.540] | 0.218 | 1.10 | 2.35 |
| 4 | 0.601 | [0.527, 0.685] | 1.057 | [1.036, 1.112] | 0.055 | 1.76 | 19.3 |
| 8 | 0.632 | [0.604, 0.937] | 1.284 | [1.246, 1.469] | 0.035 | 2.03 | 37.0 |

SF C=4 / SF C=1 is 6.645/5.690 = 1.17 on 15274915 and 1.057/0.512 = 2.07 on 3356896.

#### FullReplay, ten rounds

| Block | C | Class key | FullReplay | Max |
| --- | ---: | --- | --- | ---: |
| 15274915 | 4 | to+selector | 4, 5, 6, 6, 5, 3, 5, 5, 6, 5 | 6 |
| 15274915 | 8 | to+selector | 6, 6, 7, 7, 6, 6, 7, 3, 8, 7 | 8 |
| 15274915 | 4 | code_hash+selector | 6, 6, 6, 4, 5, 5, 5, 6, 7, 7 | 7 |
| 15274915 | 8 | code_hash+selector | 7, 6, 6, 7, 7, 4, 5, 6, 5, 6 | 7 |
| 3356896 | 4 | to+selector | 0 × 10 | 0 |
| 3356896 | 8 | to+selector | 0, 16, then 0 × 8 | 16 |
| 3356896 | 4 | code_hash+selector | 0 × 10 | 0 |
| 3356896 | 8 | code_hash+selector | 0 × 10 | 0 |

The round with 16 also re-executed 16 times (`chain_len` 17, `armed` 101, `exec_entries` 258, wall 1.778 ms). The other nine rounds at that cell are 0. C=1 is 0 on every round because that path does not arm the chain.

### Equivalence

Release tests on this tree, `lto=fat`, `--test-threads=1`:

```bash
cargo +stable test -p pevm --release --features specfence --test specfence_stage1 --test sload_static_gas -- --test-threads=1 sf_matches_onchain_focus_blocks sf_seq_par_repeat sload_static_gas_matches_chain_header
```

`sf_matches_onchain_focus_blocks` and `sf_seq_par_repeat` passed in the same process (3.75s). The first compares sequential, unmodified OCC, and SpecFence on both blocks at C=1, 4, and 8, for both class keys. The second repeats `seq=par` at C=4 and C=8. `sload_static_gas_matches_chain_header` passed (0.50s) and still matches header gas 29928443 and 4033966.

### Still open

- **The C=4 gate fails on every cell of this scan.** SF C=4 is 1.76× to 2.08× OCC C=4, and 1.08× to 2.30× SF C=1. SF/Ideal_C at C=4 is 5.20 and 5.66 on 15274915, and 21.8 and 19.3 on 3356896. At C=8 those ratios are 6.10, 6.15, 36.0, and 37.0.
- **The remaining critical path is the preseeded nearest-lower chain.** Finality removed the commit gate. It did not let the next recipient writer start before the previous writer, and the predicted members between them, have finished. On the post-fix timeline that span is 4.55 ms against 0.65 ms of writer execution.
- **Skipping that chain missed the FullReplay caps and did not cut the wall.** One fresh probe each, this host, timers off. Removing preseed: FullReplay 17 and 19 (`to+selector`, the two blocks) and 15 and 21 (`code_hash+selector`), walls still about 7.1 ms and 1.3 ms. Treating a not-started predicted member as absent: FullReplay 34 and 23, and with stealing also disabled 58 and 68. An in-flight execution window with stealing disabled either stopped the prefix (committed 80 of 1226) or finished slower (about 9.2 ms and 2.0 ms). Those runs were reverted. The chain still treats a predicted member as unresolved, and stealing stays on.
- **One 3356896 C=8 round exceeds the cap of 5.** FullReplay 16, against nine zeros in the same cell. The big-block maximum in this scan is 8, inside the cap of 12.
- **Multi-worker tax dominates the small block.** At C=1 the interpreter is a few tenths of a millisecond. Chain publish, the value-carrying record, thread spawn, and the preseed walk make C=4 slower than C=1 and slower than OCC.
- **`(code_hash, selector)` still pays a storage lookup while building classes.** Evidence-gated sibling prediction removed the thousand-park admission queue from the design. The K=10 wall is still above `(to, selector)` on the big block (6.645 ms versus 6.049 ms at C=4).

