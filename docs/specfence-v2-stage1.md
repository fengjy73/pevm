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
