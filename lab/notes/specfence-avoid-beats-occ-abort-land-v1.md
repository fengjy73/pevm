# SpecFence Avoid-beats-OCC land v1

**Date:** 2026-09-22  
**Tip:** `1f7213e` on `cursor/specfence-sf-ps-true-spine-d6e8` (PR #45)  
**SoT:** [`specfence-avoid-beats-occ-abort-v1.md`](specfence-avoid-beats-occ-abort-v1.md)  
**Baseline hot path:** land-v4 / v5 confirmatory (`69864d9`) — thin ~0.74–0.83, large ~0.59–0.65  
**Harness:** Soft=0 Instant-off, 8 cores, `SPECFENCE_COMPARE_CHECK=1`, N=5 both  
**Co-author:** `0xstride <fengjy73@users.noreply.github.com>`

---

## Verdict

**NOT MET — not a soft success below 1.5.**

| Block | v4 best | **this land reuse med TPS** | vs ≥1.5 |
|------:|--------:|----------------------------:|:-------:|
| 3356896 | 0.83 | **0.74** | gap ~2.0× |
| 15274915 | 0.65 | **0.44** | gap ~3.4× (regression) |

`seq=par`, `occ_picks=0`, `estimate_block_sf=0`, four-class + WAR held. No Win OrderedAdmit prepaid, no ordinal strip, no Estimate Block, no rem/claim-wake micro-cut.

---

## What shipped (SoT §2, not a tax strip)

Schedule-native Avoid without Fence:

1. **Pick + heal:** ungated tx with `blocking_producer` (sticky nearest pred not done) stays `ST_WAIT`. Not pushed to `Q_indep`. Not `mark_gated` / Win prepaid.
2. **Large pick:** prefer `Q_released` then `Q_indep` (thin Indep-first unchanged).
3. **Learn:** after Learn, sticky ≥32 crit ℓ keeps **AccessArm WaitOnce** + wait edges. LocStrategy stays Opt (v5 Win HOLD not repeated).

---

## Soft=0 Instant-off N=5 @8 (`1f7213e`)

### 3356896 — reuse med TPS **0.74**

| Iter | OCC | SF | TPS | (c) | est |
|-----:|----:|---:|----:|----:|----:|
| 0 | — | 2.90 | 0.56 | 27 | 0 |
| 1 | — | 1.37 | 0.74 | **0** | 0 |
| 2 | — | 1.25 | 0.72 | **0** | 0 |
| 3 | — | 1.22 | 0.76 | 1 | 0 |
| 4 | — | 3.96 | 0.20 | 1 | 0 |

Calm iters still SF≳OCC. One reuse outlier SF~4.0 ms. Antichain width did not lift TPS to ≥1.5. Shell strip was not used.

### 15274915 — reuse med TPS **0.44** (worse than v4)

| Iter | SF ms | TPS | Full | chain_ab/c | chain_n | est |
|-----:|------:|----:|-----:|-----------:|--------:|----:|
| 0 | 21.6 | 0.45 | 165 | — | 68 | 0 |
| 1 | 17.1 | 0.47 | 33 | 168/97 (1.73×) | 68 | 0 |
| 2 | 30.0 | 0.30 | 48 | 169/106 (1.59×) | 68 | 0 |
| 3 | 25.3 | 0.31 | 37 | 157/82 (1.91×) | 68 | 0 |
| 4 | 17.2 | 0.44 | 12 | 102/27 (**3.78×**) | 68 | 0 |

OCC median ~8.1 ms. Reuse `chain_ab/c` median **~1.8×** (one iter ≥3×). Sticky `chain_n=68` held. WAR `war_ab≫war_c` on reuse.

---

## Why the cost inequality failed

Holding every sticky successor until the predecessor is **done** serializes ~68 hops. That Avoid is real (FullReplay fell, one iter `chain_ab/c` 3.78×) but **more expensive than OCC’s parallel abort train** (SF ~17–30 ms vs OCC ~8 ms).

SoT target: `cost(schedule Avoid) < cost(OCC abort)`. This batch proved the opposite for a fully serialized sticky spine: one core walks the hop chain while the abort-parallel OCC block finishes sooner.

Learn did **not** receive LocStrategy Win (prepaid path stays discarded). AccessArm WaitOnce was preserved. That is not sufficient for TPS≥1.5.

Thin: no antichain-width win large enough to clear OCC/1.5. Ceiling under ordinals + ~17-writer WAW remains ~0.7–0.8.

---

## Not reopened

Win HOLD / Fence prepaid, ordinal strip, Estimate Block, thin Rewind, mark_gated, long Chain spin, claim-wake-only, rem/vis/metrics micro-skip.

---

## Next (not this land)

Avoid must overlap the spine with antichain **without** launching the next hop before Data, and the wait must be cheaper than one OCC re-exec — a pure “succ off-queue until pred done” chain of length ≥32 loses on wall. Do not answer that with another shell strip.
