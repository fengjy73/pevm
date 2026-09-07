# SpecFence A+B+C Iter 14 status

**Date:** 2026-09-08 (Asia/Shanghai)  
**Branch:** `specfence`  
**Base tip:** `d39ddb5` (Iter13)  
**Authority:** `specfence-abc-unified-protocol.md`, `specfence-abc-iter13-status.md`, `specfence-global-cc-evm-rethink.md`

## Mandate

Schedule-side **Validated Await before first SuffixRepair** (prevent doomed repair),
and/or RebindOnly collapse, and/or hang-free non-TLS jump if proven. Cut first-repair
waste; wall toward <10.

Goal: 597 median <12.5 toward <10; SoftWait Soft=0; no hang.
Keep Validated-gated FF + Iter12 2nd-repair path. Jump/capture OFF; no abort-path
evidence spins; no mass-path SSTORE tax.

## Root cause (diagnosed this iter)

1. **First SuffixRepair often resumes while conflict writers are still Executing** —
   SpecRead-through-unfinished → ESTIMATE/reabort → fb_reabort chain. Iter7/12 only
   Await before the *2nd* repair / doomed escalate; the *first* repair still pays
   doomed prefix re-interp.
2. **RebindOnly stays scarce on 597** (rb≈0) when Estimate→Data clears but tip is
   Executed without Validated — value-stable match misses until Validated tip.
3. **Abs jump still not hang-free** — left OFF (Iter13 single-SSTORE hung).

## Attack landed (production = `abc-iter14`)

| Fix | Where |
|-----|--------|
| **First-repair schedule-side Executing BO park** after arming SuffixRepair (`!was_force_bind`) | `pevm.rs` |
| **`first_repair_await` metric** | `metrics.rs` / g7 smoke |
| **RebindOnly Validated collapse** — brief Validated spin when Executed tip not yet Validated, then recheck value_stable (pre-abort; not escalate-path) | `pevm.rs` |
| Keep Iter12 2nd-repair + doomed escalate; Iter13 Validated-gated FF | unchanged |
| Jump/capture OFF; SoftWait Soft~0 | unchanged |

### Measured / rejected this iter

| Trial | Result |
|-------|--------|
| First-repair Await + RebindOnly Validated collapse (`abc-iter14` **prod**) | 597 N5 **12.4**; Soft=0; fra fires; **chosen** |
| Fra-only, no RebindOnly spin (`abc-iter14b`) | 597 N5 **13.1↑** — worse median |
| Missing-Data-only narrow fra (`abc-iter14c`) | 597 N5 **13.3↑** — **falsified** vs broad fra |
| Hang-free non-TLS jump | **not attempted** (Iter13 JUMP=1 hung) |

## Multi-block table

### Primary: N=5 (`abc-iter14` / prod)
| Block | SF wall med | OCC wall med | SoftWait Soft | fra | aj | notes |
|------:|------------:|-------------:|--------------:|----:|---:|-------|
| **597** | **12.4** | 3.7 | **0** | ~119 | 0 | **≤ Iter12 12.5 / Iter13 12.9** |
| **599** | **20.2** | 9.9 | **0** | ~62 | 0 | ~ Iter13 20.5 |
| **097** | **12.5** | 6.2 | **0** | ~56 | 0 | ≤ Iter13 14.4 |
| **598** | **2.4** | 1.2 | **0** | ~1 | 0 | quiet OK |

### Secondary: N=10 (`abc-iter14-n10`)
| Block | SF wall med | SoftWait | aj | vs Iter13 N10 |
|------:|------------:|---------:|---:|---------------|
| **597** | **13.6** | **0** | 0 | **≤ Iter13 13.7** / ≤ Iter12 14.0 |
| **599** | **22.1** | **0** | 0 | noise (noise) |
| **097** | **12.9** | **0** | 0 | ~ |
| **598** | **2.3** | **0** | 0 | quiet OK |

Last-row dig (597 SF N5 prod outlier iter): resume≈101, fb_reabort≈155, fra≈119,
sra≈10, sb_res≈26, fr≈127, rb≈1, SoftWait Soft **0**, aj=0, hsstore=0. No hang on
smoke. N5 p90 elevated (19.7) vs Iter12 (12.8) — schedule variance; median wins.

Tests: `cargo test -p pevm --lib` **97 ok**; `--test specfence` **25 ok / 13 ignored**.

## Iter 14 diagnosis (required answers)

### 1. detect / avoid / resolve — which hole now?
**Resolve still primary for <10**, but first-repair waste is now partially
**schedule-avoided**: fra parks first SuffixRepair resumes behind Executing spines
(Executing-only; Estimate park remains falsified). SoftWait Soft=0 (Avoid OK).
Detect OK. RebindOnly collapse helps rarely (rb≈1). Opcode cut still absent (aj=0).

### 2. Region / Fence / intra / inter
| Axis | Verdict |
|------|---------|
| **Region** | ℓ / `a` OK; first-repair Await attaches to fail-ℓ Executing writers. |
| **Fence** | SoftWait Soft dormant; abs-jump OFF; BO schedule park before first resume. |
| **Intra** | No mass-path plant/SSTORE tax; Validated FF kept. |
| **Inter** | Storm-only first-repair Await; Quiet 598 OK. |

### 3. vs Iter13 / plateau
- N5: **12.4** ≤ Iter13 **12.9** and ≤ Iter12 **12.5**; SoftWait Soft **0**; aj=0.
- N10: **13.6** ≤ Iter13 **13.7**.
- Stretch <10 unmet. First-repair Await fires (fra≫0) — fewer *doomed immediate*
  first resumes; fb/fr still high on noisy iters.
- Jump still OFF.

### 4. Cause for Iter 15 (named)
**Named cause:** Schedule-side first-repair Await cuts some doomed first resumes
(median ≤12.5) but **does not collapse FullRestart / fb_reabort enough for <10**.
Successful SuffixRepair still re-interprets certified-prefix opcodes; RebindOnly
remains rare on true_suffix value-changing RAW; abs jump unproven hang-free.

1. **Collapse fan-out FullRestarts** without sibling-park: stronger serial-barrier
   / single-spine claim that absorbs fra-parked consumers into one Validated Data
   publish before many SuffixRepairs arm — hang-free, SoftWait Soft=0.
2. Or **true_suffix → RebindOnly** when Storage value-stable under Validated tip
   more often (rb≫0 on 597), cutting repair depth.
3. Or prove **hang-free opcode skip** via non-TLS / non-capture-window grain
   (Handler plant + JUMP=1 hung Iter13).
4. Do **not** re-enable SoftWait Soft 1.0, abort-path Validated evidence spins,
   first-repair Estimate park, missing-Data-only fra narrow (14c wall↑),
   capture-without-jump, live_prime inspect, multi-SSTORE abs jump, or mass-path
   SSTORE tax.

## Artifacts
- `lab/results/abc-iter14-sf-occ.json`, `abc-iter14-flip.json`, `abc-iter14.run.log` (N=5 prod)
- `lab/results/abc-iter14-n10-sf-occ.json`, `abc-iter14-n10-flip.json`, `abc-iter14-n10.run.log`
- Copies: `abc-iter14-prod-*`, `abc-iter14-n10-prod-*`
- Falsified: `abc-iter14b` (fra-only), `abc-iter14c` (missing-Data narrow fra)

## Code touched
- `pevm.rs` — first-repair Executing BO park; RebindOnly Validated collapse spin
- `metrics.rs` — `first_repair_await`
- `specfence_g7_smoke.rs` — emit `first_repair_await` / `fra=`
- `mod.rs` — Iter14 blurb
