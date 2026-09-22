# SpecFence mid-block hot-key protect land v1

**Date:** 2026-09-22  
**Tip:** `e72f229` on `cursor/specfence-sf-ps-true-spine-d6e8` (PR #45)  
**SoT:** [`specfence-midblock-hotkey-protect-v1.md`](specfence-midblock-hotkey-protect-v1.md)  
**Prior falsified vehicles:** serialize-until-pred-done (large TPS **0.44**), overlap access-wait (**0.54 / 0.55**)  
**Floor:** land-v4 thin ~0.83 / large ~0.65 (`230d0a0`)  
**Harness:** Soft=0 Instant-off (`env -u SPECFENCE_HANG_TRACE`), 8 cores, `SPECFENCE_COMPARE_CHECK=1`, N=5 both  
**Co-author:** `0xstride <fengjy73@users.noreply.github.com>`

---

## Verdict

**NOT MET — not a soft success below 1.5.**

| Block | v4 | overlap land | **this land reuse med TPS** | vs ≥1.5 |
|------:|---:|-------------:|----------------------------:|:-------:|
| 3356896 | 0.83 | 0.54 | **0.80** | gap ~1.9× |
| 15274915 | 0.65 | 0.55 | **0.70** | gap ~2.1× |

Headline ratio is `occ_median_ms / sf_reuse_median_ms`.

Large is back above the v4 ~0.65 floor (overlap was 0.55). Thin is near the v4 ~0.83 shell. Neither clears 1.5. After the first protect, later FullReplay on that location does **not** drop: on the large block `replay_after` equals `full` on every reuse iter.

`seq=par`, SpecFence `occ_picks=0`, `soft_wait_arms=0`, `estimate_block_sf=0`, `explore=0`. No Estimate Block, no Win Fence prepaid, no ordinal strip, no thin Rewind, no off-queue serialize, no overlap condvar.

---

## What shipped

Schedule restored to the land-v4 hot path (overlap condvar and the serialize-until-done hold are gone). On the first non-lazy validation conflict for `ℓ`:

1. AccessArm becomes **WaitOnce** for the rest of the block. Arm `peer` stays 0, so thin txs that do not touch `ℓ` keep the OCC-shaped skip.
2. Prior-block touchers of that `ℓ` get **per-consumer** WaitOnce edges. Consult resolves the predecessor from the crit chain, then the arm peer, then that edge, then `last_writer_before` (including thin, only when `ℓ` is protected).
3. On a large block, a protected read whose ordered predecessor has not published parks through the existing scheduler dependency (`park_publish_wait`). That path does not increment `estimate_block_sf`. Thin still micro-spins and then Opt-falls; Soft=0 does not Blocking-park the thin shell.
4. Sticky ≥32 WaitOnce is re-pinned after Learn `end_pack`. LocStrategy Win prepaid stays off.

An earlier tip (`ed64494`) armed WaitOnce with peer 0 and did not resolve a predecessor. Large `pbo=0` and `replay_after` tracked `full` (reuse med TPS **0.66 / 0.64**). `e72f229` is the tip that actually consults (`pbo` hundreds on large).

---

## Soft=0 Instant-off N=5 @8 (`e72f229`)

### 3356896 — reuse med TPS **0.80**

OCC median **1.046 ms**, SF reuse median **1.307 ms**. Need SF ≲ 1.046/1.5 ≈ **0.70 ms**.

| Iter | OCC | SF | TPS | full | (c) | protect | pbo | replay_after | est |
|-----:|----:|---:|----:|-----:|----:|--------:|----:|-------------:|----:|
| 0 | 1.638 | 2.468 | 0.66 | 17 | 17 | 1 | 28 | 16 | 0 |
| 1 | 1.213 | 1.307 | 0.93 | 1 | 1 | 1 | 10 | 0 | 0 |
| 2 | 0.901 | 1.537 | 0.59 | 15 | 15 | 5 | 32 | 12 | 0 |
| 3 | 1.046 | 1.133 | 0.92 | 0 | 0 | 0 | 0 | 0 | 0 |
| 4 | 0.898 | 1.230 | 0.73 | 0 | 0 | 0 | 0 | 0 | 0 |

`chain_n` 14–17. Iters 3–4 are calm (`resolve_c=0`) at SF **1.13–1.23 ms**, still above OCC/1.5. `chain_ab/c` is 0 on this short chain.

### 15274915 — reuse med TPS **0.70**

OCC median **5.143 ms**, SF reuse median **7.361 ms**. Need SF ≲ 5.143/1.5 ≈ **3.43 ms**.

| Iter | OCC | SF | TPS | full | chain_ab/c | protect | pbo | replay_after | est |
|-----:|----:|---:|----:|-----:|-----------:|--------:|----:|-------------:|----:|
| 0 | 6.458 | 18.672 | 0.35 | 69 | — | 10 | 405 | 63 | 0 |
| 1 | 5.071 | 18.556 | 0.27 | 74 | 192/147 (1.31×) | 4 | 305 | 72 | 0 |
| 2 | 5.352 | 7.361 | 0.73 | 38 | 167/92 (1.82×) | 4 | 177 | 37 | 0 |
| 3 | 5.143 | 7.043 | 0.73 | 32 | 145/71 (2.04×) | 3 | 140 | 32 | 0 |
| 4 | 5.040 | 7.296 | 0.69 | 28 | 168/93 (1.81×) | 3 | 160 | 28 | 0 |

`chain_n=62`. `wait_for_dependency=0` on every iter. Reuse `chain_ab/c` is **1.3–2.0×**, not ≥3×. Best reuse SF wall is **7.04 ms**, about 2× the 3.43 ms bar. WAR `war_c` stays 0 while `war_ab` is 52–154.

---

## Honest gap

Protect runs at **validation**, after the read that discovered the conflict. `pbo` shows later reads of a protected `ℓ` do enter consult before the Opt read (large reuse 140–305). They still fail validation: `replay_after` matches `full` (32/32, 28/28, 37/38, 72/74). Avoid is ahead of Resolve by about 2–3× overall, which is not **b ≫ c**, and the chain class stays under 3×.

The ordered predecessor is the last writer already in MvMemory, or the prior-block chain edge installed at the first conflict. A toucher between that predecessor and the reader, who has not written yet, is invisible. The read proceeds against a published earlier tip, then aborts when the in-between write lands. `wait_for_dependency` staying 0 means the scheduler resume path is not what is cutting these aborts; the FullReplay train is still the Resolve.

Thin calm iters already have `(c)=0`. Their wall is the v4 shell (~1.1–1.2 ms) against an OCC median of ~1.0 ms. Mid-block WaitOnce on a 14–17 hop chain does not move that shell under ~0.70 ms.

Large reuse **0.70** holds the v4 floor and beats overlap **0.55** and serialize **0.44**. The remaining gap to 1.5 is a wall cut from ~7.4 ms to ~3.4 ms while `replay_after` is still the whole FullReplay count. This land does not make that cut.
