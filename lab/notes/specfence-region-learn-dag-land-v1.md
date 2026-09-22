# SpecFence region-learn land v1

**Date:** 2026-09-22
**Base:** `129bbd0a678d51030673b032f835562e87166f72` (PR #45 merge)
**Branch:** `cursor/specfence-region-learn-dag-51e8`
**Harness:** Soft=0 Instant-off, `SPECFENCE_COMPARE_CHECK=1`, N=5, `SPECFENCE_COMPARE_CORES=8`
**Host:** this VM has 4 CPUs (`nproc`). The harness still asked for 8 workers, so absolute milliseconds move between samples. Ratios below are from that host, not a quiet 8-core box.
**Co-author:** `0xstride <fengjy73@users.noreply.github.com>`

Bar is **TPS SF/OCC ≥ 1.5 on both** focus blocks. **Not met.**

---

## Four-class status (pre-land)

Diagnosis carried from the four-class root note. This iteration only changes mechanisms named in the unsolved rows.

| Class | 3356896 | 15274915 | Root if unsolved |
| RAW | yes (spine) | partial | Large: RAW is secondary; no access-grain true-Data arm is the acceptance driver |
| WAW | **no** | **no** | Region Avoid is not armed before later Opt of the shared basic ℓ after the first early-k evidence |
| WAR | **no** | **no** | No first-class reader-protect; `war_ab` is not a solved WAR |
| Chain | partial | **no** | The hot region is still Opt-raced; schedule/park is not shortest-hop plus antichain plus region Avoid |

## Ideal-DAG Diff

Bounds are the ideal-DAG note (LB = L_crit). Machine-readable stubs: `lab/notes/dag-3356896.json`, `lab/notes/dag-15274915.json`. Full edge lists were **not** regenerated (`known_from_notes_only: true`). No invented RAW/WAR endpoints.

### 3356896

Ideal: WAW spine on basic `dff71d59d972`, writers `4, 31, 66, 67, 69, 70, 93, 96, 103, 115, 131, 132, 135, 138, 141, 166, 171`, earliest k = 5 or 6, RAW = 0 on that spine, WAR = 18 (endpoints unknown), antichain proxy ~153, LB = 0.031 ms.

| | |
|---|---|
| Recognized | Reuse `region_avoid` on `0xdff71d59d972d654`, **n=17, head tx 4, tail tx 171**. Same ℓ and the same endpoints as the note. |
| Missed txs | Cold and the first reuse often see a **partial** writer list (observed n=9, head 69). Scope floor is 12, so that partial list is not armed. Those hops still Opt. |
| Missed edges | The 16 adjacent WAW hops are not installed until a snapshot contains the full 17. WAR=18 endpoints are still unnamed. |
| Wrong Avoid | **Tried and dropped:** `plant_nearest_preds` on the 17 (off-queue until the pred executes). Reuse median SF **2.273 ms**, TPS **0.53**. Calm full=0 iters were slower than the parallel shell. |
| What Avoid does now | At the read, the one-hop pred must be **validated** before an origin is recorded. Antichain txs are not in the writer list and stay OCC-shaped. |

### 15274915

Ideal: WAW hop on basic `abd6bb397881`, L=77, earliest k=3, RAW=35, WAR=107, work-weighted path tx 0 → tx 102 (13 txs), LB=1.19 ms, antichain proxy ~1111. Full writer list is not in the notes and was not emitted.

| | |
|---|---|
| Recognized | The ≥32 crit spine is still the existing hold. This land does not replace it. |
| Missed txs / edges | `abd6bb…` writers, RAW=35, and WAR=107 are not in a regenerated edge list. Later touchers are not region-armed before the first conflict. |
| Wrong Avoid | A length-8 side location `0x483a65c8273c8219` (head 73, tail 82) was selected by a looser floor and planted. `wait_for_dependency` spiked (one iter 2590). Floor is now 12, so that side chain is not a region. |
| Schedule vs DAG | Antichain fill is not what the 77-hop is missing. The hop is still Opt until mid-block `protect_hot`, then a validated wait that can re-enter from k=0. |

---

## What changed

Region Learn, two axes, on the short WAW list the ≥32 crit filter used to drop:

1. **Scope.** `select_short_region_avoid` keeps the longest non-beneficiary location with **12..=31** writers and `len * 4 <= block`. That is the thin spine (~17). It is packed across blocks the same way the crit chain is held, so a quiet snapshot does not delete it.
2. **Operation.** `install_region_avoid` marks that ℓ protected and installs one-hop WaitOnce edges **before** execute on reuse. The arm peer stays 0, so a tx that is not in the writer list does not lose the OCC-shaped skip.
3. **Read.** On a protected ℓ the successor records an origin only after the pred is **`is_validated`**. `is_done` / an MvMemory Data tip from an unvalidated incarnation is not a safe version (that was the FullReplay after the in-frame wait). If the pred is still executing, the read parks `WaitForDependency` at k=0 (same incarnation) and the core returns to other work. A 4 ms spin covers only the validation window.
4. **WAR signal.** If a later learned writer of that ℓ exists, the read counts a WAR avoid: the origin is a lower validated version, not the later write. There is still no separate reader-reserve.

`estimate_block` is not used on this path (`park_publish_wait` / the validated spin do not increment it). `next_task*` is not restored.

## Discarded this round

| Lever | Result |
|---|---|
| Off-queue one-hop plant of the 17-writer spine | TPS **0.53 / 0.43**. Zero-replay thin iters were slower than the unplanted shell. Same failure mode as serialize-all-successors. |
| Scope floor 8 (armed `0x483a65c8…` n=8 on the large block) | `wait_for_dependency` exploded. Not the `abd6bb…` spine. |
| Treating execution-done as a publish the successor may read | Left in place for non-protected reads. Protected reads no longer return Ok on `is_done` alone. |

Not retried: Estimate Block as an Avoid gate, ordinal strip, Win Fence prepaid, 15-writer hold.

## Soft=0 Instant-off N=5

Primary metric is `occ_median_ms / sf_reuse_median_ms`. `seq=par` on both. SpecFence `occ_schedule_picks=0`, `soft_wait_arms=0`, `explore=0` on these samples.

### Final binary (sample A)

| Block | OCC ms | SF cold | SF reuse | TPS | vs ≥1.5 |
|------:|-------:|--------:|---------:|----:|:-------:|
| 3356896 | 1.380 | 2.187 | **2.202** | **0.63** | unmet |
| 15274915 | 5.834 | 12.326 | **6.956** | **0.84** | unmet |

3356896 reuse: full replay 3, 0, 0, 0 and walls 2.348, 1.491, 1.643, 2.202. The median iter is a full=0 shell at 2.202 ms.

15274915 reuse: full replay 6, 5, 7, 4; reexec entries 16, 15, 15, 14; `wait_for_dependency` 0 on three of four.

### Same binary, earlier sample (sample B)

| Block | OCC ms | SF reuse | TPS |
|------:|-------:|---------:|----:|
| 3356896 | 1.112 | 1.636 | 0.68 |
| 15274915 | 6.201 | 9.353 | 0.66 |

### Tip before this change (same host, same harness)

| Block | OCC ms | SF reuse | TPS |
|------:|-------:|---------:|----:|
| 3356896 | 0.973 | 1.507 | 0.65 |
| 15274915 | 5.304 | 10.627 | 0.50 |

Large full-replay on that baseline reuse was 56, 21, 61, 71. Sample A is lower. The host moves OCC by more than a millisecond between N=5 runs, so 0.84 is not a stable large TPS. Every sample is under 1.5.

Need, at sample A's OCC: thin SF ≤ 0.92 ms, large SF ≤ 3.89 ms. The best full=0 thin iter in sample A is **1.491 ms**.

## Four-class status (post-land)

| Class | 3356896 | 15274915 | Root if unsolved |
| RAW | yes (spine) | partial | Unchanged: large RAW is not what the reexec is paying for |
| WAW | **no** | **no** | The short region is learned and the read waits for a validated pred, but a pred that is still executing forces a k=0 reentry, and a full=0 thin shell (1.49–2.20 ms) is already slower than OCC/1.5 |
| WAR | **no** | **no** | The WAR counter fires when a later writer is in the learned list. There is no reader-reserve and no WAR edge list |
| Chain | partial | **no** | Thin wait is one hop at the read; the other workers are not pinned by an off-queue plant. Large L=77 is still the old crit hold, and the wall stays above OCC/1.5 |

## Smallest next cut

Do not plant the spine off the queue again. The missing piece is a **suspended EVM frame**: while the learned pred is executing, give the worker back to the antichain **without** dropping the successor's prefix, then continue that same frame once the pred is validated. Today's park is a new `execute` from k=0, so Avoid still pays a second entry. Staying in the frame until `is_done` was already measured (hot-key protect v4) and the read was not the one validation accepted — the release condition has to be `is_validated`, which this land enforces, but only across the short validation window.

If revm cannot suspend at the basic read, the thin bar is a shell problem: full=0 already misses OCC/1.5 on this host (best 1.491 ms vs a 0.92 ms need). Another park-kind or rem strip will not open that gap.
