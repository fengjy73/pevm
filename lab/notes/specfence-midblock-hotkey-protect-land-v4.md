# SpecFence mid-block hot-key protect land v4

**Date:** 2026-09-22  
**Tip:** `3332f9b` on `cursor/specfence-sf-ps-true-spine-d6e8` (PR #45)  
**SoT:** [`specfence-midblock-hotkey-protect-v1.md`](specfence-midblock-hotkey-protect-v1.md)  
**Previous land:** [`specfence-midblock-hotkey-protect-land-v3.md`](specfence-midblock-hotkey-protect-land-v3.md) (`94f103a`, thin **0.39** / large **0.53**)  
**Harness:** Soft=0 Instant-off (`env -u SPECFENCE_HANG_TRACE`), 8 cores, `SPECFENCE_COMPARE_CHECK=1`, N=5 both  
**Co-author:** `0xstride <fengjy73@users.noreply.github.com>`

---

## Verdict

**NOT MET — not a soft success below 1.5.** Large reuse median is under v1 0.70 and under v3. `replay_after` matches `full`. `reexec` stays well above `full`.

| Block | v1 protect | v3 cheap park | **this land reuse med TPS** | vs ≥1.5 |
|------:|-----------:|--------------:|----------------------------:|:-------:|
| 3356896 | 0.80 | 0.39 | **0.76** | gap ~2.0× |
| 15274915 | 0.70 | 0.53 | **0.40** | under v1 and v3 |

Headline ratio is `occ_median_ms / sf_reuse_median_ms`.

`seq=par`, SpecFence `occ_picks=0`, `soft_wait_arms=0`, `estimate_block_sf=0`, `explore=0`. No Estimate Block, no Win Fence prepaid, no ordinal strip, no off-queue serialize of every successor.

---

## What changed

v3's same-incarnation `WaitForDependency` woke the reader into a new interpreter entry from the first opcode. v4 tries to keep that prefix:

- A protected large read stays in the current execution while the unfinished writer is live, and continues at that access once true Data is published. The 12 ms cap only releases a ghost `Executing` status.
- A learned toucher of a protected location does not enter the interpreter until the nearer unfinished writer has published (`add_wait_for_dependency` before `execute`). Thin is not gated.
- ResumeAtK is not armed. For these hot reads `k` is 3–4, and that rem path does not jump the program counter.

---

## Soft=0 Instant-off N=5 @8 (`3332f9b`)

### 15274915 — reuse med TPS **0.40**

OCC median **5.215 ms**, SF reuse median **13.054 ms**. Need SF ≲ 5.215/1.5 ≈ **3.48 ms**.

| Iter | OCC | SF | TPS | full | replay_after | reexec | wfd | chain_ab/c | est |
|-----:|----:|---:|----:|-----:|-------------:|-------:|----:|-----------:|----:|
| 0 | 7.613 | 10.480 | 0.73 | 32 | 26 | 90 | 5 | 0/0 | 0 |
| 1 | 5.653 | 16.627 | 0.34 | 68 | 67 | 159 | 416 | 219/144 (1.52×) | 0 |
| 2 | 5.089 | 8.129 | 0.63 | 53 | 53 | 135 | 40 | 201/126 (1.60×) | 0 |
| 3 | 5.053 | 10.450 | 0.48 | 36 | 36 | 93 | 432 | 160/84 (1.90×) | 0 |
| 4 | 5.215 | 13.054 | 0.40 | 68 | 67 | 167 | 178 | 228/153 (1.49×) | 0 |

`pbo` on reuse is 174, 148, 124, 181. The reuse median is the 13.054 ms iter: `replay_after` 67 against `full` 68, `reexec` 167, `wait_for_dependency` 178.

### 3356896 — reuse med TPS **0.76**

OCC median **0.987 ms**, SF reuse median **1.305 ms**. Need SF ≲ **0.66 ms**.

| Iter | OCC | SF | TPS | full | replay_after | (c) | wfd | est |
|-----:|----:|---:|----:|-----:|-------------:|----:|----:|----:|
| 0 | 1.365 | 1.627 | 0.84 | 19 | 17 | 19 | 0 | 0 |
| 1 | 1.427 | 1.548 | 0.92 | 6 | 5 | 6 | 0 | 0 |
| 2 | 0.987 | 1.305 | 0.76 | 0 | 0 | 0 | 0 | 0 |
| 3 | 0.863 | 1.283 | 0.67 | 0 | 0 | 0 | 0 | 0 |
| 4 | 0.864 | 1.268 | 0.68 | 0 | 0 | 0 | 0 | 0 |

`wait_for_dependency` is 0. Calm reuse iters have `replay_after` 0 and SF about 1.27–1.30 ms. The upper median is iter 2.

---

## Honest gap

Staying in the frame until the named writer publishes does not make that read the one validation accepts. On every large reuse iter `replay_after` is `full` or one below it (67/68, 53/53, 36/36, 67/68). The in-exec wait therefore still ends in FullReplay, and `reexec` is 93–167 against `full` 36–68.

Admission does run: large `wait_for_dependency` is 40–432. Those parks keep learned touchers out of the interpreter until a nearer writer finishes, and the reuse wall moves from v3 **10.725 ms** to **13.054 ms**. Best large SF iter is **8.129 ms**, still above v1 **7.36 ms** and about 2.3× the 3.48 ms bar. `chain_ab/c` on reuse is 1.5–1.9×.

Thin pays neither the in-exec park nor the admission gate. Reuse median TPS **0.76** is above v3 **0.39** and still short of 1.5, with calm SF about 1.3 ms against a 0.66 ms bar.
