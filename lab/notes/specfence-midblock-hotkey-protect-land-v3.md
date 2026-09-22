# SpecFence mid-block hot-key protect land v3

**Date:** 2026-09-22  
**Tip:** `94f103a` on `cursor/specfence-sf-ps-true-spine-d6e8` (PR #45)  
**SoT:** [`specfence-midblock-hotkey-protect-v1.md`](specfence-midblock-hotkey-protect-v1.md)  
**Previous land:** [`specfence-midblock-hotkey-protect-land-v2.md`](specfence-midblock-hotkey-protect-land-v2.md) (`541cc4d`, thin **0.71** / large **0.54**)  
**Harness:** Soft=0 Instant-off (`env -u SPECFENCE_HANG_TRACE`), 8 cores, `SPECFENCE_COMPARE_CHECK=1`, N=5 both  
**Co-author:** `0xstride <fengjy73@users.noreply.github.com>`

---

## Verdict

**NOT MET — not a soft success below 1.5.** Large reuse median stays under the v1 0.70 floor. The same-incarnation wait did not bring the wall back, and `replay_after` is no longer well below `full`.

| Block | v1 protect | v2 unfinished | **this land reuse med TPS** | vs ≥1.5 |
|------:|-----------:|--------------:|----------------------------:|:-------:|
| 3356896 | 0.80 | 0.71 | **0.39** | gap ~3.9× |
| 15274915 | 0.70 | 0.54 | **0.53** | under v1 and v4 |

Headline ratio is `occ_median_ms / sf_reuse_median_ms`.

`seq=par`, SpecFence `occ_picks=0`, `soft_wait_arms=0`, `estimate_block_sf=0`, `explore=0`. No Estimate Block, no Win Fence prepaid, no ordinal strip, no off-queue serialize.

---

## What changed

v2 parked the protected read with `park_publish_wait`. A prefix shorter than 8 checkpoints makes that BlockingOther, which marks Aborting and reexecutes higher transactions. v3 keeps the unfinished-writer scan and the sticky protected WaitOnce.

- A protected large read polls 4096 times for true Data in the current execution.
- If the writer is still open, the read returns `Blocking` with `ParkKind::WaitForDependency` and `armed_at_k=0`. That park stays at the same incarnation. ResumeAtK is not recorded, so the k<8 prefix tax is not paid.
- The SpecFence executor honors that park before `chain_spine_schedule_defer`. The defer path recovered the reader to Ready and restarted it immediately; the pre-fix run (`895e6cb`) died with SIGABRT in `note_ungated_wait_on` during that restart. `94f103a` skips the defer for this park and drops a raced consumer entry.
- Thin still does not Blocking-park.

---

## Soft=0 Instant-off N=5 @8 (`94f103a`)

### 15274915 — reuse med TPS **0.53**

OCC median **5.710 ms**, SF reuse median **10.725 ms**. Need SF ≲ 5.710/1.5 ≈ **3.81 ms**.

| Iter | OCC | SF | TPS | full | replay_after | reexec | wfd | chain_ab/c | est |
|-----:|----:|---:|----:|-----:|-------------:|-------:|----:|-----------:|----:|
| 0 | 6.870 | 13.416 | 0.51 | 24 | 18 | 65 | 95 | 0/0 | 0 |
| 1 | 6.424 | 10.889 | 0.59 | 47 | 44 | 110 | 114 | 176/101 (1.74×) | 0 |
| 2 | 5.338 | 10.725 | 0.50 | 8 | 7 | 49 | 115 | 111/36 (3.08×) | 0 |
| 3 | 5.112 | 8.112 | 0.63 | 43 | 33 | 109 | 28 | 162/87 (1.86×) | 0 |
| 4 | 5.710 | 7.906 | 0.72 | 41 | 37 | 105 | 24 | 169/94 (1.80×) | 0 |

`pbo` on reuse is 295, 227, 145, 180. `inc>0` is 61, 42, 68, 64. The reuse median is the 10.725 ms iter: `replay_after` 7 against `full` 8, `reexec` 49, `wait_for_dependency` 115.

### 3356896 — reuse med TPS **0.39**

OCC median **0.893 ms**, SF reuse median **2.316 ms**. Need SF ≲ **0.60 ms**.

| Iter | OCC | SF | TPS | full | replay_after | (c) | wfd | est |
|-----:|----:|---:|----:|-----:|-------------:|----:|----:|----:|
| 0 | 1.836 | 1.648 | 1.11 | 16 | 15 | 16 | 0 | 0 |
| 1 | 0.918 | 2.316 | 0.40 | 9 | 8 | 9 | 0 | 0 |
| 2 | 0.839 | 2.414 | 0.35 | 25 | 23 | 25 | 0 | 0 |
| 3 | 0.893 | 1.339 | 0.67 | 0 | 0 | 0 | 0 | 0 |
| 4 | 0.822 | 1.226 | 0.67 | 0 | 0 | 0 | 0 | 0 |

`wait_for_dependency` is 0 on every thin iter. The upper median is iter 1 (2.316 ms). Iters 3–4 are calm (`replay_after` 0, SF 1.34 ms and 1.23 ms) and do not set the headline.

---

## Honest gap

The park is the cheap kind: large `wait_for_dependency` is 24–115 and the reader is not Aborting for that park. Waking still runs the transaction again from the start. `reexec` on reuse is 49–110, and `inc>0` stays 42–68 because validation FullReplay is still there. `replay_after` tracks `full` (7/8 on the median-wall iter, 44/47 on the storm iter). v2's calm iters had `replay_after` 0–2 against `full` 2–3 and `chain_c` 0. Those calm drops are gone; reuse `chain_c` is 36–101.

The 4096-spin poll is shorter than the writer's remaining execution, so the common case is the park. Holding the reader until that writer finishes puts the hot readers on the writer's completion. The reuse wall is **10.725 ms**, above v2 **10.211 ms** and v1 **7.361 ms**. Best large SF iter is **7.906 ms**, still about 2× the 3.81 ms bar.

Thin pays no WaitForDependency. Two reuse iters with `full` 9 and 25 pull the upper median to **2.316 ms** (TPS **0.39**), under v2 **1.431 ms**.
