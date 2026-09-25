# SpecFence v2, stage 1

Stage 1 adds a SpecFence engine beside upstream Block-STM. The base is risechain/pevm `e94b0e3` (`ci: replace hand-rolled cache with Swatinem/rust-cache`), branch `specfence-v2`. SpecFence lives in `crates/pevm/src/specfence/` and is compiled only with `--features specfence`.

## Upstream OCC stays untouched

`vm.rs`, `mv_memory.rs`, `scheduler.rs`, and `pevm.rs` are byte-identical to `e94b0e3`. The only shared edits are:

- `Cargo.toml`: feature `specfence` and the `specfence_inflation_dig` example, which itself requires the feature.
- `lib.rs`: `#[cfg(feature = "specfence")] pub mod specfence;`

`cargo check -p pevm` without the feature does not compile the module. `Pevm::execute` and `execute_revm_parallel` do not call it. SpecFence owns its multi-version memory, its VM, and its scheduler. It reuses upstream types (`MemoryValue`, `MemoryEntry`, `PevmChain`) but does not patch them.

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

## Wall clock

Host: this VM, not ict21. 4 vCPUs, KVM, Intel Xeon model 207 family 6, one thread per core, CPUs 0–3. C=8 is pinned with `--oversub 8:0,1,2,3`, so it shares those four CPUs. Seven fresh rounds, no warm-up. Median and 95% bootstrap interval. `TPS_SEQ` is the C=1 sequential median, used as the single 1-core baseline at every worker count. `TPS_ideal(C)` is a list schedule of the traced per-transaction durations and RAW edges from the single trace run above (those durations include time spent waiting inside the interpreter).

| Block | C | Engine | Median ms | 95% CI | TPS | TPS_SEQ | TPS_ideal |
| --- | ---: | --- | ---: | --- | ---: | ---: | ---: |
| 3356896 | 1 | SEQ | 0.262 | 0.258–0.297 | 671103 | 671103 | |
| 3356896 | 1 | OCC | 0.501 | 0.425–0.640 | 351075 | 671103 | |
| 3356896 | 1 | SF to+selector | 0.617 | 0.582–0.646 | 285273 | 671103 | 484583 |
| 3356896 | 1 | SF code_hash+selector | 0.680 | 0.630–0.736 | 258946 | 671103 | 468726 |
| 3356896 | 4 | OCC | 0.526 | 0.512–0.588 | 334498 | 671103 | |
| 3356896 | 4 | SF to+selector | 1.342 | 1.062–1.463 | 131167 | 671103 | 360908 |
| 3356896 | 4 | SF code_hash+selector | 1.423 | 1.383–1.523 | 123717 | 671103 | 707637 |
| 3356896 | 8 | OCC | 0.740 | 0.637–0.928 | 237949 | 671103 | |
| 3356896 | 8 | SF to+selector | 1.388 | 1.365–1.578 | 126845 | 671103 | 484068 |
| 3356896 | 8 | SF code_hash+selector | 1.605 | 1.413–1.615 | 109645 | 671103 | 1194686 |
| 15274915 | 1 | SEQ | 3.580 | 3.504–3.860 | 342492 | 342492 | |
| 15274915 | 1 | OCC | 4.904 | 4.559–5.332 | 250019 | 342492 | |
| 15274915 | 1 | SF to+selector | 6.454 | 6.046–8.165 | 189947 | 342492 | 252787 |
| 15274915 | 1 | SF code_hash+selector | 7.637 | 7.014–8.880 | 160525 | 342492 | 208320 |
| 15274915 | 4 | OCC | 3.413 | 3.252–4.041 | 359186 | 342492 | |
| 15274915 | 4 | SF to+selector | 7.077 | 5.675–8.014 | 173236 | 342492 | 437046 |
| 15274915 | 4 | SF code_hash+selector | 8.614 | 8.511–10.171 | 142321 | 342492 | 400401 |
| 15274915 | 8 | OCC | 3.837 | 3.752–5.485 | 319551 | 342492 | |
| 15274915 | 8 | SF to+selector | 7.322 | 6.546–7.837 | 167447 | 342492 | 577204 |
| 15274915 | 8 | SF code_hash+selector | 8.377 | 7.843–10.185 | 146349 | 342492 | 546073 |

On this 4-core VM, unmodified OCC at C=4 is the first point that beats sequential on the big block. SpecFence is slower than both. C=8 does not add CPUs.

## Deviations from the design

- **No opcode hook.** Predicted locations are marked running when the transaction starts. The concrete write set is published when the interpreter returns. A sibling that already started can read before that publish. That is why FullReplay is bimodal and why `full_replay_after_arm` stays high. Stage 6 (publish before the interpreter returns) is not in this stage.
- **Stage 1 scheduling only.** No persistent pool, no learned `C_eff`, no Ideal-ready pool, no late split, no IntraPatch, no QuietExit. The two ablations that need a persistent pool (OCC on that pool, SpecFence with the chain off) are later.
- **Seeding is strided index order**, not critical-path order. Contiguous chunks made the tail of the block run before the head had published; the stride keeps the first wave at transactions `0..C`.
- **Beneficiary writes are skipped** by the chain. They are still applied as lazy rewards in multi-version memory.
- **The online controller is a small AIMD tick**, not the full Part 2 concurrency controller. Safety bounds are the constants `K` in `[8, 256]`, abort-cost in `[5µs, 2ms]`, and class-hit floor 0.15. A plain-transfer class can be hundreds of transactions; its eviction priority is capped at 32 so it does not displace a location that has already failed validation.
- **`TPS_ideal` is transaction-level**, from `scripts/specfence_step_ideal.py`. The reference script's opcode-step schedule needs a step trace this stage does not record.
- **Nonce and balance waits use the previous same-sender transaction**, not `tx-1`. Blocking on `tx-1` after that unrelated transaction had committed spun the worker and stalled the commit prefix.

## Open issues

- FullReplay does not stay under the design caps (≤12 on 15274915, ≤5 on 3356896). The chain length does reach the carry-round spines (77 and 17) for `code_hash+selector`, but a reader can still commit a stale read of an armed location when the next writer has not published yet.
- `(to, selector)` on 15274915 is dominated by a 997-writer plain-transfer recipient. That key did not reduce FullReplay below the old fresh-run 50.
- Wall-clock comparison with the 128-core host is not meaningful here. C=8 oversubscribes four vCPUs.
