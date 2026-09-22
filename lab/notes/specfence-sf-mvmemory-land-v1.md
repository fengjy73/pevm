# SpecFence SfMvMemory land v1

**Date:** 2026-09-22  
**Tip:** `801c525+` on `cursor/specfence-sf-ps-true-spine-d6e8` (PR #45)  
**Harness:** Soft=0 Instant-off (`env -u SPECFENCE_HANG_TRACE`), 8 cores, `SPECFENCE_COMPARE_CHECK=1`, N=5 both  
**SoT:** [`specfence-sf-mvmemory-redesign-v1.md`](specfence-sf-mvmemory-redesign-v1.md), [`specfence-thin-avoid-no-estimate-v1.md`](specfence-thin-avoid-no-estimate-v1.md)  
**Acceptance:** **TPS SF/OCC ≥ 1.5** both `3356896` and `15274915`, Soft=0 Instant-off N≥5.  
**Co-author:** `0xstride <fengjy73@users.noreply.github.com>`

---

## Verdict

**Did not meet TPS ≥ 1.5.** Four-class Detect|Avoid|Resolve audit landed (RAW / WAR / WAW / Chain). Soft=0 Instant-off N≥5 both: `seq=par`, `occ_picks=0`, `estimate_block_sf=0`, `soft_wait_arms=0`. Best thin calm TPS ≈ **0.84** (reuse, path-c=0); reuse median ≈ **0.72**. Large reuse median ≈ **0.48** — Chain late ≈ Chain avoid; sticky Hold still incomplete vs ≥1.5.

Detect|Avoid|Resolve remain **concurrent capabilities per access across all four classes**, not a pipeline and not early-WAW-only.

---

## Four conflict classes (semantic)

| Class | Detect (a) | Avoid (b) timely | Late Resolve (c) |
|-------|------------|------------------|------------------|
| **RAW** | WaitOnce pred before storage read | true publish / done → read published write | FullReplay/Rewind when Opt saw Storage pre-state |
| **WAR** | higher readers of published ℓ | `enqueue_higher_revalidate` demote before stale Commit | FullReplay when write landed after Opt read without demote |
| **WAW** | WaitOnce / early-k / OrderedTip | tip Released + spin/park_publish_wait | Opt→FullReplay at fail_k |
| **Chain** | sticky≥32 crit nearest-pred | WaitOnce on crit + antichain fill | FullReplay/Rewind along spine when Avoid miss |

**Not:** fold WAR into schedule-only absorption.  
**Not:** Opt→FullReplay theater as the common path for any class.  
**Path (c) dominating a class = incomplete for that class.**

---

## Four-class audit (Soft=0 Instant-off N=5 @8)

### 3356896 — thin WAW spine ℓ `dff71d59d972d654` (RAW≈0)

| Iter | Full | (a) | (b) | (c) | raw_ab/c | war_ab/c | waw_ab/c | chain_ab/c | early_tip | est | TPS |
|-----:|-----:|----:|----:|----:|---------:|---------:|---------:|-----------:|----------:|----:|----:|
| 0 | 17 | 51 | 25 | 17 | 0/0 | 4/2 | 21/15 | 0/0 | 17 | 0 | 0.79 |
| 1 | 1 | 21 | 20 | 1 | 0/0 | 1/0 | 19/1 | 0/0 | 1 | 0 | 0.67 |
| 2 | 16 | 61 | 48 | 16 | 0/0 | 18/0 | 31/16 | 0/0 | 16 | 0 | 0.72 |
| 3 | **0** | 18 | **18** | **0** | 0/0 | 0/0 | **18/0** | 0/0 | 0 | 0 | 0.57 |
| 4 | **0** | 19 | 18 | **0** | 0/0 | 0/0 | **18/0** | 0/0 | 0 | 0 | **0.84** |

**Thin read:** Class mass is **WAW**. Calm reuse: `waw_ab=waw_c+detect`, **(c)=0**, WAR Avoid fires on noisy Commit. RAW/Chain idle (no sticky≥32). Even with (c)=0, SF wall ≳ OCC → TPS≪1.5 (scaffolding tax).

### 15274915 — sticky≥32 Chain ℓ `abd6bb3978815b97`

| Iter | Full | (a) | (b) | (c) | raw_ab/c | war_ab/c | waw_ab/c | chain_ab/c | early_tip | est | TPS |
|-----:|-----:|----:|----:|----:|---------:|---------:|---------:|-----------:|----------:|----:|----:|
| 0 | 48 | 385 | 224 | 75 | 5/0 | 102/2 | 148/73 | 0/0 | 0 | 0 | 0.65 |
| 1 | 92 | 739 | 373 | 175 | 16/1 | **218/1** | 16/6 | **177/167** | 0 | 0 | 0.49 |
| 2 | 83 | 644 | 336 | 159 | 19/2 | 147/0 | 18/8 | 166/149 | 0 | 0 | 0.46 |
| 3 | 178 | 1273 | 675 | 343 | 22/0 | **403/0** | 24/10 | 300/333 | 0 | 0 | 0.43 |
| 4 | 102 | 742 | 414 | 190 | 15/1 | 206/0 | 18/4 | 203/185 | 0 | 0 | 0.49 |

**Large read:** All four classes fire. **WAR Avoid is first-class** (`war_ab≫war_c`). **Chain late ≈ Chain avoid** — sticky nearest-pred helps but does not make (b) dominate. Tip plane stays thin-only (large crit tip tax regressed wall; Chain Avoid via park_publish_wait + nearest-pred). Path (c) still common → incomplete for ≥1.5.

---

## Call-graph (four-class × Detect|Avoid|Resolve)

```
RAW
  Detect: consult_ungated_wait_once (WaitOnce, k==0, not crit≥32)
  Avoid:  true_publish_ready / done → record_class_avoid(Raw)
  Late:   FullReplay Storage EffectiveWAW → record_class_late(Raw)

WAR  (NOT schedule-only absorption)
  Detect: Commit/publish → higher_readers_of(ℓ) nonempty
  Avoid:  prepare_revalidate + wake Revalidate → record_class_avoid(War)
  Late:   Opt read then peer-done without WaitOnce → record_class_late(War)

WAW
  Detect: WaitOnce early-k / crit <32 / OrderedTip
  Avoid:  tip Released (thin) / park_publish_wait (large) → class_avoid(Waw)
  Late:   FullReplay/Rewind Basic EffectiveWAW → class_late(Waw)

Chain (sticky≥32)
  Detect: install_crit_chain + plant_nearest_preds; consult crit_pred
  Avoid:  WaitOnce on crit + antichain fill remaining cores
  Late:   FullReplay/Rewind on crit ℓ → class_late(Chain)
  Tip:    thin WaitOnce/crit only (large tip DashMap tax discarded)

OCC baseline only: Estimate tip / park_estimate_blocking (estimate_block_sf≡0)
```

---

## TPS tables (primary = reuse median)

TPS SF/OCC = OCC_wall / SF_wall. **Hard bar ≥1.5: NOT MET.**

### Paired Soft=0 Instant-off N=5 @8 (post four-class tip)

| Block | OCC med / SF reuse med / **TPS** | vs ≥1.5 |
|------:|---------------------------------:|:-------:|
| 3356896 | 1.501 / 1.965 / **0.76** (best calm **0.84**) | gap ~1.8–2× |
| 15274915 | 7.589 / 15.851 / **0.48** | gap ~3× |

### Invariants

| Check | 3356896 | 15274915 |
|-------|:-------:|:--------:|
| seq≡par | ok | ok |
| occ_picks | 0 | 0 |
| soft_wait_arms | 0 | 0 |
| explore (reuse) | 0 | 0 |
| estimate_block_sf | **0** | **0** |
| sticky ≥32 + Rewind | n/a | intact (Rewind≫0; chain_ab>0) |

---

## What shipped (this land)

1. `SfConflictClass` + per-class avoid/late counters on `SfTipTable`  
2. Consult classifies WaitOnce → Raw|Waw|Chain; Resolve late maps FullReplay/Rewind  
3. **WAR Avoid first-class** in `enqueue_higher_revalidate` (Detect higher readers → demote)  
4. Harness focus print: `raw_ab/c war_ab/c waw_ab/c chain_ab/c`  
5. Tip plane remains thin WaitOnce/crit (large Chain via nearest-pred + park)  
6. `estimate_block_sf=0`

---

## Remaining gaps blocking TPS ≥ 1.5

1. **Thin:** calm (c)=0 still TPS ~0.7–0.8 — SF scaffolding tax > OCC abort cost.  
2. **Large:** Chain late ≈ avoid; FullReplay tens–hundreds; wall ~2× OCC.  
3. **RAW** volume low on focus pair (thin pure WAW; large RAW ab≪Chain/WAR) — redesign served, not the TPS bottleneck.  
4. Next levers (still no Estimate Block / thin Rewind / 15-hold / mark_gated / broad plant): raise Chain (b)/(c) ratio without tip tax; cut Soft=0 DashMap/metrics hot-path; keep WAR demote but coalesce fan-out.

---

## Discarded (still)

Estimate Block as Avoid, thin Rewind/checkpoints, 15-writer hold, `mark_gated` broad plant, one-shot InconsistentRead, tip-install on large full WS / large crit tip plane (tax), thin Blocking park.
