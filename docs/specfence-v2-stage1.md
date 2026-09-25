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

The low band (FullReplay 7–14 on the big block) meets the ≤12 target. The high band does not, and it is the common outcome. Most of those aborts happen after the location is already armed (`full_replay_after_arm` is close to `full_replay`).

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

- **No opcode hook.** Predicted locations are marked running when the transaction starts. The concrete write set is published when the interpreter returns. A sibling that already started can read before that publish. That is why FullReplay is bimodal and why `full_replay_after_arm` stays high. Stage 6 (publish before the interpreter returns) is not in this stage.
- **Stage 1 scheduling only.** No persistent pool, no learned `C_eff`, no Ideal-ready pool, no late split, no IntraPatch, no QuietExit. The two ablations that need a persistent pool (OCC on that pool, SpecFence with the chain off) are later.
- **Seeding is strided index order**, not critical-path order. Contiguous chunks made the tail of the block run before the head had published; the stride keeps the first wave at transactions `0..C`.
- **Beneficiary writes are skipped** by the chain. They are still applied as lazy rewards in multi-version memory.
- **The online controller is a small AIMD tick**, not the full Part 2 concurrency controller. Safety bounds are the constants `K` in `[8, 256]`, abort-cost in `[5µs, 2ms]`, and class-hit floor 0.15. A plain-transfer class can be hundreds of transactions; its eviction priority is capped at 32 so it does not displace a location that has already failed validation.
- **`TPS_ideal` is the PR #62 critical-path schedule of SpecFence workers=1 profile attempts.** Native OCC has no per-transaction probe, and this stage does not record an opcode step trace, so `TPS_ideal_step` is absent. Beneficiary and lazy writes are dropped from the DAG, matching that report.
- **Nonce and balance waits use the previous same-sender transaction**, not `tx-1`. Blocking on `tx-1` after that unrelated transaction had committed spun the worker and stalled the commit prefix.

## Open issues

- FullReplay does not stay under the design caps (≤12 on 15274915, ≤5 on 3356896). The chain length does reach the carry-round spines (77 and 17) for `code_hash+selector`, but a reader can still commit a stale read of an armed location when the next writer has not published yet.
- `(to, selector)` on 15274915 is dominated by a 997-writer plain-transfer recipient. That key did not reduce FullReplay below the old fresh-run 50.
- Wall-clock comparison with the 128-core host is not meaningful here. C=8 oversubscribes four vCPUs.
