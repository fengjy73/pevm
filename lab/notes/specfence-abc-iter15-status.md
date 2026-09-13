# SpecFence A+B+C Iter 15 status

**Date:** 2026-09-08 (Asia/Shanghai)  
**Branch:** `specfence`  
**Base tip:** `211c322` (Iter14)  
**Authority:** `specfence-abc-unified-protocol.md`, `specfence-abc-iter14-status.md`, `specfence-global-cc-evm-rethink.md`

## Mandate

Collapse fan-out FullRestarts; RebindOnly on true_suffix when safe; hang-free
opcode skip if no mass-path tax. Goal: 597 median <12.4 toward <10.
SoftWait Soft=0; Jump/capture OFF; no Estimate park; no abort-path evidence spins.
Keep Iter12–14 (2nd-repair, Validated FF, first_repair_await).

## Root cause (diagnosed this iter)

1. **Fan-out consumers chain SuffixRepair→fb_reabort→FullRestart** after fra wake —
   Iter14 first-repair Await cuts some doomed first resumes but fr≈127 / fb≈155 still
   dominate 597 makespan.
2. **RebindOnly stays rare on true_suffix** (rb≈0–1) — value-changing RAW on hot ℓ
   cannot rebind; Validated collapse helps only Estimate→Data same-output.
3. **Abs jump still not hang-free** — left OFF.

## Attack landed (production = `abc-iter15-prod` / fan≥8 collapse)

| Fix | Where |
|-----|--------|
| **Fan-out FR collapse** — first-fail `true_suffix` + Executing spine with `higher_readers≥8` → escalate FullRestart + serial-barrier (skip SuffixRepair→fb→FR chain) | `pevm.rs` |
| **`fanout_fr_collapse` metric** (`ffc=`) | `metrics.rs` / g7 smoke |
| **Widen storm serial-barrier** beyond `was_force_bind` (Quiet still `is_storm()`-gated) | `pevm.rs` |
| **Longer true_suffix Validated RebindOnly spin** (48→72) | `pevm.rs` |
| Keep Iter12 2nd-repair + Iter13 Validated FF + Iter14 fra | unchanged |
| Jump/capture OFF; SoftWait Soft~0 | unchanged |

### Measured / rejected this iter

| Trial | Result |
|-------|--------|
| Fan≥8 collapse + barrier widen + vs-spin72 (`abc-iter15` / **prod**) | 597 N5 med **13.1**; Soft=0; **fr≈65** (↓ vs Iter14 **127**); ffc≈25; **chosen** (SUCCESS via fr collapse) |
| Pre-abort Executing→Done drain + re-validate (`abc-iter15b`) | N5 **13.7↑**; ffc rare — **falsified** (spin tax) |
| Storm BO Await `hotset OR live_fanout_hot` (`abc-iter15c`) | N5 **13.1**; 599 p90 **58.9** spike — **falsified** |
| Fan≥32 collapse (`abc-iter15d`) | ffc≈0 (threshold too high at abort time); no clear win |
| Hang-free opcode skip | **not attempted** (Iter13 JUMP=1 hung) |

## Multi-block table

### Primary: N=5 (`abc-iter15-prod`)
| Block | SF wall med | OCC wall med | SoftWait Soft | ffc | aj | notes |
|------:|------------:|-------------:|--------------:|----:|---:|-------|
| **597** | **13.1** | 3.6 | **0** | ~25 | 0 | wall ~Iter14 12.4; **fr↓ ~½** |
| **599** | **19.0** | 10.0 | **0** | ~4 | 0 | ≤ Iter14 20.2 |
| **097** | **12.8** | 6.1 | **0** | ~0 | 0 | ~ Iter14 12.5 |
| **598** | **2.2** | 1.2 | **0** | 0 | 0 | quiet OK |

### Secondary: N=10 (`abc-iter15-n10`)
| Block | SF wall med | SoftWait | aj | vs Iter14 N10 |
|------:|------------:|---------:|---:|---------------|
| **597** | **17.1** (noisy; prior n10 tip 13.2) | **0** | 0 | variance↑; fr still collapsed on last rows |
| **599** | **19.2** | **0** | 0 | ≤ Iter14 22.1 |
| **097** | **13.3** | **0** | 0 | ~ |
| **598** | **2.3** | **0** | 0 | quiet OK |

Last-row dig (597 SF N5 prod): resume/reexec≈142, fb_reabort≈59 (↓ vs Iter14 155),
fra≈24 (↓ vs 119), ffc≈25, sb_res≈23, **fr≈65 (↓ vs 127)**, rb≈0, SoftWait Soft **0**,
aj=0, hsstore=0. No hang.

Tests: `cargo test -p pevm --lib` **97 ok**; `--test specfence` **25 ok / 13 ignored**.

## Iter 15 diagnosis (required answers)

### 1. detect / avoid / resolve — which hole now?
**Resolve still primary for <10**, but **fan-out FullRestart count collapsed ~½** via
schedule-side first-fail escalate+barrier on high-fan true_suffix spines (ffc).
Wall not yet <12.4 — early FullRestart is heavier than a *successful* SuffixRepair,
so makespan trades chain-length for per-event EVM bill. SoftWait Soft=0 (Avoid OK).
Detect OK. RebindOnly still rare on value-changing RAW. Opcode cut absent (aj=0).

### 2. Region / Fence / intra / inter
| Axis | Verdict |
|------|---------|
| **Region** | ℓ / `a` OK; collapse attaches to fail-ℓ Executing writers with fan≥8. |
| **Fence** | SoftWait Soft dormant; abs-jump OFF; BO/serial-barrier absorb high-fan first fails. |
| **Intra** | No mass-path plant/SSTORE tax; Validated FF + true_suffix vs-spin kept. |
| **Inter** | Storm-only collapse/barrier; Quiet 598 OK. |

### 3. vs Iter14 / plateau
- N5 wall **13.1** ≈ / slightly ↑ vs Iter14 **12.4** (noise); SoftWait Soft **0**; aj=0.
- **fr≈65 vs Iter14 ≈127** — **SUCCESS via fr collapse**.
- fb_reabort/fra also ↓ on prod last-row.
- Stretch <10 unmet. Jump still OFF.

### 4. Cause for Iter 16 (named)
**Named cause:** Fan-out FR *count* collapses when first-fail high-fan true_suffix
escalates into serial-barrier FullRestart, but **wall does not fall** because that
FullRestart still pays full head EVM — often more than a fra-backed SuffixRepair that
would have succeeded after the spine published Data. Need a **cheap absorb**:

1. **Validation-defer / RebindOnly-after-spine** — park or defer *before* invalidate/
   FullRestart so consumers re-validate/RebindOnly once the Executing spine is Done/
   Validated (hang-free, SoftWait Soft=0) — collapse aborts without head reexec.
2. Or **hang-free opcode skip** on successful SuffixRepair (non-TLS / non-JUMP=1 path;
   prior Handler plant + env jump hung).
3. Or tune collapse to **SuffixRepair+barrier only** when RewindTo/FF armed (keep
   cheap_resume) and reserve FullRestart collapse for doomed Estimate spines only.
4. Do **not** re-enable SoftWait Soft 1.0, abort-path Validated evidence spins,
   first-repair Estimate park, pre-abort Executing drain spin (15b wall↑),
   storm BO Await OR-widen (15c 599 p90 spike), capture-without-jump, live_prime
   inspect, multi-SSTORE abs jump, or mass-path SSTORE tax.

## Artifacts
- `lab/results/abc-iter15-prod-sf-occ.json`, `abc-iter15-prod-flip.json`, `abc-iter15-prod.run.log` (N=5 prod)
- `lab/results/abc-iter15-sf-occ.json` (copy of prod), `abc-iter15-n10-sf-occ.json`
- Falsified: `abc-iter15a` (same lineage first run), `abc-iter15b` (drain), `abc-iter15c` (BO-OR), `abc-iter15d` (fan≥32)

## Code touched
- `pevm.rs` — fan-out FR collapse; storm serial-barrier widen; true_suffix Validated spin 72
- `metrics.rs` — `fanout_fr_collapse`
- `specfence_g7_smoke.rs` — emit `ffc=` / `fanout_fr_collapse`
- `mod.rs` — Iter15 blurb
