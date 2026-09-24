# SpecFence A+B+C Iter 16 status

**Date:** 2026-09-08 (Asia/Shanghai)  
**Branch:** `specfence`  
**Base tip:** `162763c` (Iter15)  
**Authority:** `specfence-abc-unified-protocol.md`, `specfence-abc-iter15-status.md`, `specfence-global-cc-evm-rethink.md`

## Mandate

Fix early-FR wall tax after Iter15 fr collapse. Cheap absorb: validate-defer /
RebindOnly-after-spine without FullRestart, or hang-free opcode skip without
mass-path tax. Goal: wall ≤ Iter14 **12.4** toward <10; SoftWait Soft=0;
keep useful Iter12–15 pieces. Jump/capture OFF; no 15b drain; no 15c BO-OR.

## Root cause (diagnosed this iter)

1. **Iter15 early FullRestart collapse cuts fr≈½ but wall↑** — FR pays full head
   EVM; often heavier than a fra-backed SuffixRepair that would succeed after the
   Executing spine publishes Data.
2. **Naive SuffixRepair absorb (16a) wall↑↑ (N5 med 20.3)** — replacing FR with
   SuffixRepair re-armed sticky `note_sticky_resolve` → WaitHard/BO park tax
   (`park_ms` 9→29, `wait_hard` 56→138). Sticky was the hidden tax, not SuffixRepair.
3. **Validate-defer for true_suffix is unsafe** without invalidate (wrong Data
   poisons higher readers). Safe only for `!true_suffix` (RebindOnly grain);
   on 597 that path rarely arms (`fvd=0`).

## Attack landed (production = `abc-iter16b` / absorb-no-sticky)

| Fix | Where |
|-----|--------|
| **Fanout absorb** — first-fail true_suffix + fan≥8 Executing spine → SuffixRepair + fra (no FullRestart); **skip sticky** | `pevm.rs` |
| **Fanout FR collapse retained** only when Estimate/Aborting∧Executing (doomed resume) | `pevm.rs` |
| **Validate-defer plumbing** — `!true_suffix` Executed consumer parks behind Executing spine; wake re-queues Validation (same incarnation); cap 1/tx | `scheduler.rs`, `rem.rs` |
| Metrics `fanout_absorb` / `fanout_validate_defer` (`fab=` / `fvd=`) | `metrics.rs` / g7 smoke |
| Keep Iter12–15 (2nd-repair, Validated FF, fra, barrier widen, vs-spin72) | unchanged |
| Jump/capture OFF; SoftWait Soft~0 | unchanged |

### Measured / rejected this iter

| Trial | Result |
|-------|--------|
| SuffixRepair absorb + sticky (16a) | N5 **20.3↑↑**; park_ms↑ — **falsified** (sticky BO tax) |
| SuffixRepair absorb **no sticky** + doomed-only FR (`abc-iter16b` / **prod**) | 597 N5 med **12.3** Soft=0; ≤ Iter14 12.4 — **chosen** |
| Hang-free opcode skip | **not attempted** (Iter13 JUMP=1 hung; mass-path risk) |
| true_suffix validate-defer without invalidate | **rejected** (correctness poison) |

## Multi-block table

### Primary: N=5 (`abc-iter16b` / prod)
| Block | SF wall med | OCC wall med | SoftWait Soft | fab | ffc | aj | notes |
|------:|------------:|-------------:|--------------:|----:|----:|---:|-------|
| **597** | **12.3** | 4.7 | **0** | ~3 | ~13 | 0 | ≤ Iter14 **12.4**; ↓ vs Iter15 13.1 |
| **599** | **21.9** | 10.7 | **0** | 0 | ~2 | 0 | med ~Iter14 20.2; one p90 outlier 67 |
| **097** | **12.5** | 6.5 | **0** | 0 | ~1 | 0 | ~ Iter14 12.5 |
| **598** | **2.2** | 1.3 | **0** | 0 | 0 | 0 | quiet OK |

### Secondary: N=10 (`abc-iter16-n10`)
| Block | SF wall med | SoftWait | aj | vs Iter14 N10 |
|------:|------------:|---------:|---:|---------------|
| **597** | **13.5** | **0** | 0 | ≤ Iter14 13.6 / ≪ Iter15 noisy 17.1 |
| **599** | **19.8** | **0** | 0 | ≤ Iter14 22.1 |
| **097** | **12.5** | **0** | 0 | ~ |
| **598** | **2.1** | **0** | 0 | quiet OK |

Last-row dig (597 SF N5 prod): fr≈66 (≈ Iter15 65; ≪ Iter14 127), fab≈3,
ffc≈13, fra≈24, fb≈77, SoftWait Soft **0**, aj=0, fvd=0. No hang.

Tests: `cargo test -p pevm --lib` **97 ok**; `--test specfence` **25 ok / 13 ignored**.

## Iter 16 diagnosis (required answers)

### 1. detect / avoid / resolve — which hole now?
**Resolve still primary for <10**, but early-FR wall tax is **closed** on the
Executing-spine fan-out path: SuffixRepair+fra absorb without sticky restores
Iter14 wall (**12.3 ≤ 12.4**) while keeping most of Iter15's fr collapse on doomed
Estimate spines. SoftWait Soft=0 (Avoid OK). Detect OK. RebindOnly-after-spine
validate-defer armed but rare (`fvd=0`) on 597 true_suffix. Opcode cut absent (aj=0).

### 2. Region / Fence / intra / inter
| Axis | Verdict |
|------|---------|
| **Region** | ℓ / `a` OK; absorb attaches to fail-ℓ Executing writers with fan≥8. |
| **Fence** | SoftWait Soft dormant; abs-jump OFF; BO/fra absorb Executing spines; FR+barrier only for Estimate/Aborting doomed. |
| **Intra** | No mass-path plant/SSTORE tax; sticky skipped on absorb (critical). |
| **Inter** | Storm-only absorb/collapse; Quiet 598 OK. |

### 3. vs Iter14 / Iter15 / plateau
- N5 wall **12.3** ≤ Iter14 **12.4** and ↓ vs Iter15 **13.1**; SoftWait Soft **0**; aj=0.
- fr≈66 keeps Iter15-class collapse on doomed spines; fab absorbs Executing spines cheaply.
- Stretch <10 unmet. Jump still OFF.

### 4. Cause for Iter 17 (named)
**Named cause:** Wall is back at the Iter14 plateau (~12.3) with SoftWait Soft=0
and fr still collapsed vs Iter14, but **makespan hole remains resolve≠cheap**:
successful SuffixRepair still pays opcode-seconds; RebindOnly stays rare on
value-changing RAW (`rb≈0`); validate-defer never hits 597 true_suffix (`fvd=0`).

1. **Hang-free opcode skip on successful SuffixRepair** — non-TLS / non-JUMP=1
   path; prior Handler plant + env jump hung (Iter13). Need memory-safe skip
   without mass-path SSTORE tax.
2. Or **value-stable RebindOnly on true_suffix after fra wake** — once spine
   Validated, patch origins without suffix reexec when snap matches.
3. Or **schedule-side Await before first SpecRead** on hot-ℓ fan-out (BO until
   Validated) without SoftWait Soft 1.0 / 15c OR-widen.
4. Do **not** re-enable SoftWait Soft 1.0, abort-path Validated evidence spins,
   first-repair Estimate park, pre-abort Executing drain (15b), storm BO Await
   OR-widen (15c), sticky-on-absorb (16a wall↑), capture-without-jump,
   live_prime inspect, multi-SSTORE abs jump, or mass-path SSTORE tax.

## Artifacts
- `lab/results/abc-iter16b-sf-occ.json` / `abc-iter16-prod-*.json` (N=5 prod)
- `lab/results/abc-iter16-n10-sf-occ.json`
- Falsified: `abc-iter16a` (absorb+sticky)

## Code touched
- `pevm.rs` — fanout absorb (no sticky); doomed-only FR collapse; validate-defer gate
- `scheduler.rs` — `defer_validation_behind`; Executed wake → re-validate
- `rem.rs` — `validation_defer_count` / `try_claim_validation_defer`
- `metrics.rs` — `fanout_absorb`, `fanout_validate_defer`
- `specfence_g7_smoke.rs` — emit `fab=` / `fvd=`
- `mod.rs` — Iter16 blurb
