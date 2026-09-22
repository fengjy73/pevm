# SpecFence v3 per-access land

**Date:** 2026-09-22
**Branch:** `cursor/specfence-sf-ps-true-spine-d6e8`
**Code:** `00d42cb`. This note is the docs commit that follows it.
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

Reuse walls are still above OCC on all four (primary SF/OCC about 1.51, 1.58, 2.08, 1.79). That is left for the next single-block tune. The ArmTable can still record a window arm on a location that also has NeverWait; the read path does not park that address.
