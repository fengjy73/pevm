# SpecFence A+B+C Iter 17 status

**Date:** 2026-09-08 (Asia/Shanghai)  
**Branch:** `specfence`  
**Base tip:** `d897edb` (Iter16)  
**Authority:** `specfence-abc-unified-protocol.md`, `specfence-abc-iter16-status.md`, `specfence-global-cc-evm-rethink.md`

## Mandate

Hang-free opcode skip / RebindOnly-after-fra on true_suffix / schedule Await
before SpecRead on hot unfinished writers. Goal: 597 median **<12.3** toward
<10; SoftWait Soft=0; keep Iter16 absorb-no-sticky; Jump/capture OFF; no
sticky-absorb; stock SSTORE mass path.

## Root cause (diagnosed this iter)

1. **597 RAW fan-out values change** — RebindOnly stays rare (`rb≈0..1`). Waiting
   longer for spine Validated (defer / long spin) cannot make mismatched snaps
   match; it only delays SuffixRepair.
2. **BO park Await on live_fanout≥8 wall↑** — same family as SoftWait Soft /
   Iter15c / sticky-absorb: park_ms and steal idle dominate any abort cut.
3. **Makespan hole remains successful SuffixRepair opcode-seconds** — Avoid can
   only shave discovery races; without hang-free resume skip, wall stays at the
   Iter14/16 plateau (~12–13).

## Attack landed (production = yield-spin Await / `abc-iter17`)

| Fix | Where |
|-----|--------|
| **Schedule Await before SpecRead** — storm+program + live_fanout≥8 + unfinished Data writer → yield-spin (64) + brief Validated spin **without** BlockingOther park | `vm.rs` |
| Keep Iter16 absorb-no-sticky / doomed-only FR / validate-defer !true_suffix / fra | unchanged |
| Jump/capture OFF; SoftWait Soft~0; stock SSTORE | unchanged |

### Measured / rejected this iter

| Trial | Result |
|-------|--------|
| 17a: live_fanout BO park + Validated gate (all inc) | N5 **14.4↑**; park_ms↑ — **falsified** |
| 17b: true_suffix validate-defer when !estimate_cleared | N5 **13.3↑**; fvd↑ rb still ~0 — **falsified** |
| 17c/prod: yield-spin no park | N5 **12.7–12.8**; Soft=0; ≈plateau / ≤ noisy Iter16 — **chosen** |
| 17d: true_suffix Executing yield-wait×192 for RebindOnly | N5 **14.4↑** p90 38 — **falsified** |
| 17f: inc0-only fan_hot BO park | N5 **14.2↑** — **falsified** |
| 17g: cut true_suffix vs_spin/rebind_spin | N5 **13.3** — no win — **falsified** |
| Hang-free opcode skip (jump/plant) | **not re-enabled** (Iter13 JUMP=1 hung; mass-path tax) |

## Multi-block table

### Primary: N=5 (`abc-iter17` / prod)
| Block | SF wall med | OCC wall med | SoftWait Soft | fab | fvd | aj | notes |
|------:|------------:|-------------:|--------------:|----:|----:|---:|-------|
| **597** | **12.8** | 3.7 | **0** | ~6 | 0 | 0 | ≈ Iter16 **12.3**; A/B cmp **12.7** vs noisy Iter16 15.7 |
| **599** | **20.7** | 9.7 | **0** | 0 | 0 | 0 | ≤ Iter16 21.9 |
| **097** | **12.4** | 6.0 | **0** | ~ | 0 | 0 | ~ Iter16 12.5 |
| **598** | **2.2** | 1.2 | **0** | 0 | 0 | 0 | quiet OK |

### Secondary: N=10 (`abc-iter17-n10`)
| Block | SF wall med | SoftWait | aj | vs Iter16 N10 |
|------:|------------:|---------:|---:|---------------|
| **597** | **13.0** | **0** | 0 | ≤ Iter16 **13.5** |
| **599** | **21.3** | **0** | 0 | ~ |
| **097** | **12.6** | **0** | 0 | ~ |
| **598** | **2.3** | **0** | 0 | quiet OK |

Last-row dig (597 SF N5 cmp): SoftWait Soft **0**, aj=0, rb≈1, fvd=0, fab≈9,
fra≈20, park_ms≈4.9 (no BO-park tax). No hang.

Tests: `cargo test -p pevm --lib` **97 ok**; `--test specfence` **25 ok / 13 ignored**.

## Iter 17 diagnosis (required answers)

### 1. detect / avoid / resolve — which hole now?
**Resolve still primary for <10.** Avoid trial (schedule Await before SpecRead)
via **yield-spin** is the only non-regressing shape; BO park / true_suffix defer /
long RebindOnly wait all wall↑. Detect OK. SoftWait Soft=0. Opcode cut absent
(aj=0). RebindOnly cannot absorb 597 RAW value changes.

### 2. Region / Fence / intra / inter
| Axis | Verdict |
|------|---------|
| **Region** | ℓ / `a` OK; yield-spin attaches to unfinished Data writer on hot fan ℓ. |
| **Fence** | SoftWait Soft dormant; abs-jump OFF; BO park **not** widened (falsified); yield-spin is hang-free Await substitute. |
| **Intra** | No mass-path plant/SSTORE tax; sticky-absorb stays OFF. |
| **Inter** | Storm-only yield-spin; Quiet 598 OK. |

### 3. vs Iter16 / plateau
- N5 wall **12.7–12.8** ≈ Iter16 **12.3** (machine noise; A/B: 12.7 ≤ Iter16-cmp 15.7).
- N10 **13.0** ≤ Iter16 **13.5**. SoftWait Soft **0**; aj=0.
- Stretch <10 unmet. Jump still OFF.

### 4. Cause for Iter 18 (named)
**Named cause:** Avoid-without-park is at the plateau; RebindOnly is structurally
dead on 597 value-changing RAW; remaining makespan is **successful SuffixRepair
interpreter-seconds**.

1. **Hang-free opcode skip on successful SuffixRepair** — non-TLS / non-JUMP=1
   path that is seq≡par under pevm MV (CallEntry-only? single-SSTORE memory-lite
   with proven aj>0); prior Handler plant + env jump hung (Iter13).
2. Or **certified-prefix / FF density↑ without mass-path SSTORE wrap** so RewindTo
   `k_fail` lands later (fewer resume opcodes) — without capture tax.
3. Or **serial clique / write-set barrier** that cuts fan-out resume storms without
   early-FR wall tax (Iter15) or sticky-absorb (16a).
4. Do **not** re-enable SoftWait Soft 1.0, fan_hot BO park (17a/f), true_suffix
   validate-defer (17b), long true_suffix RebindOnly wait (17d), sticky-on-absorb
   (16a), 15b drain, 15c BO-OR, capture-without-jump, live_prime inspect,
   multi-SSTORE abs jump, or mass-path SSTORE tax.

## Artifacts
- `lab/results/abc-iter17-sf-occ.json` / `abc-iter17-prod-*.json` (N=5 prod)
- `lab/results/abc-iter17-n10-sf-occ.json`
- A/B: `abc-iter16-cmp-*` vs `abc-iter17-cmp-*`
- Falsified: `abc-iter17a`..`17g` (except prod 17c family)

## Code touched
- `vm.rs` — Iter17 yield-spin Await before SpecRead/Bind on hot unfinished writers
- `specfence/mod.rs` — Iter17 blurb
