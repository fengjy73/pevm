# SpecFence v2, stage 2c

Draft PR: https://github.com/fengjy73/pevm/pull/73

Stage 2c starts from stage 2b (`cursor/specfence-v2-stage2b-2c54`, `4352911`, PR #72). SpecFence stays in `crates/pevm/src/specfence/` and compiles only with `--features specfence`. Upstream `vm.rs`, `mv_memory.rs`, `scheduler.rs`, and `pevm.rs` are byte-identical to that commit. Opcode `static_gas()` is unchanged. Hooks run only on the SpecFence path.

ict21 stage 2b (CPUs 128–255, fat LTO, system malloc, K=10, no warm-up) is the problem. On block 15274915, `(to, selector)`, SF was 5.11 ms at C=1 and 7.25 / 6.33 / 9.92 / 11.47 ms at C=4/8/16/32, against OCC 2.86 ms at C=4. Interpreter thread time was 3.13 ms on the fast path and 9.84 / 12.9 / 13.2 ms at C=4/8/16. Plain transfers grew 0.50 → 1.48 ms. Other contracts grew 2.72 → 10.5 ms. A traced park of tx 101 on a predecessor lasted 7.7 ms at C=8 and 8.7 ms at C=16. The active set dropped to 1 mid-block. Sched thread time at C=16 was 16.3 ms. Forced parallel at one worker was only about 1.3× the fast path after re-execution, so the rest of the growth is cross-core.

## What changed

- **No interpreter spin.** A read whose predecessor was executing used to sleep on the scheduler condvar, 40 times, 50 µs each, inside `handler.run`. That sleep is interpreter thread time, and a reader only entered it when another core was running the predecessor, so it grew with C. The reader now returns `Blocking` and the scheduler parks it on the executing transaction.
- **A parked reader does not wait on a predecessor that is not executing.** `claim_blocker` walks `Parked` links. A `Ready` or `Sticky` root is marked `Executing` and run by the waiting worker. Only an `Executing` transaction is a `Wait`. `park` retargets dependents of a task that just stopped executing onto that root. An `Executed` transaction that is not final is sealed by the reader (`Claim::Seal`) instead of parked. `boost` still exists and still returns without pushing when the phase is not `Ready`; the claim path is what runs the predecessor.
- **Active set floor is ready plus executing.** The old input was an exponential moving average of the ready queue. Tasks leave that queue when they start, the average decays, and two decisions ratcheted the set to 1 while other workers were inside the interpreter. The floor is the live ready count plus the in-flight count. One decision grows to the floor. Shrink waits for two decisions, then drops to the floor, not by one each time. A chain hop is not an input.
- **Stale deque copies skip the scheduler mutex.** `pop` reads the phase atomic and discards a non-`Ready` entry before locking. `is_executing` reads the same atomic. `SCHED_SKIPS` counts the discards when buckets are on.
- **Shared bytecode and base state, both optional.** `SPECFENCE_SHARED_CODE=0` keeps the per-worker code cache. Otherwise each code hash is analyzed once per block and the revm `Bytecode` (`Arc`) is shared. `SPECFENCE_SHARED_CACHE=0` keeps the per-worker account and slot maps. Otherwise one read-mostly cache, 32 shards, is prefilled with the beneficiary, every caller, and every callee. Both default on.
- **One process allocator for both engines.** `--allocator mimalloc` or `SPECFENCE_ALLOCATOR=mimalloc` selects mimalloc. The choice is read from `/proc` with no heap traffic, because it runs inside `alloc`. Default remains system malloc.
- **In-block writer of an account touched inside a contract.** A directory insert or replacement bumps a generation. The worker-local slot cache used to key only on epoch and occupancy, so a full table that replaced a slot looked like the same cache and a miss hid the new chain. The abort's writer is inserted as `Predicted`, not only its classmates.

## Experiments

Block 15274915, `(to, selector)`, C=4, K=10, `SPECFENCE_BUCKETS=1`, CPUs 0–3, release, `lto=false`. Buckets call `Instant::now`, so these wall times are not the clean-scan wall times below. Interpreter, `class_*`, `mv_record`, `sched`, and `read_*` are median thread time. `read_cold` is a count.

Fast path, same flags, C=1: interpreter 2.964 ms / 1226, plain 0.660 / 1046, other 2.369 / 180, re-execution 0, wall 5.653 ms.

### (a) Allocator

Shared code and shared cache on. OCC and SF in the same process.

| Allocator | SF interpreter | SF wall | OCC wall | read_origin | sched |
| --- | ---: | ---: | ---: | ---: | ---: |
| system | 6.600 ms | 8.822 ms | 3.355 ms | 0.457 ms | 0.782 ms |
| mimalloc | 6.837 ms | 9.158 ms | 4.184 ms | 0.312 ms | 0.762 ms |

mimalloc is slower for both engines (SF interpreter +0.24 ms, SF wall +0.34 ms, OCC wall +0.83 ms). The flag stays. The default stays system malloc, and every comparison below uses it.

### (b) Shared bytecode

System malloc. Cache on for both rows.

| Code | interpreter | wall | read_code | class_other |
| --- | ---: | ---: | ---: | ---: |
| per worker | 6.417 ms | 8.827 ms | 0.112 ms | 4.877 ms |
| shared | 6.600 ms | 8.822 ms | 0.074 ms | 4.926 ms |

Shared code lowers the code fetch by 0.038 ms. Interpreter and wall do not move outside the spread of these ten rounds.

### (c) Shared base-state cache

System malloc. Code on for both rows. The both-off row is the stage 2b shape (per-worker code and per-worker cache).

| Cache | interpreter | wall | read_base | read_base calls | read_cold count |
| --- | ---: | ---: | ---: | ---: | ---: |
| per worker, code per worker | 6.735 ms | 8.417 ms | 0.180 ms | 1131 | 2643 |
| per worker, code shared | 6.663 ms | 8.578 ms | 0.184 ms | 1131 | 2619 |
| shared, code per worker | 6.417 ms | 8.827 ms | 0.142 ms | 595 | 2616 |
| shared, code shared | 6.600 ms | 8.822 ms | 0.140 ms | 600 | 2631 |

Cold reads stay about 2.6k at C=4, the same flat count as C=1 forced-parallel (2.7k). The shared cache halves base fetches (1131 → 600) and cuts `read_base` by about 0.04 ms. That is inside the interpreter number, and the interpreter median does not fall. Wall is not better. `read_cold` is the bloom skip, not a cache miss, so it does not move when the cache is shared.

### (d) Top 10 by interpreter inflation

`SPECFENCE_INFLATION=1`, one fast C=1 run against one C=4 run, sum of interpreter nanoseconds. Ratio is parallel / fast. This clock is not the bucket clock.

Before removing the spin, the sum was 3.609 ms fast and 6.946 ms parallel (1.93×). The top row was tx 78, USDT `transferFrom` (`0xdAC17F958D2ee523a2206206994597C13D831ec7`, selector `0x23b872dd`): 11.0 µs → 177.7 µs, 16.2×, one attempt, 12 reads, 4 writes. Tx 96, selector `0x18cbafe5`, was 27.3 µs → 196.5 µs, two attempts. Those are one or two 50 µs sleeps inside the interpreter while a predecessor executed on another core. Forced parallel at C=1 does not take that sleep, which is why it stayed near 1.3×.

After removing the spin, the sum was 3.598 ms fast and 5.610 ms parallel (1.56×), wall 8.816 ms and 11.082 ms. The USDT row left the top 10. What remains is mostly plain transfers whose absolute gap is a few microseconds, plus one USDC transfer:

| tx | to | selector | fast µs | parallel µs | ratio | attempts | reads | writes | first write |
| ---: | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 727 | `0x7758E507850dA48cd47df1fB5F875c23E3340c50` | transfer | 1.8 | 17.0 | 9.56 | 1 | 4 | 3 | `0xc7472a65314682ab` |
| 7 | `0x5418f8413856d100aDe94cc54c543ca0c0cE5a48` | transfer | 2.3 | 18.6 | 7.96 | 1 | 4 | 3 | `0xcf2ebd5bb9e12fdf` |
| 56 | `0x4ECF8850f8eE33e887e1a43d26707EE5fa1f1471` | transfer | 1.1 | 6.5 | 6.10 | 1 | 4 | 3 | `0x755ad1f8affcb733` |
| 1096 | `0x6262998Ced04146fA42253a5C0AF90CA02dfd2A3` | transfer | 0.6 | 2.9 | 4.70 | 1 | 1 | 3 | `0x7ec8be01af547316` |
| 54 | `0xE5657E9DB7330138D75F568F48934c2440039801` | transfer | 1.1 | 5.2 | 4.59 | 1 | 4 | 3 | `0xe5f04107525d854b` |
| 648 | `0x7758E507850dA48cd47df1fB5F875c23E3340c50` | transfer | 1.7 | 7.5 | 4.44 | 1 | 4 | 3 | `0xabd6bb3978815b97` |
| 1060 | `0x7758E507850dA48cd47df1fB5F875c23E3340c50` | transfer | 1.9 | 8.0 | 4.25 | 1 | 4 | 3 | `0x15fd188bf8707721` |
| 11 | `0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48` | `0xa9059cbb` | 10.2 | 39.9 | 3.93 | 1 | 13 | 4 | `0x35700bf9b3dcdbdf` |
| 62 | `0x828EfAa43A040a5aA191277a435b4AB1BB2774FE` | transfer | 1.1 | 4.2 | 3.86 | 1 | 4 | 3 | `0x24f82b4fe3b518ca` |
| 6 | `0xf25E10cDdA8E1b4A6f3b8E1eAD45C223f5538e57` | transfer | 1.6 | 5.8 | 3.72 | 1 | 4 | 3 | `0x2ae8c664833d9bff` |

Tx 648's first write is the hot chain `0xabd6bb3978815b97` (the contract `0x7758…0c50`). Tx 1096's first write is the plain-transfer chain `0x7ec8be01af547316`. Tx 11 is USDC `transfer`. The 4-read / 3-write transfers touch the sender, the recipient, and a lazy beneficiary write. Their ratio is high because the fast path is about 1 µs; the absolute gap is under 20 µs.

Bucket split after the spin removal, median of K=10, still with buckets on:

| Bucket | Fast C=1 | C=4 | C=8 |
| --- | ---: | ---: | ---: |
| interpreter | 2.964 ms | 6.146 ms | 6.290 ms |
| plain | 0.660 ms | 0.988 ms | 1.095 ms |
| other | 2.369 ms | 4.503 ms | 4.477 ms |
| re-execution | 0 | 1.262 ms | 0.919 ms |
| coordinate | 0 | 0.062 ms | 0.061 ms |
| mv_record | 0.552 ms | 2.120 ms | 5.618 ms |
| sched | 0 | 0.819 ms | 0.702 ms |

C=4 interpreter / fast = 2.07×. Coordinate fell from about 1.0 ms, when the spin was still inside the read, to 0.06 ms. `mv_record` at C=8 is 5.6 ms on four CPUs and is the bucket that still grows with C. Re-execution is inside the interpreter total.

## What was adopted

- The interpreter spin is gone. Rank sum ratio 1.93× → 1.56×, and the 16× USDT row disappeared. Coordinate thread time 1.0 ms → 0.06 ms.
- Predecessor claim, retarget, and seal-instead-of-park. Tests: `reader_parks_on_the_executing_root_of_a_parked_chain`, `executed_predecessor_is_sealed_instead_of_parked`, `claim_runs_a_predecessor_that_is_not_executing`, `claim_follows_a_parked_predecessor_to_a_ready_root`, `parking_retargets_a_reader_onto_the_running_root`.
- Active-set floor and the phase atomic in `pop`. Test: `executing_count_keeps_the_active_set`.
- Directory generation and inserting the abort's own writer. Test: `replaced_chain_is_visible_after_a_cached_miss`.
- Allocator flag, default system. mimalloc lost on both engines.
- Shared code and shared cache stay on. The controlled bucketed comparison did not show an interpreter or wall win. It did cut base fetches from 1131 to 600 and code fetch from 0.112 ms to 0.074 ms. `SPECFENCE_SHARED_CODE=0` and `SPECFENCE_SHARED_CACHE=0` are the off legs for the ict21 rerun.

## Parked predecessor

On ict21, tx 101 parked for 7.7 ms at C=8 and 8.7 ms at C=16 on location `0x3002e6fbbca936f8` (reason block), and for 1.3 ms at C=4 on `0x111015025e6393bd`. That is about 40% of the traced wall at the higher widths.

The same shape is on this VM with the stage 2b binary. A timeline of 15274915 at C=4 showed tx 102 parked on tx 101 (estimate, `0x111015025e6393bd`) while tx 101 was parked on tx 94 (armed, `0xbef034365ca24581`) and tx 94 was parked on tx 92 (admission). `boost` pushes only when the predecessor's phase is `Ready`. A parked predecessor is a no-op, so the reader waits for a task that is not in the interpreter. The wait is transitive: 102 waits for 101, who is waiting for 94, who is waiting for 92. The worker that could have run 92 is either busy or asleep, and a bottom-boosted copy is the last thing a thief steals.

After the claim change, a traced C=4 run (timeline on, so the block is slower than the clean scan) has 10 parks, longest 569 µs: tx 4 on tx 3, reason estimate, location `0x8ebeaa932b6abe9f`. That interval ends before tx 3's interpreter span starts, so it is not the old "wait for a parked chain" shape; tx 3's phase was already `Executing` from the pop, and the timeline span starts at `vm.execute`. The next parks are 330 µs, 274 µs, 270 µs. A traced C=8 run has 3 parks, longest 475 µs (tx 4 on tx 3 again). Tx 101 no longer has a multi-millisecond park. In an earlier traced C=4 run its waits were a 224 µs armed park on `0xbef034365ca24581` and an 89 µs class wait.

## Controller

The mid-block drop to 1 was the ready-queue average. Once tasks are `Executing` or `Parked`, ready depth is ~0, the average decays, and two decisions move the set to 1. Workers with index ≥ active then sleep in `wait_inactive`. New ready work is left to the one worker still polling. `learned_active` is the last sample, so it reads 1 even when the block ran wide.

The 16.3 ms sched bucket at C=16 was not the condvar. `wait_work` sits outside the sched bucket. The bucket wraps `take_sticky` and `pop`. `pop` used to take the scheduler mutex for every stale deque copy, and boost plus steals leave duplicate entries. This VM records about 1170 skips per C=4 block. Skipping them under the lock is enough, at a few microseconds per lock, to land in the 10 ms range once C=16 multiplies the steal loop. The phase atomic drops those skips before the lock. With buckets on, sched median is 0.82 ms at C=4 and 0.70 ms at C=8.

After the floor change, C=4 active samples (buckets on, K=10) are 4 for most of the block. 46 of 495 samples are 1, and those 1s are the tail: first 1 appears at sample 23–53 of 34–60, and two rounds never leave 4. C=8 stays at 8 until the last few samples in the rounds that do drop. The tail is one in-flight chain (`ready + executing` is 1), not the old collapse while C tasks were inside the interpreter. `learned_active` is still often 1 because it is that last sample. Peak stays at C.

## Clean scan

4 vCPUs, CPUs 0–3, release, `lto=false`, system malloc, shared code and shared cache on, no buckets, no timeline, no warm-up, fresh engine each round, K=10. Median and bootstrap 95% CI, 10,000 resamples, seed 0. SEQ is timed on the C=1 rounds only. `delta_mismatch` is 0 and `ok` is true in every cell. C=8 is eight workers on four CPUs.

One `(code_hash, selector)` C=4 round on 15274915 was 211.7 ms, the 200 ms idle timeout. The other nine rounds of that cell are 7.6–10.5 ms. The median below does not include that round as the middle value. The other five cells on this block stay under 16 ms.

### `(to, selector)`

Block 15274915. SEQ 5.618 ms [5.224, 6.865].

| C | OCC ms | OCC 95% CI | SF ms | SF 95% CI | reexec | full replay |
| ---: | ---: | --- | ---: | --- | --- | --- |
| 1 | 8.297 | [8.119, 10.343] | 6.950 | [6.379, 9.990] | 0 | 0 |
| 4 | 4.102 | [3.953, 4.970] | 9.630 | [9.183, 12.983] | 12–38 | 6–16 |
| 8 | 4.622 | [4.320, 5.158] | 9.437 | [8.528, 10.931] | 5–19 | 3–9 |

Block 3356896. SEQ 0.534 ms [0.501, 0.572].

| C | OCC ms | OCC 95% CI | SF ms | SF 95% CI | reexec | full replay |
| ---: | ---: | --- | ---: | --- | --- | --- |
| 1 | 0.957 | [0.832, 1.168] | 0.584 | [0.516, 0.601] | 0 | 0 |
| 4 | 0.649 | [0.633, 0.791] | 1.626 | [1.457, 1.709] | 0 | 0 |
| 8 | 0.836 | [0.764, 0.930] | 1.721 | [1.498, 1.993] | 0 | 0 |

### `(code_hash, selector)`

Block 15274915. SEQ 4.665 ms [4.581, 4.943].

| C | OCC ms | OCC 95% CI | SF ms | SF 95% CI | reexec | full replay |
| ---: | ---: | --- | ---: | --- | --- | --- |
| 1 | 6.786 | [6.450, 8.819] | 5.769 | [5.307, 6.072] | 0 | 0 |
| 4 | 3.474 | [3.365, 4.241] | 8.615 | [7.699, 10.509] | 13–22 | 8–9 |
| 8 | 4.505 | [4.239, 5.011] | 9.799 | [8.758, 10.306] | 6–14 | 4–9 |

Block 3356896. SEQ 0.541 ms [0.525, 0.599].

| C | OCC ms | OCC 95% CI | SF ms | SF 95% CI | reexec | full replay |
| ---: | ---: | --- | ---: | --- | --- | --- |
| 1 | 0.927 | [0.865, 1.092] | 0.621 | [0.579, 0.688] | 0 | 0 |
| 4 | 0.541 | [0.530, 0.631] | 1.434 | [1.344, 1.716] | 0 | 0 |
| 8 | 0.785 | [0.745, 0.845] | 1.508 | [1.443, 2.135] | 0–52 | 0–7 |

## Gates

Measured here. ict21 C=1, 4, 8, 16, 32 is the confirmation run. C=16 and C=32 were not scanned: this host has four CPUs.

| Gate | This VM |
| --- | --- |
| 15274915 `(to, selector)`: SF(4) < SF(1) | 9.630 vs 6.950. Not met. |
| SF(8) ≤ SF(4) × 1.1, same key | 9.437 vs 10.593. Met. |
| Same, `(code_hash, selector)` | 9.799 vs 9.477. Not met. |
| C=4 interpreter ≤ 1.5× fast path | 6.146 / 2.964 = 2.07×. Not met. The spin was the 16× rows; what remains is about 1.5× on plain transfers, re-execution, and `mv_record`. |
| SF C=4 ≤ OCC C=4 on 15274915 | 9.630 vs 4.102. Not met. |
| No traced park longer than 0.5 ms | C=8 longest 475 µs. C=4 longest 569 µs (one estimate park). Not met on that one span. The 7–9 ms tx 101 park is gone. |
| specfence tests, focus blocks, seq/par repeat, SLOAD static gas | 37 `specfence::` tests, `sf_matches_onchain_focus_blocks`, `sf_seq_par_repeat`, `sload_static_gas_matches_chain_header`. Met. |
| SF equals SEQ at every C | `delta_mismatch` is 0 in every cell above. `sf_seq_par_repeat` matches sequential execution. Met on the cells this host ran. |

## Reproduce

Pinned host, fat LTO, system malloc unless `--allocator` is set. No warm-up. K=10. SEQ is the C=1 rounds only.

```bash
scripts/soft0_percore_scan.sh --cpu-list 128-255 --c-list 1,4,8,16,32 --k 10 --profile-k 1
SPECFENCE_CLASS_KEY=code_hash scripts/soft0_percore_scan.sh \
  --cpu-list 128-255 --c-list 1,4,8,16,32 --k 10 --profile-k 1 --skip-build
```

Allocator, both engines, same process. Repeat with `--allocator mimalloc`.

```bash
SPECFENCE_BUCKETS=1 SPECFENCE_CLASS_KEY=to SPECFENCE_INFLATION_WHICH=scan \
  SPECFENCE_INFLATION_BLOCKS=15274915 SPECFENCE_INFLATION_K=10 \
  SPECFENCE_INFLATION_ENGINES=sf,occ \
  taskset -c 128-255 target/release/examples/specfence_inflation_dig \
  --workers 4 --cpu-list 128-255 --allocator system
```

Shared code and shared cache off legs:

```bash
SPECFENCE_SHARED_CODE=0 SPECFENCE_SHARED_CACHE=0 \
  SPECFENCE_BUCKETS=1 SPECFENCE_CLASS_KEY=to SPECFENCE_INFLATION_WHICH=scan \
  SPECFENCE_INFLATION_BLOCKS=15274915 SPECFENCE_INFLATION_K=10 \
  SPECFENCE_INFLATION_ENGINES=sf \
  taskset -c 128-255 target/release/examples/specfence_inflation_dig \
  --workers 4 --cpu-list 128-255
```

Top 10 inflation. Do not set `SPECFENCE_FORCE_PARALLEL` in this process.

```bash
SPECFENCE_INFLATION=1 SPECFENCE_INFLATION_WHICH=rank \
  SPECFENCE_INFLATION_BLOCKS=15274915 SPECFENCE_CLASS_KEY=to \
  taskset -c 128-255 target/release/examples/specfence_inflation_dig \
  --workers 4 --cpu-list 128-255
```

Traced parks:

```bash
SPECFENCE_TIMELINE=1 SPECFENCE_TIMELINE_OUT=/tmp/sf-c4.jsonl \
  SPECFENCE_INFLATION_WHICH=scan SPECFENCE_INFLATION_BLOCKS=15274915 \
  SPECFENCE_CLASS_KEY=to SPECFENCE_INFLATION_K=1 SPECFENCE_INFLATION_ENGINES=sf \
  taskset -c 128-255 target/release/examples/specfence_inflation_dig \
  --workers 4 --cpu-list 128-255
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
