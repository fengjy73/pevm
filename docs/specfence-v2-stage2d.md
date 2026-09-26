# SpecFence v2, stage 2d

Stage 2d starts from stage 2c (`cursor/specfence-v2-stage2c-2a99`, `c820ed1`, PR #73). SpecFence stays in `crates/pevm/src/specfence/` and compiles only with `--features specfence`. Upstream `vm.rs`, `mv_memory.rs`, `scheduler.rs`, and `pevm.rs` are byte-identical to that commit. Opcode `static_gas()` is unchanged. Hooks run only on the SpecFence path.

**The early commit hole was the prefix sitting `Ready` while every worker ran a later transaction, and a wake that could leave the other workers asleep.** On this VM a stage 2c trace of block 15274915 at C=4 left 4.722 ms between the commit of tx 11 and tx 12. Tx 12 then executed for 7 µs. Claiming that prefix before any other pop removes every commit gap above 0.5 ms in the rebuilt trace. The same change wakes every sleeper on publish, and a park times out after 500 µs if that wake is missed.

**The default harness no longer installs a global allocator.** A runtime branch inside `alloc` had slowed every engine, sequential execution included. On this host, one CPU, K=10, the PR 72 sequential median is 3.773 ms and the new binary is 3.829 ms (1.015×). The stage 2c wrapper on the same command is 4.820 ms.

ict21 stage 2c (CPUs 128–255, fat LTO, K=10, no warm-up) did not move: 15274915 `(to, selector)` SF was 5.81 / 7.60 / 8.10 / 10.36 / 12.11 ms at C=1/4/8/16/32 against OCC 6.73 / 3.21 / 2.43 / 3.52 / 4.38, SEQ 4.40, Ideal_C 1.48 from C=4. SF matched SEQ at every C. This host has four CPUs, so C=16 and C=32 are not in the tables below.

## Allocator

The example used to install `PickedAlloc` as `#[global_allocator]`. Every allocation loaded an atomic and branched to system malloc or mimalloc, including the sequential engine. Removing only that wrapper put sequential execution back; pointing `#[global_allocator]` straight at mimalloc was 4.44 ms on ict21, still not the unwrapped binary.

The default binary now has no `#[global_allocator]`. `specfence-mimalloc` installs `mimalloc::MiMalloc` directly. `--allocator mimalloc` or `SPECFENCE_ALLOCATOR=mimalloc` on the default binary exits 2 and names the feature. There is no `/proc` read on the allocation path.

Same host, CPU 0, release, `lto=false`, `codegen-units=16`, block 15274915, `SPECFENCE_INFLATION_ENGINES=seq`, K=10, no warm-up. Median is the sorted sample at index K/2.

| Binary | Median | Samples, sorted |
| --- | ---: | --- |
| PR 72 (`4352911`, no wrapper) | 3.773 ms | 3.687, 3.737, 3.752, 3.754, 3.767, 3.773, 3.802, 3.823, 3.901, 4.323 |
| Stage 2c wrapper | 4.820 ms | 4.754, 4.756, 4.769, 4.793, 4.809, 4.820, 4.821, 4.871, 4.935, 5.380 |
| Stage 2d default | 3.829 ms | 3.737, 3.803, 3.815, 3.820, 3.822, 3.829, 3.848, 3.882, 3.898, 4.284 |

3.829 / 3.773 = 1.015. The gate is 5%. The wrapper is 1.28× the PR 72 binary.

## Commit-frontier gap

### What the stage 2c trace did

Block 15274915, C=4, CPUs 0–3, timeline on, stage 2c binary. The engine clock in the dump is 10.11 ms. One commit gap is above 0.5 ms: **4.722 ms, tx 11 committed at 3.133 ms, tx 12 committed at 7.856 ms.** Tx 12 is class 65535, has no chain members and no reads, three writes, and its only execution span is 6.9 µs on worker 3 starting at 7.846 ms. It was `Ready` the whole hole.

During that window the four workers were busy (about 2.5–2.9 ms each). Idle time inside the window is under 0.01 ms. The work they ran is later than the prefix: tx 49, 59, 91, 101, 102, and the armed parks on `0xbef034365ca24581`. No idle span in the whole trace is above 1 ms. On this host the hole is not a parked worker. It is the scheduler's order.

Index seeding pushes `w, w+C, …` high to low, so the owner's LIFO pop takes the low index and a thief takes the high index. `rescue` runs only after `pop` returns empty. While any deque is non-empty, a `Ready` prefix buried under later work is never preferred. Tx 12 waited until a worker's pop finally reached it.

ict21 adds a second shape the same code can produce. Every traced run there had a 7.8–9.3 ms commit hole early in the block (between tx 11–12, 25–26, 31–32, or 39–40) and worker idle spans up to 9.1 ms. That matches a lost wake:

- `poke_work` used `notify_one`. The worker that woke could run a long non-prefix transaction. The others stayed on the condvar.
- `park` could enqueue a predecessor and return without a wake.
- `seed` left `active` at 1, so workers 1..C slept on `sleep_cv` until the first completion grew the set.
- The condvar timeout was 200 ms, so a missed wake did not correct itself inside the block.

### What changed

- **`claim_commit_frontier` runs before `take_sticky` and `pop`,** unless this worker already holds a chain handoff. A `Ready` prefix is marked `Executing` and run. A `Parked` prefix goes through `claim_blocker`. An `Executing` prefix that is inside `handler.run` is left alone. An `Executing` prefix whose owner has gone idle, and who has not entered the interpreter, is stolen. Stealing from an owner who is still in the pre-check bounced the task between workers and never entered; that path waits.
- **`poke_work` notifies every sleeper.** It also raises `active` to `min(workers, ready + executing)` and notifies `sleep_cv` when the set grows. `try_mark_committed` notifies all even when the block is not finished. `park`, `rescue`, `unstick`, and `handoff_sticky` publish `work_seq` under `mu` before returning when they enqueue anything.
- **`seed` sets `active` to `min(workers, n)`.** The other workers are not parked until the first task finishes.
- **Both condvars time out at 500 µs.** That is the backstop. The publisher is the waker. A worker does not call `wait_work` when `ready_now() > 0` or `work_seq` has already moved.

Tests: `commit_frontier_is_claimed_ahead_of_later_ready_work`, `parking_on_a_finished_predecessor_wakes`.

### Rebuilt trace

Same block, C=4, timeline on, this binary. Dump clock 7.77 ms. Commit gaps above 0.5 ms: none. Three idle spans, longest 0.384 ms, and each has ready-depth 0. Seven parks, longest 381 µs (tx 4 on tx 3, estimate), and that interval sits inside tx 3's execution span. `delta_mismatch` is 0. Active samples are 4 until the tail sample of 1.

## Parked on a predecessor that is not running

On ict21, tx 102 parked on tx 101 (`0x111015025e6393bd`, reason estimate) 0.5–1.5 ms before tx 101's last execution started. Stage 2c's `claim_blocker` treats phase `Executing` as "this predecessor is running, so wait." The phase is stored at `pop` / `take_over`, before `handler.run`. The drive loop also ran `close_from` on the previous transaction after `depend` had already taken the new root. The root stayed `Executing`, not inside the interpreter, for that whole validation walk, and the claim rule refused to run it.

The close of the previous transaction now happens before anchor, admission, and class checks. Admission uses `in_interpreter`, not `is_executing`, so a popped-but-not-started predecessor is claimed. `claim_blocker` returns `Wait` only when the gate's running bit is set. If the owner is idle and the running bit is clear, the waiter steals and gets `Run`. `try_enter_interp` then fails for the original owner.

`estimate_claim_runs_a_predecessor_that_is_not_in_the_interpreter` pops tx 0, marks worker 0 idle, and checks that `claim_blocker(1, 0)` is `Run(0)`. The original `try_enter_interp` fails. The claimer's enter succeeds. The rebuilt C=4 trace has no park above 0.5 ms whose predecessor is outside an execution span.

## Buckets

`SPECFENCE_BUCKETS=1` or `SPECFENCE_INFLATION=1` is what calls `Instant::now`. A clean wall run calls neither. `WaitGuard` adds the interval to a wait bucket and, when it is opened inside `handler.run`, subtracts it from the interpreter total, from the class buckets, and from profile `interp_ns`.

| Wait | Where it is charged |
| --- | --- |
| Armed location coordination | `wait_armed` |
| Unarmed chain coordination | `wait_chain` |
| Estimate entry (`MemoryEntry::Estimate`) | `wait_estimate` counts the hit. The DashMap hold is `wait_lock`. The task then returns `Blocking`; the park is outside the interpreter. |
| Sender nonce block and admission predecessor | `wait_admit` |
| DashMap location read, base-share lookup, shared code fetch | `wait_lock` |

Median thread time, K=10, `(to, selector)`, system malloc, shared code and cache on. `lto=false`. These clocks are not the clean-scan wall times.

Block 15274915:

| Bucket | C=1 | C=4 | C=8 |
| --- | ---: | ---: | ---: |
| interpreter | 2.750 ms | 4.851 ms | 5.449 ms |
| plain | 0.466 ms | 0.741 ms | 0.847 ms |
| other | 2.364 ms | 3.863 ms | 4.156 ms |
| re-execution | 0 | 0.629 ms | 0.857 ms |
| sched | 0 | 0.646 ms | 0.757 ms |
| mv_record | 0.392 ms | 1.811 ms | 5.193 ms |
| wait_armed | 0 | 0.034 ms | 0.035 ms |
| wait_chain | 0 | 0.006 ms | 0.006 ms |
| wait_admit | 0 | 0.132 ms | 0.120 ms |
| wait_lock | 0.105 ms | 0.244 ms | 0.333 ms |
| wait_estimate (count) | 0 | 2 | 1 |

Plain transfers are 1046 calls. The class total is 0.466 ms at C=1 and 0.741 ms at C=4: 0.45 µs and 0.71 µs per transaction. The 50× rows in the rank are a few transfers whose fast path is under 1 µs. Their absolute gap is about 14 µs, and that time is what remains after the wait buckets are subtracted.

`mv_record` is still the bucket that grows with C (0.392 → 1.811 → 5.193 ms on four CPUs). It is outside the interpreter.

### Rank

`SPECFENCE_INFLATION=1`, one C=1 run against one C=4 run, sum of interpreter nanoseconds after wait exclusion. Block 15274915: fast 3.313 ms, parallel 4.709 ms, ratio 1.421. Stage 2c after the spin removal was 1.56× on a different clock. Wall of these two runs is 7.569 ms and 8.184 ms; the inflation clock is on, so this is not the clean scan.

| tx | to | selector | fast µs | parallel µs | ratio | attempts | reads | writes | first write |
| ---: | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 812 | `0x6262998Ced04146fA42253a5C0AF90CA02dfd2A3` | transfer | 0.4 | 14.3 | 32.27 | 1 | 1 | 3 | `0x7ec8be01af547316` |
| 866 | `0x6262998Ced04146fA42253a5C0AF90CA02dfd2A3` | transfer | 0.4 | 7.9 | 18.50 | 1 | 1 | 3 | `0x7ec8be01af547316` |
| 305 | `0x6262998Ced04146fA42253a5C0AF90CA02dfd2A3` | transfer | 0.4 | 6.0 | 13.84 | 1 | 1 | 3 | `0x7ec8be01af547316` |
| 1066 | `0x6262998Ced04146fA42253a5C0AF90CA02dfd2A3` | transfer | 0.4 | 3.1 | 7.18 | 1 | 1 | 3 | `0x7ec8be01af547316` |
| 364 | `0x6262998Ced04146fA42253a5C0AF90CA02dfd2A3` | transfer | 0.4 | 2.8 | 6.63 | 1 | 1 | 3 | `0x7ec8be01af547316` |
| 128 | `0x9Fb538820D4FDe2FCC509Dc01Ae73a192f36cfcC` | transfer | 1.8 | 9.3 | 5.04 | 1 | 4 | 3 | `0xde1644810b012b46` |
| 648 | `0x7758E507850dA48cd47df1fB5F875c23E3340c50` | transfer | 1.8 | 8.3 | 4.54 | 1 | 4 | 3 | `0xabd6bb3978815b97` |
| 56 | `0x4ECF8850f8eE33e887e1a43d26707EE5fa1f1471` | transfer | 1.1 | 5.0 | 4.48 | 1 | 4 | 3 | `0x755ad1f8affcb733` |
| 6 | `0xf25E10cDdA8E1b4A6f3b8E1eAD45C223f5538e57` | transfer | 1.4 | 5.4 | 3.78 | 1 | 4 | 3 | `0x2ae8c664833d9bff` |
| 104 | `0xDebfBE80C8aebA98A32968278463ccB639C6C4e3` | transfer | 1.1 | 4.0 | 3.60 | 1 | 4 | 3 | `0xb981c922e19cdd48` |

Block 3356896, same flags: fast 0.265 ms, parallel 0.484 ms, ratio 1.829. The top row is tx 32, 0.9 µs → 18.1 µs, one attempt, 4 reads, 3 writes.

## Sched

Stage 2c still recorded about 11 ms of sched thread time at C=16 on ict21 (warm). `wait_work` is outside the bucket. The bucket is `claim_commit_frontier`, `take_sticky`, and `pop`. `take_sticky` took the scheduler mutex on every attempt, including the common case where the sticky slot is empty. At C=16 every worker hits that mutex on every task boundary, and the steal loop adds empty pops of the other deques. That is the 11 ms. It is not the condvar.

`take_sticky` now returns before the mutex when `sticky_flag` is clear. `pop_injector` returns before its mutex when the length is zero. `claim_commit_frontier` is two atomics when the prefix is already inside the interpreter, and takes the mutex only when the prefix is `Ready`, `Parked`, or `Executing` but not yet running.

On this VM, buckets on, sched median is 0.646 ms at C=4 (1118 calls) and 0.757 ms at C=8. Stage 2c on the same shape was 0.819 ms at C=4. C=16 was not run: four CPUs.

The 500 µs timeout does not show up as a poll through the block. The rebuilt trace has three idle spans, all at the tail, all with ready-depth 0, longest 0.384 ms.

## Clean scan

4 vCPUs, CPUs 0–3, release, `lto=false`, `codegen-units=16`, system malloc, shared code and shared cache on, no buckets, no timeline, no warm-up, fresh engine each round, K=10. Median is the sorted sample at index 5. The 95% interval is a bootstrap of that median, 10,000 resamples, seed 0. SEQ is timed on the C=1 rounds only. `delta_mismatch` is 0 and `ok` is true in every cell. C=8 is eight workers on four CPUs.

The dedicated sequential ab above (engines=seq, CPU 0) is the allocator gate. The SEQ column here is the C=1 round inside the three-engine shuffle, so it is a few tenths higher.

### `(to, selector)`

Block 15274915. SEQ 3.996 ms [3.877, 4.436].

| C | OCC ms | OCC 95% CI | SF ms | SF 95% CI | reexec | full replay |
| ---: | ---: | --- | ---: | --- | --- | --- |
| 1 | 5.435 | [5.089, 6.628] | 5.076 | [4.737, 5.414] | 0 | 0 |
| 4 | 3.675 | [3.468, 3.925] | 6.703 | [6.403, 7.014] | 3–10 | 3–8 |
| 8 | 4.184 | [3.929, 4.729] | 7.276 | [7.042, 7.685] | 6–14 | 5–10 |

Block 3356896. SEQ 0.308 ms [0.297, 0.318].

| C | OCC ms | OCC 95% CI | SF ms | SF 95% CI | reexec | full replay |
| ---: | ---: | --- | ---: | --- | --- | --- |
| 1 | 0.512 | [0.451, 0.562] | 0.399 | [0.394, 0.415] | 0 | 0 |
| 4 | 0.535 | [0.524, 0.566] | 1.180 | [1.142, 1.216] | 0 | 0 |
| 8 | 0.671 | [0.624, 0.864] | 1.406 | [1.236, 1.510] | 0 | 0 |

### `(code_hash, selector)`

Block 15274915. SEQ 3.902 ms [3.844, 4.299].

| C | OCC ms | OCC 95% CI | SF ms | SF 95% CI | reexec | full replay |
| ---: | ---: | --- | ---: | --- | --- | --- |
| 1 | 5.459 | [5.266, 6.370] | 5.127 | [4.716, 5.397] | 0 | 0 |
| 4 | 3.609 | [3.281, 4.112] | 6.786 | [6.347, 6.929] | 2–8 | 2–5 |
| 8 | 4.115 | [3.945, 4.819] | 7.064 | [6.847, 8.225] | 6–11 | 3–7 |

Block 3356896. SEQ 0.311 ms [0.302, 0.313].

| C | OCC ms | OCC 95% CI | SF ms | SF 95% CI | reexec | full replay |
| ---: | ---: | --- | ---: | --- | --- | --- |
| 1 | 0.499 | [0.449, 0.551] | 0.401 | [0.394, 0.421] | 0 | 0 |
| 4 | 0.533 | [0.519, 0.575] | 1.183 | [1.155, 1.229] | 0 | 0 |
| 8 | 0.699 | [0.646, 0.779] | 1.333 | [1.231, 1.415] | 0 | 0 |

Stage 2c on this VM, with the wrapper still installed, had 15274915 `(to, selector)` SF at 6.950 / 9.630 / 9.437 ms. The same cells are now 5.076 / 6.703 / 7.276 ms. Part of that drop is the allocator. The trace says the rest is the prefix: the 4.7 ms hole is gone. SF(4) is still above SF(1). What remains in the buckets is interpreter time on non-plain contracts (2.364 → 3.863 ms), re-execution (0.629 ms), and `mv_record` (1.811 ms).

## Gates

Measured here. ict21 C=1, 4, 8, 16, 32 is the confirmation run.

| Gate | This VM |
| --- | --- |
| Default harness SEQ within 5% of the PR 72 binary | 3.829 vs 3.773 ms (1.015×). Met. |
| No commit-frontier gap above 1 ms from idle workers | Rebuilt C=4 trace: no commit gap above 0.5 ms. Longest idle 0.384 ms with ready-depth 0. Met on that trace. |
| No park above 0.5 ms on a predecessor that is not running | Longest park 381 µs, and it overlaps the predecessor's execution span. Met on that trace. |
| 15274915 `(to, selector)`: SF(4) < SF(1) | 6.703 vs 5.076. Not met. |
| SF C=4 ≤ 1.5× OCC C=4 | 6.703 vs 3.675 (1.82×). Not met. |
| SF C=4 ≤ OCC C=4 | Not met. |
| specfence tests, focus blocks, seq/par repeat, SLOAD static gas | 40 `specfence::` tests. `sf_matches_onchain_focus_blocks`, `sf_seq_par_repeat`, `sload_static_gas_matches_chain_header`. Met. |
| SF equals SEQ at every C | `delta_mismatch` is 0 in every cell above. The focus-block and repeat tests match sequential execution. Met on the cells this host ran. |

## Reproduce

Pinned host, fat LTO, the default binary (no `specfence-mimalloc`). No warm-up. K=10. SEQ is the C=1 rounds only. Do not set `SPECFENCE_TIMELINE` on a timed run. Do not set `SPECFENCE_ALLOCATOR=mimalloc` unless the binary was built with `--features specfence,specfence-mimalloc`.

```bash
scripts/soft0_percore_scan.sh --cpu-list 128-255 --c-list 1,4,8,16,32 --k 10 --profile-k 1
SPECFENCE_CLASS_KEY=code_hash scripts/soft0_percore_scan.sh \
  --cpu-list 128-255 --c-list 1,4,8,16,32 --k 10 --profile-k 1 --skip-build
```

Sequential ab against a PR 72 binary built the same way, one pinned CPU:

```bash
SPECFENCE_INFLATION_WHICH=scan SPECFENCE_INFLATION_BLOCKS=15274915 \
  SPECFENCE_INFLATION_K=10 SPECFENCE_INFLATION_ENGINES=seq SPECFENCE_CLASS_KEY=to \
  taskset -c 128 target/release/examples/specfence_inflation_dig \
  --workers 1 --cpu-list 128
```

Buckets and rank. Buckets call `Instant::now`, so the wall column is not the clean scan.

```bash
SPECFENCE_BUCKETS=1 SPECFENCE_CLASS_KEY=to SPECFENCE_INFLATION_WHICH=scan \
  SPECFENCE_INFLATION_K=10 SPECFENCE_INFLATION_ENGINES=sf \
  taskset -c 128-255 target/release/examples/specfence_inflation_dig \
  --workers 16 --cpu-list 128-255

SPECFENCE_INFLATION=1 SPECFENCE_INFLATION_WHICH=rank \
  SPECFENCE_INFLATION_BLOCKS=15274915 SPECFENCE_CLASS_KEY=to \
  taskset -c 128-255 target/release/examples/specfence_inflation_dig \
  --workers 16 --cpu-list 128-255
```

This VM used CPUs 0–3, `cargo +stable`, and `--config profile.release.lto=false --config profile.release.codegen-units=16`. C=8 passed `--allow-oversub` in spirit: `--workers 8 --cpu-list 0,1,2,3`.
