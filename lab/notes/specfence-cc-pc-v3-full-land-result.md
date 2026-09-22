# SpecFence v3 per-access land

**Date:** 2026-09-22
**Branch:** `cursor/specfence-sf-ps-true-spine-d6e8`
**Code:** `00d42cb`. Focus census `97df2f7` (fail_k histogram, FullReplay-without-prefix, first Execution-pick time). This note follows that census.
**Soft:** 0. `Scheduler::next_task*` was not restored.

The coordinator files `specfence-cc-pc-first-principles-redesign-v3.md`, `specfence-detect-avoid-resolve-learn-redesign-access-v2-adaptive.md`, and `specfence-cc-pc-v3-full-land-v1.md` are not in this checkout. `gh api repos/fengjy73/specfence-lab` returns 404, so those notes were not copied onto this branch. The land follows the must-land list and `lab/notes/specfence-detect-avoid-resolve-learn-redesign-access-v1.md`. `lab/notes/specfence-access-grain-dig-v1.md` was already on the branch.

## What landed

**Avoid, per read.** `Opt | WaitOnce | NeverWait` in `access_arm.rs`. Beneficiary and `is_lazy` are NeverWait: a live multi-version tip is skipped, not parked. A true live RAW/WAW writer parks once per `(tx, ℓ, w)`. A second attempt of the same triple is counted in `access_wait_suppressed` and does not return `Blocking`. A writer that is already done or validated returns `InconsistentRead` so the read is taken again. `leftover_min` still parks, except on beneficiary and lazy.

**Resolve.** `validate_to_plan` no longer returns `OrderedReplay` for the Opt path or for an edged EffectiveWAW. One invalid location with a known `k > 0` that is not NeverWait and not lazy copies value snaps for `k' < fail_k` into `ff_head` after the abort clears the previous head. The failed location is not in that set. The next incarnation serves those reads from `ff_head` when the origin still matches (`journal_ff_hits`). Value-stable rebind is unchanged and may stay 0.

**Learn.** The read arm is the AccessArm table, not a predicted-essential bit by itself. A hot early WAW calls `note_early_waw` so the next read of that location is WaitOnce. NeverWait is packed as a prior and is not decayed. A morph flip drops WaitOnce that this block did not reinforce. `force_opt` and under-covered no longer set sticky Opt. An Opt prior does not install sticky. IntraPatch still queues on FullReplay and now also on partial abort.

**PC.** `seed_begin` pushes high indices first, so the local LIFO pop takes the lowest index (longest remaining suffix). Steal pops the other end. Gated transactions that may not execute are still refused in `admit_or_refuse`. Idle workers already steal inside `pick` and release finished writers in `heal`. MvMemory, the scheduler's live writer, `AccessArmTable`, `ArmTable`, and the runnable queues are shared. Each worker keeps its own `Vm` (stack, journal) and its `worker_i` steal seed.

## Checks

`RUSTUP_TOOLCHAIN=stable cargo test -p pevm --lib --release` (lto off, codegen-units 16, `--test-threads=1`): **415 passed**.

`complete_arch_edge_pi_seq_eq_par_softwait0`: **ok**.

Instant-off, `SPECFENCE_COMPARE_CHECK=1`, 8 cores, N=3. Every block `seq=par`, `occ_picks=0`, `resolve` ordered count 0, `soft_wait_arms=0`. No hang. No mixed-49 sweep.

| Block | iter | wait_once | suppressed | never_wait | prefix_resume | journal_ff_hits |
|------:|-----:|----------:|-----------:|-----------:|--------------:|----------------:|
| 3356896 | 0 | 5 | 0 | 0 | 13 | 35 |
| 3356896 | 1 | 3 | 0 | 0 | 9 | 27 |
| 3356896 | 2 | 2 | 1 | 0 | 9 | 27 |
| 15274915 | 0 | 72 | 0 | 2 | 67 | 110 |
| 15274915 | 1 | 67 | 35 | 0 | 82 | 268 |
| 15274915 | 2 | 66 | 16 | 0 | 91 | 130 |
| 19860366 | 0 | 132 | 71 | 96 | 90 | 1228 |
| 19860366 | 1 | 147 | 152 | 206 | 98 | 828 |
| 19860366 | 2 | 102 | 51 | 87 | 68 | 767 |
| 19469097 | 0 | 135 | 25 | 248 | 142 | 757 |
| 19469097 | 1 | 140 | 24 | 125 | 174 | 873 |
| 19469097 | 2 | 165 | 14 | 214 | 189 | 837 |

`wait_once` is the number of distinct `(tx, ℓ, w)` first parks. `suppressed` is a repeat of a triple that did not `Blocking` again. The unit test `beneficiary_never_waits_and_true_waw_waits_once` checks the cap is 1. `19860366` and `19469097` are the blocks whose beneficiary k=1 used to mill; `never_wait` is those skips. `3356896` had no live beneficiary or lazy tip in these three iters (`never_wait=0`) and still kept a prefix (`prefix_resume` 13/9/9, `journal_ff_hits` 35/27/27).

Reuse walls are still above OCC on all four (primary SF/OCC about 1.51, 1.58, 2.08, 1.79 on that pre-census sample). The ArmTable can still record a window arm on a location that also has NeverWait; the read path does not park that address.

`cargo test -p pevm --lib --release` on `97df2f7`: **415 passed**. `complete_arch_edge_pi_seq_eq_par_softwait0` passed on a retry after one warm `seq!=par`. A 20-run sample on `97df2f7` was 19/20 (the miss was the cold block inside `run_mode`). On `5c9df95`, before the census, 2/20 failed the warm assert. Occasional `seq!=par` on this ERC-20 cluster predates the census. No hang.

## Focus pair 3356896 + 15274915

Instant-off, check on, 8 cores, N=3, census `97df2f7`. Both blocks `seq=par`, `occ_picks=0`, ordered resolve 0, `soft_wait_arms=0`. Host has 4 cores; the request is 8, so the ratio is the comparison and a single wall moves. `chain` is the longest final writer list. `head` is its smallest index. `corr` is `corr(tx index, first Execution-pick)`. Positive means low indices start first. `full_from_0` is a FullReplay that installed no prefix snaps. `fail_k` is the known k of a prefix keep.

| | 3356896 | 15274915 |
|:---|---:|---:|
| n | 176 | 1226 |
| OCC median ms | 1.652 | 8.542 |
| SF cold / reuse0 / reuse1 ms | 2.309 / 1.745 / 2.114 | 16.193 / 20.030 / 13.646 |
| primary SF/OCC | 1.28 (2.114 / 1.652) | 2.34 (20.030 / 8.542) |
| wait_once | 9 / 3 / 4 | 105 / 66 / 30 |
| suppressed | 4 / 0 / 0 | 7 / 0 / 0 |
| never_wait | 0 / 0 / 0 | 2 / 0 / 0 |
| prefix_resume | 30 / 7 / 19 | 114 / 95 / 73 |
| journal_ff_hits | 74 / 19 / 49 | 139 / 138 / 100 |
| FullReplay | 30 / 7 / 19 | 193 / 150 / 121 |
| full_from_0 | 0 / 0 / 0 | 79 / 55 / 48 |
| fail_k | min 5, max 9; mass at 5 and 6 | min 4, max 18; mass at 4 |
| chain | `dff71d59d972d654`, 15 writers | `abd6bb3978815b97`, 77 writers |
| head tx, start ms | 66, 0.645 / 0.484 / 0.543 | 116, 2.249 / 2.184 / 2.189 |
| tail tx, start ms | 171, 1.195 / 0.984 / 1.084 | 1219, 8.208 / 7.737 / 6.704 |
| corr | +0.999 / +0.975 / +0.995 | +0.993 / +0.998 / +0.997 |
| low-decile / high-decile start ms | ~0.28 / ~1.06 | ~1.63 / ~7.3 |
| explore | 0 | 0 |

`3356896` iter detail, fail_k counts: cold `5:9,6:20,9:1`; reuse `5:2,6:4,9:1` and `5:5,6:13,9:1`. `15274915`: cold `4:110` plus four later k; reuse `4:89` and `4:69`, each with a handful of k in 7..18.

Light pass, same binary, not the optimize target: `19860366` primary SF/OCC 2.05 (22.838 / 11.160), `19469097` 2.56 (21.912 / 8.567). Both `seq=par`, `occ_picks=0`. `never_wait` is 116–322. `full_from_0` is 25–37 and 4–18.

### What still violates v3

Shared by the pair, and the input to the next optimize loop:

1. **Avoid still misses the early WAW.** The failing read is k=5 or 6 on `3356896` and k=4 on `15274915` (`abd6bb…` / `dff71d…`, the same basic locations as the access-grain dig). FullReplay is larger than WaitOnce on every iter. The read is Opt, validation fails, then the tx restarts. Suppressed stays near 0, so the WaitOnce cap is holding; it is not the thing being used on this edge. Beneficiary is not the mill (`never_wait` is 0 on every reuse iter).

2. **Resolve is still a FullReplay entry.** `3356896` installs a prefix on every FullReplay (`full_from_0=0`) and `journal_ff_hits` shows those earlier reads served from `ff_head`. `resolve_rewind` is 0, so the interpreter is not resumed at `fail_k`; the call starts again and the overlay answers the prefix. `15274915` is worse: 48–79 of 121–193 FullReplays install nothing (`full_from_0`), so those restarts have no prefix snaps. Ordered resolve stays 0. Rebind stays 0.

3. **PC order flipped, crit-head is still late relative to index 0.** `corr(index, first_start)` is about +1 on both blocks, and the chain head starts before the chain tail. That is the reverse of the pre-v3 index order (about −0.97). The remaining gap is that the pick is longest-remaining by index, not by the crit chain. On `15274915` the chain head is tx 116 and it starts at ~2.2 ms, after the low-decile median (~1.6 ms). On `3356896` tx 66 starts at ~0.5 ms on a ~2.1 ms reuse wall. The tail of `15274915` (tx 1219) still starts at 6.7–8.2 ms and then waits on that head.

4. **Learn does not retire the next FullReplay.** `explore=0` on every iter. Reuse still fails at the same k (4, or 5/6). `3356896` FullReplay went 30 → 7 → 19. `15274915` went 193 → 150 → 121. The early-WAW note is not an arm that stops the next read of that location from taking the Opt path.

Walls stay above OCC: 1.28 on `3356896`, and 1.60–2.34 on `15274915` across the two reuse iters (the printed primary is the worse reuse, 2.34). No mixed-49 sweep.
