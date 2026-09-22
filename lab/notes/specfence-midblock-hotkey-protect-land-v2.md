# SpecFence mid-block hot-key protect land v2

**Date:** 2026-09-22  
**Tip:** `541cc4d` on `cursor/specfence-sf-ps-true-spine-d6e8` (PR #45)  
**SoT:** [`specfence-midblock-hotkey-protect-v1.md`](specfence-midblock-hotkey-protect-v1.md)  
**Previous land:** [`specfence-midblock-hotkey-protect-land-v1.md`](specfence-midblock-hotkey-protect-land-v1.md) (`e72f229`, thin **0.80** / large **0.70**)  
**Harness:** Soft=0 Instant-off (`env -u SPECFENCE_HANG_TRACE`), 8 cores, `SPECFENCE_COMPARE_CHECK=1`, N=5 both  
**Co-author:** `0xstride <fengjy73@users.noreply.github.com>`

---

## Verdict

**NOT MET — not a soft success below 1.5.** Large reuse median is **under** the v1 0.70 and the v4 ~0.65 floor.

| Block | v1 protect | **this land reuse med TPS** | vs ≥1.5 |
|------:|-----------:|----------------------------:|:-------:|
| 3356896 | 0.80 | **0.71** | gap ~2.1× |
| 15274915 | 0.70 | **0.54** | gap ~2.8× |

Headline ratio is `occ_median_ms / sf_reuse_median_ms`.

`seq=par`, SpecFence `occ_picks=0`, `soft_wait_arms=0`, `estimate_block_sf=0`, `explore=0`. No Estimate Block, no Win Fence prepaid, no ordinal strip, no off-queue serialize.

---

## What changed

v1 resolved WaitOnce to the last **published** writer. v2 also names an **unfinished** writer between that tip and the reader:

- On abort, and again at execution start, a protected / WaitOnce / crit location is marked open for that writer until true Data `publish_data`.
- A protected read parks on the nearest open writer above the last finished tip (`park_publish_wait`, scheduler dependency). `decide`'s once-only Skip is not used for that park, so the second look does not Opt-read.
- A lower writer does not block once a higher known writer has already finished. The read origin is the higher tip.
- Sticky WaitOnce on a protected location is still kept across Learn `end_pack`.

---

## Soft=0 Instant-off N=5 @8 (`541cc4d`)

### 15274915 — reuse med TPS **0.54**

OCC median **5.468 ms**, SF reuse median **10.211 ms**. Need SF ≲ 5.468/1.5 ≈ **3.65 ms**.

| Iter | OCC | SF | TPS | full | replay_after | chain_ab/c | est |
|-----:|----:|---:|----:|-----:|-------------:|-----------:|----:|
| 0 | 7.540 | 28.139 | 0.27 | 16 | 11 | — | 0 |
| 1 | 6.475 | 10.737 | 0.60 | 45 | 44 | 194/119 (1.63×) | 0 |
| 2 | 5.263 | 10.211 | 0.52 | 3 | 1 | 109/34 (3.21×) | 0 |
| 3 | 5.468 | 7.076 | 0.77 | 2 | 0 | 76/0 | 0 |
| 4 | 5.223 | 7.126 | 0.73 | 3 | 2 | 76/0 | 0 |

`chain_n` 75–77. `pbo` on reuse is 249, 707, 27, 26.

Iters 3–4 are the evidence the park was aimed at: `replay_after` is 0 and 2 against `full` 2 and 3, and `chain_c` is 0. Iter 2 is `replay_after` 1 vs `full` 3 with `chain_ab/c` 3.21×, but `reexec=44` and the wall is **10.2 ms**. Iter 1 does not drop (`44/45`). The reuse median is that 10.2 ms iter.

### 3356896 — reuse med TPS **0.71**

OCC median **1.020 ms**, SF reuse median **1.431 ms**. Need SF ≲ **0.68 ms**.

| Iter | OCC | SF | TPS | full | replay_after | (c) | est |
|-----:|----:|---:|----:|-----:|-------------:|----:|----:|
| 0 | 1.816 | 1.666 | 1.09 | 15 | 13 | 15 | 0 |
| 1 | 0.874 | 1.631 | 0.54 | 1 | 0 | 1 | 0 |
| 2 | 1.493 | 1.430 | 1.04 | 0 | 0 | 0 | 0 |
| 3 | 1.020 | 1.431 | 0.71 | 9 | 8 | 9 | 0 |
| 4 | 0.959 | 1.347 | 0.71 | 2 | 1 | 2 | 0 |

Thin still does not Blocking-park. `replay_after` is low on iters 1–2 and 4, and the calm SF wall stays ~1.3–1.6 ms.

---

## Honest gap

The unfinished writer is visible, and on the calmer large iterations validation FullReplay after protect does fall (`replay_after` 0–2, `chain_c` 0). The park that creates that drop restarts the reader through the scheduler. Those restarts are `reexec` (44 on the median iter) even when `full` is 3. The reuse wall moves from v1 **7.36 ms** to **10.21 ms**. Best calm large SF is **7.08 ms**, still about 2× the 3.65 ms bar, and the median is worse than both v1 and v4.

`chain_ab/c` ≥ 3× on one reuse iter and infinite on two where `chain_c` is 0. It is not a median property of the run, and the wall is not ≤ OCC/1.5.
