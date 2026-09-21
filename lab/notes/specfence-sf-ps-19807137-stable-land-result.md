# SpecFence — leftover_passed stale park, stable Soft=0 land

**Date:** 2026-09-21
**PR:** [#45](https://github.com/fengjy73/pevm/pull/45) `cursor/specfence-sf-ps-true-spine-d6e8`
**Continues:** `f8cde47` (partial-abort `note_abort_reincarnate`, `blocked_on` mill guard, `leftover_min_on_skippable_gate` admit-only).
**Does not restore** `Scheduler::next_task*` / `next_task_with_wave_ready` / `validate_occ_stage`.
**Does not re-land** width-8 leftover Detect, earlier←later leftover_min recover, `may_execute` true for every leftover_passed blocker, `recover_executing` of a ghost writer, leftover_min always-may_execute, or hot-path flush.

Harness: `specfence_3356896_compare`, Soft=0, Instant-off (one SpecFence `Pevm` reused), 8 cores. `occ_picks=0` on every SpecFence line below. Mainnet rows are finish-without-abort; `complete_arch_edge_pi_seq_eq_par_softwait0` checks sequential equality.

---

## Cause

Two races stacked on the mill guard from `f8cde47`.

1. **Stale Aborting park.** A chain root stays `Aborting` on a writer that is already `leftover_passed` and whose worker has left (`ST_WAIT`, not `ST_RUNNING`). The direct Detect pred is often not passed; an ancestor is. `live_block` correctly refused `recover_aborting` (that was the ~38k incarnation mill). The waiter then never moved, so the block did not validate.

2. **`force_push` / `mark_wait` of `ST_RUNNING`.** Drain and heal swapped a live execute's runnable bit onto a queue. The next pick lost `try_execute` (status already `Executing`) and `mark_wait` stored `ST_WAIT` over the owner still inside `vm.execute`. Heal treated that as a ghost and `recover_executing_waiter` entered the same `ExecutionResults` slot. glibc reported it later as `double free`, `unaligned tcache` / `fastbin`, or SEGV. On `f8cde47` this aborted 6196166 on all 3 Instant-off N=3 runs tried here (after a printed SpecFence ok line). Dropping the wake instead of stealing it hung with `n_unf>0`, `has_unfinished=false`, queues empty: the txs were `Executed` with runnable state `ST_DONE`, and `has_unfinished` does not count scheduler-done txs, so the idle recover never ran.

`ReadyEdgeTable` is still constructed fresh per `Pevm::execute`. Cross-block `leftover_passed` leak remains false.

---

## What this cut does

- `clear_stale_block`: if the waiter and the `leftover_passed` writer are both not `ST_RUNNING`, drop `blocked_on` and detach, then `recover_aborting` the waiter once. Execute does not `add_dependency` when the blocker is `leftover_passed`, so the recover does not re-park into a mill. `may_execute` stays false. The writer is not `recover_executing`'d.
- `wake_idle`: heal, drain, and higher-reader revalidate refuse `ST_RUNNING` (CAS). `ST_DONE` may still be queued — that is the rewind/revalidate path, not a live slot. A running wave entry is pushed back after the drain loop; it is not dropped and it is not `force_push`'d.
- Owner paths (`schedule` after a failed claim, `Resolve` requeue of the tx just validated) still use `force_push`. A failed pick releases with `release_running` (CAS `ST_RUNNING` → `ST_WAIT`) so a stolen bit is not clobbered.
- Heal requeues `Executed`/`Ready` txs whose runnable state is already `ST_DONE`, so a dropped revalidate cannot sit forever behind `has_unfinished=false`.

---

## Soft=0 Instant-off @ 8 cores

| Block | Runs | SF cold ms | SF reuse ms | OCC cold ms | occ_picks (SF) | soft | Learn |
|------:|-----:|-----------:|------------:|------------:|:--------------:|-----:|-------|
| 3356896 | 1× N=3 | 2.314 | 2.180, 1.784 | 1.474 | 0 | 0 | Win_1 → Opt |
| 6196166 | 4× N=3 | 7.6–28.5 | 6.8–49.4 | ~2.5–3.2 | 0 | 0 | Win_1 → Opt |
| 19807137 | 2× N=3 | 261.2, 81.8 | 164–292 | 2292, 2318 | 0 | 0 | Opt / Win_2 → Opt |
| 19469101 | 1× N=3 | 31.971 | 16.188, 16.504 | 9.129 | 0 | 0 | Win_2 → Opt |
| 14689597 | 1× N=3 | 29.900 | 34.392, 41.291 | 7.872 | 0 | 0 | Win_2 → Opt |

6196166 on `f8cde47` (same harness, 3× N=3 before this cut): every run aborted in the allocator (`double free (!prev)`, `unaligned tcache`) after at least one SpecFence line printed ok. This cut: 4/4 N=3 finished, including reuse, no allocator abort.

19807137 cold SpecFence is under the OCC cold wall (~2.3 s) on both runs (261 ms and 82 ms). Reuse SpecFence is slower than warm OCC (~16–18 ms) and did not repeat the earlier one-off 59 ms; both N=3 runs finished with `inc>0` 611 then 0, and 610 then 0. Not an incarnation mill.

`complete_arch_edge_pi_seq_eq_par_softwait0`: pass, Soft=0, sequential equals parallel on the cold block and the warm second block.
`specfence_learn_wiring_is_first_class`: pass (B1–B7 source arms).
Lib: `leftover_min_*` and `*_source_never_calls_*` (`next_task`, `next_occ_task`, `validate_occ_stage`) pass.

---

## Still out

- Mainnet compare does not check receipts against sequential execution.
- 6196166 and 19469101 SpecFence walls are above OCC on these runs. They finish; they are not the fast path versus warm OCC.
- `recover_executing` of a leftover_passed writer that is `Executing` + `ST_WAIT` is still not done. This cut ungates the waiter instead.
