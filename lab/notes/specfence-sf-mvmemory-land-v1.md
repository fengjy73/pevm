# SpecFence SfMvMemory land v1

**Date:** 2026-09-22  
**Tip:** `5e88d67` on `cursor/specfence-sf-ps-true-spine-d6e8` (PR #45)  
**Harness:** Soft=0 Instant-off, 8 cores, `SPECFENCE_COMPARE_CHECK=1`, N=5 both  
**SoT:** [`specfence-sf-mvmemory-redesign-v1.md`](specfence-sf-mvmemory-redesign-v1.md), [`specfence-thin-avoid-no-estimate-v1.md`](specfence-thin-avoid-no-estimate-v1.md)  
**Acceptance:** **TPS SF/OCC ≥ 1.5** both `3356896` and `15274915`, Soft=0 Instant-off N≥5.  
**Co-author:** `0xstride <fengjy73@users.noreply.github.com>`

---

## Verdict

**Did not meet TPS ≥ 1.5.** SpecFence-native `SfMvMemory` + concurrent Detect|Avoid|Resolve path counters landed. Soft=0 Instant-off N≥5 both: `seq=par`, `occ_picks=0`, `estimate_block_sf=0`. Best calm TPS this host ≈ **0.78** (3356896, path-c=0 reuse) / ≈ **0.67** (15274915 sticky) — still ≪ **1.5**. Path **(c)** still fires on noisy reuse / cold; even when (c)=0, SF scaffolding tax keeps TPS below OCC.

Detect→Avoid→Resolve are **concurrent capabilities per access**, not a sequential pipeline.

---

## Concurrent Detect | Avoid | Resolve (semantic)

| Capability | When | SpecFence verb |
|------------|------|----------------|
| **(a) Detect** | Structure/prior says conflict coming — **before** the read | AccessArm WaitOnce + peer / wait edges / crit pred |
| **(b) Avoid** | Up front so collision never happens | WaitOnce + SfMvMemory true publish / done; thin spin; large park_publish_wait |
| **(c) Resolve** | Only when a **live mistake** is found | Prefix from fail_k (large Rewind); FullReplay if Avoid missed |

**Not:** Opt → validate → FullReplay theater as the common path.  
**Path (c) dominating = incomplete.** SfMvMemory redesign must make **(a)/(b)** the common path.

Counters (per block, Soft=0): `sf_detect_before_n`, `sf_avoid_publish_n`, `sf_resolve_after_fail_n` (+ `early_tip`, `est_block`).

---

## Early-WAW basic audit (focus ℓ)

### 3356896 — ℓ `dff71d59d972d654` (fail_k 5/6, RAW=0 pure WAW)

Soft=0 Instant-off N=5 @8, tip `5e88d67` (pass with best thin TPS):

| Iter | FullReplay | **(a)** detect | **(b)** avoid | **(c)** resolve | early_tip | est_block | Read |
|-----:|-----------:|---------------:|--------------:|----------------:|----------:|----------:|:-----|
| 0 cold | 9 | 20 | 19 | 9 | 9 | 0 | (c) ≈ FullReplay; (a)/(b) partially fire |
| 1 reuse | **0** | 18 | **18** | **0** | 0 | 0 | **(a)=(b), (c)=0** — Avoid held |
| 2 | 8 | 26 | 24 | 8 | 8 | 0 | (c) matches FullReplay |
| 3 | 8 | 28 | 25 | 8 | 8 | 0 | (c) matches FullReplay |
| 4 reuse | **0** | 20 | **20** | **0** | 0 | 0 | **(a)=(b), (c)=0** |

**Thin summary:** On calm reuse, Detect+Avoid succeed and **(c)=0**. Noisy reuse still takes Opt→FullReplay **(c)** at fail_k 5/6 (~8–24). **(c) does not dominate calm reuse**, but still dominates cold and noisy iters. Prefer ≥1.5 needs (c)≈0 **and** SF wall ≤ ~OCC/1.5 (calm SF ~1.41 vs OCC ~1.10 → TPS ~0.78 even with (c)=0).

### 15274915 — sticky spine ℓ `abd6bb3978815b97` (≥32)

| Iter | FullReplay | **(a)** | **(b)** | **(c)** | early_tip | est_block | Read |
|-----:|-----------:|--------:|--------:|--------:|----------:|----------:|:-----|
| 0 | 80 | 852 | 215 | 155 | 0 | 0 | (a)≫(b); (c) ≃ Full+Rewind |
| 1 | 64 | 309 | 215 | 130 | 0 | 0 | (c) still large |
| 2 | 76 | 335 | 203 | 148 | 0 | 0 | incomplete vs (a)/(b)-common |
| 3 | 70 | 358 | 238 | 141 | 0 | 0 | Rewind salvage ⊂ (c) |
| 4 | 90 | 365 | 232 | 169 | 0 | 0 | (c) still common |

**Large summary:** Detect (a) fires heavily (WaitOnce/crit consult). Avoid (b) via publish/done is real but **(c) Resolve-after-fail remains common** (FullReplay 64–90 + Rewind). Tip plane thin-only (`early_tip=0`). Sticky ≥32 + fail_k Rewind intact. **Path (c) still too common → incomplete for ≥1.5.**

---

## TPS tables (primary = reuse median)

TPS SF/OCC = OCC_wall / SF_wall. **Hard bar ≥1.5: NOT MET.**

### Paired Soft=0 Instant-off N=5 @8 (tip `5e88d67`)

| Pass | 3356896 OCC / SF / **TPS** | 15274915 OCC / SF / **TPS** | vs ≥1.5 |
|-----:|---------------------------:|----------------------------:|:-------:|
| 0 | 1.018 / 1.545 / **0.659** | 5.318 / 8.343 / **0.637** | gap ~2.3× |
| 1 | 0.915 / 1.632 / **0.561** | 5.651 / 14.954 / **0.378** | gap ~2.7–4× |
| 2 | 1.099 / 1.414 / **0.777** | 5.860 / 8.736 / **0.671** | gap ~1.9–2.2× |

Med TPS ≈ **0.66** / **0.64**. Best thin calm with (c)=0 still ≈ **0.78**. Target SF for ≥1.5: thin ≲0.67 ms, large ≲3.9 ms on this host — not reached.

### Invariants

| Check | 3356896 | 15274915 |
|-------|:-------:|:--------:|
| seq≡par | ok | ok |
| occ_picks | 0 | 0 |
| soft_wait_arms | 0 | 0 |
| explore (reuse) | 0 | 0 |
| estimate_block_sf | **0** | **0** |
| sticky ≥32 + Rewind | n/a | intact (Rewind≫0) |

---

## Call-graph (SF never Blocks on Estimate)

```
Detect (a) — concurrent, before read
  prior AccessArm WaitOnce + peer (now packed)
  plant_wait_edges → note_ungated_wait_on (thin begin)
  consult_ungated_wait_once → record_detect_before

Avoid (b) — concurrent, at read
  true_publish_ready / done → record_avoid_publish
  thin: Executing micro-spin; no Blocking park
  large: park_publish_wait (not estimate_block_sf)
  SfMvMemory WaitReleased|OrderedTip skip Estimate tips

Resolve (c) — only on live mistake
  FullReplay / PartialAbortRewind → record_resolve_after_fail
  large fail_k Rewind + prefix; thin Prefer Avoid-before-FullReplay

OCC baseline only: Estimate tip / park_estimate_blocking
```

---

## What shipped

1. `SfTipTable` / `SfMvMemory`: version tip, live_writer, exact wake, thin-only tip plane  
2. WaitOnce **peer persisted** in AccessArm prior (Detect before read on reuse)  
3. Path counters **(a)/(b)/(c)** + harness focus print  
4. Thin spin-only WaitOnce; large sticky hold (no equal-ℓ hop) + Rewind  
5. `estimate_block_sf=0`

---

## Remaining gaps blocking TPS ≥ 1.5

1. **(c) still common** on thin noisy reuse and large sticky (FullReplay tens–hundreds).  
2. **Even (c)=0 thin reuse** TPS ~0.78 — SF scaffolding tax > OCC remaining abort cost.  
3. Large tip plane off (protect sticky) → less SfMvMemory (b) on spine; park/Rewind carry Avoid/Resolve.  
4. Next levers (still no Estimate Block / thin Rewind / 15-hold / mark_gated / broad plant): stronger (a) edge coverage without serializing thin WAW; cut Soft=0 hot-path DashMap/metrics tax; optional narrow Data publish earlier for WaitOnce ℓ.

---

## Discarded (still)

Estimate Block as Avoid, thin Rewind/checkpoints, 15-writer hold, `mark_gated` broad plant, one-shot InconsistentRead, tip-install on large full WS, thin Blocking park.
