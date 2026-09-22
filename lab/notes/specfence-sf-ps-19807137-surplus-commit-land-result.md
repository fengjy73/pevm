# SpecFence — 19807137 surplus-commit / 619 reuse land result

**Date:** 2026-09-21  
**PR:** [#45](https://github.com/fengjy73/pevm/pull/45) `cursor/specfence-sf-ps-true-spine-d6e8`  
**Base tip checked:** `1b36e7f` (abort-flush and worker skip-plant already dropped)  
**This note does not restore** `Scheduler::next_task*` / `next_task_with_wave_ready` / `validate_occ_stage`.  
**Width-8 leftover Detect is not re-landed.**  
**Earlier←later leftover_min recover is not re-landed.**

---

## 1. Cross-block `leftover_passed_bits` leak — false

`Pevm::execute` builds a new `ReadyEdgeTable` at the start of every block (`ReadyEdgeTable::new()`). `leftover_passed_bits`, `leftover_bits`, and `global_leftover_min` live on that table. They are not on `Pevm`, not `thread_local`, and not static.

What survives a reused `Pevm` is learn state (`CostPolicy`, `InterBlockPrior`, `HotSet`, `BayesMap`). That changes the next iteration's plants. It does not keep leftover bits.

A green first SpecFence iteration followed by a reuse crash is latent glibc heap damage reported on a later allocation (`double free` / `unaligned tcache` / `Segmentation fault`), or a reuse-specific plant pattern from learned policy. Not a bitset carried into the next table.

---

## 2. 19807137 hang — verified chain, not a bitset leak

`SPECFENCE_HANG_TRACE=1`, Soft=0, 8 cores. OCC finishes (~2.3 s). SpecFence then sticks with `n_unf` frozen, `occ_picks` unused (SF pick only). Queues empty or one head. Not an OCC pick.

Representative chain after the claim has walked:

| glob_min | direct pred | pred_passed | chain | root | root scheduler dep |
|---------:|------------:|:-----------:|-------|------|--------------------|
| 445 | 438 | **true** | depth ≥ 8, `passed_hop=438` | Aborting, `may_execute`, no Detect edge | writer that is **leftover_passed + edge done-bit + scheduler Ready/Executing**, incarnation hundreds |
| 180 | 179 | false | depth 10, `passed_hop=172` | Aborting on the chain | same shape: passed writer, edge done-bit set, scheduler not `Executed` |
| 310 | 309 | false | depth 2 | 309 Aborting, ungated, `may_execute` | 308 Executing, `ST_WAIT` (worker already left), passed, edge done-bit, inc ~15 |

Before the mill guard, the chain root's incarnation climbed to **~38_000**. `heal` saw `may_execute` (no Detect edge: `note_consumer_on` returns immediately when `is_writer_done`) and `recover_aborting` did `incarnation++` while `add_dependency` had already parked the tx on a writer that was not scheduler-done.

Why the done-bit and the scheduler disagree: `PartialAbortRewind` calls `try_validation_abort` (status → Aborting, scheduler done-flag cleared) and then `finish_validation_sf` (→ Ready, incarnation++) **without** `note_abort_reincarnate`. The edge done-bit and `leftover_passed` from the earlier commit stay set. Readers treat the writer as published, skip the Detect plant, park in the scheduler, and the heal mill starts.

---

## 3. What landed

1. **`PartialAbortRewind` calls `note_abort_reincarnate`.** Bits only (same as full abort). The edge done-bit no longer outlives the publish.
2. **`blocked_on` + `live_block`.** `add_dependency` records the writer. Heal does not `recover_aborting` while that writer is still unpublished. It requeues the writer when the writer is Ready or Executed and not `ST_RUNNING`. Validation abort clears `blocked_on` (it is not a park).
3. **`leftover_min_on_skippable_gate`.** If leftover_min's direct Detect pred is `leftover_passed` or leftover surplus, pick admits it and heal requeues it. `may_execute` stays false, so `drain_wave` does not `force_push` that tx. Execute skip-park (`leftover_min_skips_blocker`) is unchanged.

Hang trace (`SPECFENCE_HANG_TRACE`) also prints the Detect chain, runnable state, and the scheduler dependency of the chain root. It is off unless the env var is set.

---

## 4. Discarded (measured this pass)

| Attempt | Result |
|---------|--------|
| `may_execute` true when the blocker is `leftover_passed` (all consumers) | **3356896** and **6196166** reuse hung (Ready / !may_execute or timeout). Reverted. |
| `drain_wave` defer while `ST_RUNNING` && `Executing`, paired with that `may_execute` change | Same hangs. Reverted. Dropping the wake without a later apply was already known to hang 619. |
| Heal `recover_executing_waiter` on the live scheduler blocker when it is Executing but not `ST_RUNNING` | **3356896** `Segmentation fault`. **6196166** SF[0] ok then SEGV on reuse. Reverted. |
| `may_execute` true only for leftover_min's passed/surplus blocker, heal `flush_pred_if`, `drain_wave` defer alone, always-`may_execute`, recover+`force_push` after Blocked, width-8 Detect, earlier←later recover, abort-path flush, worker skip-plant | Already discarded on or before `1b36e7f`, or in the note that was on the tree at the start of this pass. Not re-landed. |

---

## Instant-off Soft=0 @8

Harness: `specfence_3356896_compare`, `SPECFENCE_COMPARE_CORES=8`. Product path includes the three lands above. `occ_picks` below are the SpecFence lines.

| Block | N | OCC med | SF cold | SF reuse | Soft | occ_picks | Status |
|-------|--:|--------:|--------:|---------:|-----:|----------:|--------|
| **3356896** | 3 | ~1.5–2.2 ms | 2.317 | 1.791 | 0 | **0** | **green** Learn `Win_1` then `Opt`, `explore=0`, `switch=2`. A later N=3 after reverting the ghost-exec recover also finished (`sf_all_median_ms=2.090`, `occ_picks=0`) |
| **6196166** | 3 | (in log) | 6.754 | 15.074 | 0 | **0** | **green this run** (reuse included) |
| **6196166** | 3 | — | SF[0] then abort | — | — | — | **flaky** `double free or corruption (out)` / `(top)` on a later run. Same latent-heap family as §1. One run with hang-trace left on also stuck at `n_unf=25`, glob_min=79 gated on an Aborting pred whose scheduler dep was Executing |
| **19807137** | 1 | 2.316 s | **59.1 ms** | — | 0 | **0** | **one clean finish** after the mill guard, before the narrow admit (`sf_picks=3339`, soft 0) |
| **19807137** | 1 | ~2.3 s | hang / SEGV | — | — | — | **open** chain in §2. `root_inc` stays small (2–15) after the mill guard; the block still does not validate. Direct pred is often not `leftover_passed`; an ancestor in the chain is. SEGV / `unaligned fastbin` still happens |
| **19469101** | — | — | — | — | — | — | not re-run |
| **14689597** | — | — | — | — | — | — | not re-run |
| complete_arch Soft=0 | — | — | — | — | — | — | not re-run; prior tip was `double free` / `munmap_chunk` |

619 is not a deterministic reuse-only SEGV and not a leftover-bitset leak. One full N=3 finished with `occ_picks=0` and Soft=0, including reuse. Another aborted in the allocator.

198 can commit on this scheduler (59 ms vs OCC ~2.3 s, `occ_picks=0`). The same binary also hangs and heap-aborts. Not papered with OCC pick.

Learn B1–B7 call-graph was not edited (`arms.observe`, `note_e4`, `begin_from_prior` still in source). Lib tests passed: `leftover_min_skips_passed_leftover_pred`, `leftover_min_still_detect_plants_live_earlier_non_leftover`, `pick_source_never_calls_next_task`, `pick_source_never_calls_block_stm_next_task`, `worker_source_never_calls_block_stm_pick`. The integration Learn suite and `complete_arch_edge_pi_seq_eq_par_softwait0` were not re-run.

---

## Still open

leftover_min's direct passed/surplus gate is now admissible without a DashMap flush and without `may_execute` returning true. The hang that remains is a longer Detect chain whose **root** is `Aborting` on a writer that is `leftover_passed` with a stale-or-live Executing/Ready status and often `ST_WAIT` (the worker has left). Recovering that ghost with `recover_executing_waiter` SEGV'd 335 and 619. Leaving it parked stops the incarnation mill and also stops the chain.

`may_execute` treating every `leftover_passed` blocker as published hung 335 and 619 reuse. That path stays closed.
