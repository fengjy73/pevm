# SpecFence overlap Avoid land v1

**Date:** 2026-09-22  
**Tip:** `a343abe` on `cursor/specfence-sf-ps-true-spine-d6e8` (PR #45)  
**SoT:** [`specfence-avoid-overlap-not-serialize-v1.md`](specfence-avoid-overlap-not-serialize-v1.md)  
**Prior falsified vehicle:** [`specfence-avoid-beats-occ-abort-land-v1.md`](specfence-avoid-beats-occ-abort-land-v1.md) (serialize-until-pred-done, large TPS **0.44**)  
**Floor:** land-v4 thin ~0.83 / large ~0.65  
**Harness:** Soft=0 Instant-off (`env -u SPECFENCE_HANG_TRACE`), 8 cores, `SPECFENCE_COMPARE_CHECK=1`, N=5 both  
**Co-author:** `0xstride <fengjy73@users.noreply.github.com>`

---

## Verdict

**NOT MET — not a soft success below 1.5.** Large reuse median also stays **under the v4 ~0.65 floor**.

| Block | v4 | serialize land-v1 | **this land reuse med TPS** | vs ≥1.5 |
|------:|---:|------------------:|----------------------------:|:-------:|
| 3356896 | 0.83 | 0.74 | **0.54** | gap ~2.8× |
| 15274915 | 0.65 | 0.44 | **0.55** | gap ~2.7× (under v4 floor) |

Headline ratio is `occ_median_ms / sf_reuse_median_ms`.

`seq=par`, `occ_picks=0`, `soft_wait_arms=0`, `estimate_block_sf=0`, `explore=0`. Sticky `chain_n=68` on the large block. WAR `war_ab` stays ahead of `war_c` on reuse. No ordinal strip, no Estimate Block, no Win Fence prepaid, no full-tx off-queue serialize as the Avoid.

---

## What shipped

Overlap, not “hold every sticky successor off `Q_*` until pred done”:

1. **Large seed:** a learned WAW successor stays on `Q_indep`. Thin still seeds those successors off-queue (v4).
2. **Access wait:** on the chain location, one core may poll ~400µs for true Data and then **continue the same exec**. If Data is not ready, the access parks and `chain_release` wakes it. `note_ungated_wait_on` is not planted from the consult path.
3. **Early Data:** once `write_set` is final (after rewards, before the rest of finish / scheduler Commit), the chain location is inserted into MvMemory and the chain tip is Released. That is the earliest safe value: validation checks writer incarnation, not the bytes, so an intermediate SSTORE must not be published.
4. **Counters:** `overlap` = in-exec resumes on that early Data; `overlap_fill` = other txs picked while the wait slot was held.

A 20 ms in-exec sleep was measured at `7c0f2e6` and dropped (large reuse TPS **0.50**, thin **0.64**). It pinned a core for the whole predecessor and lost to OCC’s parallel abort train. The sleep is not on this tip.

---

## Soft=0 Instant-off N=5 @8 (`a343abe`)

### 3356896 — reuse med TPS **0.54**

OCC median **0.921 ms**, SF reuse median **1.715 ms**.

| Iter | OCC | SF | TPS | (c) | est | overlap |
|-----:|----:|---:|----:|----:|----:|--------:|
| 0 | 1.877 | 1.552 | 1.21 | 20 | 0 | 0 |
| 1 | 0.881 | 1.181 | 0.75 | 0 | 0 | 0 |
| 2 | 1.165 | 1.715 | 0.68 | 11 | 0 | 0 |
| 3 | 0.921 | 1.300 | 0.71 | 0 | 0 | 0 |
| 4 | 0.826 | 4.046 | 0.20 | 8 | 0 | 0 |

`chain_n` 15–17 (below the sticky≥32 spine). Iter 4 SF **4.046 ms** sets the reuse median. Calm iters sit near SF 1.2–1.7 ms, still above OCC/1.5. No antichain-width lift to TPS≥1.5.

### 15274915 — reuse med TPS **0.55**

OCC median **5.121 ms**, SF reuse median **9.240 ms**. Need SF ≲ 5.121/1.5 ≈ **3.4 ms** for TPS≥1.5, and ≲ 5.121/0.65 ≈ **7.9 ms** to hold the v4 floor on this run’s OCC.

| Iter | OCC | SF | TPS | chain_ab/c | chain_n | overlap | fill | est |
|-----:|----:|---:|----:|-----------:|--------:|--------:|-----:|----:|
| 0 | 7.309 | 17.288 | 0.42 | — | 68 | 0 | 0 | 0 |
| 1 | 4.766 | 7.441 | 0.64 | 162/87 (1.86×) | 68 | 5 | 187 | 0 |
| 2 | 5.043 | 7.466 | 0.68 | 115/40 (2.88×) | 68 | 6 | 197 | 0 |
| 3 | 5.121 | 9.240 | 0.55 | 185/127 (1.46×) | 68 | 2 | 711 | 0 |
| 4 | 5.695 | 25.262 | 0.23 | 264/207 (1.28×) | 68 | 11 | 154 | 0 |

`overlap_fill` shows other transactions were picked while one hop held the access-wait slot (not a single-core walk of 68 hops). `chain_ab/c` stays about **1.3–2.9×**, not a ≥3× median, and SF wall is not ≤ OCC/1.5. Iter 4 FullReplay (full=102) is the wall outlier.

Early chain Data does fire (`early_tip` 195–444 on reuse; `overlap` 2–11 in-exec resumes). Most chain Avoids still happen because the predecessor has already published, not because the successor overlapped a long prefix and then continued.

---

## Why the cost inequality still fails

Publishing the chain value any earlier than journal finalize is unsafe: a later SSTORE or REVERT in the same incarnation would not change the read origin, and `seq≡par` would break. Finalize is the end of the predecessor’s EVM. The successor’s access wait therefore cannot finish in the middle of the predecessor’s bytecode unless it arrives in the last few hundred microseconds.

That window is real (`overlap` > 0) and other cores keep working (`overlap_fill` > 0). It does not remove the OCC abort train. Launching the sticky successors onto `Q_indep` lets them run, hit the unpublished chain read, park, and replay. That replay, plus the iter-4 storm, is more expensive than v4’s parallel Opt path.

---

## Invariants (held)

| Check | 3356896 | 15274915 |
|-------|:-------:|:--------:|
| seq≡par | ok | ok |
| occ_picks | 0 | 0 |
| soft_wait_arms | 0 | 0 |
| estimate_block_sf | 0 | 0 |
| explore | 0 | 0 |

No `lab/scripts/sync-to-github.sh` in this tree. The note is committed on the PR branch.
