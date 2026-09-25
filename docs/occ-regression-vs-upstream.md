# OCC baseline is upstream pevm at e94b0e3

The fork's Block-STM path is slower than risechain/pevm `e94b0e3`. This change does not try to make the fork OCC path match that speed. The scan's `TPS_OCC` is unmodified upstream. `TPS_SF` stays on this fork. `TPS_SEQ` is one 1-core sequential measurement, not a column repeated at every concurrency.

## How upstream is wired

`crates/pevm_upstream` is the library from https://github.com/risechain/pevm at `e94b0e3df9c0c1b983ed3e4d78b3aefb0b5b7cc0`.

- `src/` is byte-identical to that commit (`diff -rq` against `git archive`).
- The Cargo package name is `pevm_upstream` so it links next to this workspace's `pevm`.
- `benches/mainnet.rs` is the only logic change: `PEVM_BENCH_CONCURRENCY` overrides the criterion concurrency, and the import is `pevm_upstream`. `tests/common` imports the same package name so that bench builds.
- `benches/gigagas.rs` is the upstream file and is not a Cargo target. It pulls `tests/erc20` and `tests/uniswap`, which are not vendored.

The harness example `specfence_inflation_dig` depends on `pevm_upstream`. For a wall sample (`SPECFENCE_INFLATION` and `SPECFENCE_STEP_TRACE` unset) the `occ` row calls `pevm_upstream::Pevm::execute_revm_parallel` after the tx list and block env are built. The `Instant` wraps that call, same as `execute_revm_sequential` and the fork's SpecFence `execute_revm_parallel`. Each round builds a new `Pevm`. There is no untimed warm-up.

Probe modes still execute the fork's Occ path. Upstream has no `ExecPhase` or step-trace hooks, so `TPS_ideal` per-tx samples stay on the fork probe. Those rows are not `TPS_OCC`. The wall `path` field for occ is `pevm_upstream@e94b0e3 execute_revm_parallel`.

`which=occcheck` compares upstream `execute_revm_sequential` with upstream `execute_revm_parallel` on one block (receipts, gas, logs, state via `PartialEq`).

## What the scan reports

`scripts/soft0_percore_scan.sh` pins with `taskset -c` to `PEVM_CPU_LIST` or `--cpu-list`. The default list is `128-255` (ict21 node1). A smaller machine passes its own list. `--allow-oversub` runs a C larger than the list by pinning the process to the whole list.

Sequential rows are emitted only at `workers == 1`. The curve table copies that `TPS_SEQ` onto every C instead of re-timing sequential. Per-C columns are `TPS_OCC` (upstream), `TPS_SF` (fork), and `TPS_ideal`.

## Reproduce

```bash
# ict21 node1, default CPU list 128-255
scripts/soft0_percore_scan.sh --c-list 1,4,8 --k 10

# this 4-core VM
scripts/soft0_percore_scan.sh --cpu-list 0-3 --allow-oversub \
  --c-list 1,4,8 --k 10 --profile-k 1 --step-k 1
```

The script builds `target/release/examples/specfence_inflation_dig` with release `codegen-units=1` and `lto=false` (the scan's existing profile, not the criterion fat-LTO bench). Upstream criterion, if you want it separately:

```bash
PEVM_BENCH_CONCURRENCY=4 cargo +stable bench -p pevm_upstream --bench mainnet --features global-alloc -- --sample-size 10
```

## Known gap (ict21, not re-measured here)

ict21, 2x EPYC 9754, criterion `mainnet`, `global-alloc` (rpmalloc), fat LTO, `codegen-units=1`, K=10, no warm-up. Sequential matched. Fork parallel did not.

| block | C | upstream ms | fork OCC ms |
| --- | ---: | ---: | ---: |
| 15274915 | 1 | 4.993 | 7.255 |
| 15274915 | 4 | 2.768 | 3.931 |
| 15274915 | 8 | 2.239 | 3.177 |
| 3356896 | 1 | 0.450 | 0.776 |
| 3356896 | 4 | 0.378 | 0.566 |

The extra is about 1.85 µs/tx on both blocks (2.26 ms / 1226 txs and 0.33 ms / 176 txs) and is already visible at C=1. Allocator, LTO, and fresh-vs-reused `Pevm` were ruled out on that host. This document does not assign a percentage of that gap to each hunk. The scope here is the baseline, not a fix.

## Hunks that also sit on the SpecFence execution path

SpecFence does not call `next_occ_task`. It does call `Vm::execute` and `chain.run_pevm_tx` on this fork. These fork-vs-upstream differences are on that path, so they are paid by `TPS_SF` as well as by the fork's own OCC:

- `Vm::execute` always starts `ExecPhase` (`Instant::now` plus metric atomics) before the interpreter.
- `NoBeneficiaryHandler::run_exec_loop` calls `note_frame_depth` on every opcode. Upstream's handler only skipped the beneficiary reward.
- `VmDb::basic` clones the multi-version history for the location (`range(..tx_idx)` then `v.clone()`) before walking it. Upstream walked the map in place.
- `MvMemory::record` updates a reader index (`DashMap` plus `BTreeSet`) on every read location. Upstream swapped the read set and wrote the data.
- `execute_revm_parallel` builds the SpecFence tables (`AccountHints::build`, `PartialRetryTable::new(block_size)`, `RunnableSet`, access spine, ordinal log, certificates) before it branches to `run_sf_block`. SpecFence uses those tables. The fork's OCC path built them too; upstream OCC does not.

Not on the SpecFence path: the OCC worker's `next_occ_task` (`OCC_PICK_CALLS` plus the fat `next_task`) and the `Instant::now` around every OCC pick and validation in the non-SpecFence worker loop.

## Sanity run

Filled after the local scan. See the host line in that section.
