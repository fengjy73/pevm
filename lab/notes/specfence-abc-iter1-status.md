# SpecFence A+B+C Iter 1 status

**Date:** 2026-09-08 (Asia/Shanghai)  
**Branch:** `specfence`  
**Base:** `5d142fd` (docs) / code plateau `59754eb`  
**Authority:** `specfence-abc-unified-protocol.md`

## Mandate

Land A∧B∧C as **one** unified protocol (not sequential phases), SoftWait Soft ≪428 (prefer ~0), no Wait storms / SpecRead-through-writer / whole-block inspect hang. Then Iter 1 diagnose only.

## What landed (runtime, not docs-only)

### A — Avoidance at access grain
- Hot unfinished writer → **BlockingOther prefer-steal Await** until Executed/Validated, then Bind (not SoftWait Soft).
- Fence intent at consumer first-cross: `wave.set_pending_park(ℓ, armed_at_k, BlockingOther)`.
- Quiet: `force_prefix | sticky | prior_inc0` (prevent-first).
- Storm + program: also `hotset ∧ live_fanout_hot` (gated — abort-noise HotSet alone caused BO park wall↑; bisected).
- Cold ℓ: OCC-lite SpecRead (no π). SoftWait Soft stays **0**.

### B — Resolve ≠ FullRestart default
- **RebindOnly** when value-stable (unchanged).
- **SuffixRepair** first; hang-free absolute jump when `jump_is_safe` (+ optional TLS live attach); else journal-FF resume.
- Escalate **FullRestart** after `was_force_bind | depth≥2` (fb-loop break kept).
- After SuffixRepair (!escalate): **sticky** conflict ℓ for other consumers; `needs_live_capture` marked when write_replays / force_bind (capture-inspect *not* opened solely to prime — known 4× regression).
- Never ESTIMATE-poison certified prefix (ESTIMATE→SpecRead on incarn0 kept).

### C — Learning actuates
- **Inter:** `BlockEngagementMode::{Quiet,Storm}` from morph EMA (+ top-ℓ fanout≥16 storm nudge). Flip decay via existing InterBlockPrior α; mid-block `maybe_flip_mode` on live morph after abort / hot observe.
- **Intra:** `choose_action` / `note_observe` only on hot learn candidates (prior/hotset/sticky/force_bind/live fanout); cold SpecRead skips π.
- Mode changes **Await set** (A), not unused Bayes-only seed.

## Multi-block table (N=5 medians, @8 cores) — `abc-iter1b`

| Block | SF wall med | OCC wall med | SoftWait Soft | SF aborts med | notes |
|------:|------------:|-------------:|--------------:|--------------:|-------|
| **597** | **13.3** | 4.5 | **0** | 167 | ≈ plateau ~13; min 12.5; BO park ~8ms residual |
| **599** | 19.6 | 8.9 | **0** | 238 | ~2.2× OCC; flip smoke flip_count>0 |
| **097** | 12.6 | 5.8 | **0** | 217 | ~2.2× |
| **598** | **2.3** | 1.3 | **0** | 22 | not ≫ OCC×2 abs; quiet path OK |

Earlier widen (storm+hotset without fanout gate) regresssed 597 to **15.4** SoftWait=0 — bisected to BO park tax; tightened.

Flip 598→599: SoftWait=0; inter `flip_count` increments (mode/prior actuation path live).

Tests: `cargo test -p pevm --lib` **95 ok**; `--test specfence` **23 ok / 13 ignored**. No hang on 597/599.

## Iter 1 diagnosis (required answers)

### 1. detect / avoid / resolve — which hole now?
**Resolve still primary.** `force_bind_reabort ≈ full_restart ≈ 90` on 597 — repair bill remains an extra EVM incarnation.  
**Avoid** is no longer hollow on the sticky/force_prefix/prior path (BO Await + armed_at_k), but SoftWait Soft stays 0 by design; residual BO park (~8ms) is not free.  
**Detect** OK (conflicts surface; sticky after SuffixRepair feeds A).

### 2. Region / Fence / intra learn / inter learn — which broken?
| Axis | Verdict |
|------|---------|
| **Region** | Identity ℓ OK; event `a` now records `armed_at_k` on BO Await — still under-used for SoftWait Soft (dormant). |
| **Fence** | SoftWait Soft dormant (correct post-profile). BO Await fences exist at hot first-cross; not yet enough to cut FullRestart count. |
| **Intra learn** | **Wired** on hot candidates only; cold OCC-lite intact. Not yet driving cheaper resolve. |
| **Inter learn** | **Actuates mode** (Quiet/Storm) + top-ℓ HotSet seed; single-block G7 resets prior so storm often mid-block from abort morph — actuation real but weak on cold start. |

### 3. vs 59754eb plateau (~13ms SoftWait=0)
**In band:** 597 median **13.3** (plateau ~13±1). SoftWait Soft **0**. 598 not regressing into tax. Stretch &lt;10 **not met**.

### 4. Cause for Iter 2 (do not implement unless quick win)
**Named cause:** SuffixRepair still escalates to **FullRestart-class EVM** (~90/block on 597) because hang-free absolute jump almost never arms (`absolute_jump_applied=0` — no live snap on Lean Handler path without inspect tax). Sticky/BO Await reduces some first-cross races but does not remove the resume/reexec makespan.

**Iter 2 bet (falsifiable):** hang-free live snap **without** whole-block inspect / without 4× evm_entries — e.g. Storage+write_replay-only one-shot capture on first SuffixRepair resume, then **one** extra SuffixRepair that can `jump_is_safe` before fb escalate — target 597 median &lt;10 with SoftWait Soft still ≪50 and BO park_ns not above ~plateau.

## Artifacts
- `lab/results/abc-iter1b-sf-occ.json`, `abc-iter1b-flip.json`, `abc-iter1b-smoke5.run.log`
- `lab/results/abc-iter1-sf-occ.json` (pre-bisect widen, 597 med 15.4 — rejected)

## Code
- `engagement.rs` — `BlockEngagementMode` Quiet/Storm
- `learner.rs` — `note_hot_touch`, `is_hot_learn_candidate`, `live_fanout_hot`
- `vm.rs` — A Await + armed_at_k; C hot-only π; B jump-if-safe / journal-FF
- `pevm.rs` — inter mode set; sticky after SuffixRepair; morph flip on abort
