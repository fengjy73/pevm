# SpecFence A+B+C Iter 18 status

**Date:** 2026-09-08 (Asia/Shanghai)  
**Branch:** `specfence`  
**Base tip:** `74649b7` (Iter17) + diagnosis blurb  
**Authority:** `specfence-abc-unified-protocol.md`, `specfence-abc-iter17-status.md`, `specfence-global-cc-evm-rethink.md`

## Mandate

Hang-free opcode skip on **successful** SuffixRepair / later RewindTo without
mass-path tax. Not fan BO park. Goal: 597 median **<12.3** toward <10; SoftWait
Soft=0; keep Iter12–17; Jump/capture OFF; stock SSTORE.

## Root cause (diagnosed this iter)

1. **True opcode cut still = abs jump / PC resume** — journal FF / head-FF only
   skip DB/MV. Interpreter still pays certified-prefix opcodes on every successful
   SuffixRepair resume.
2. **Handler plant+capture cannot arm jump on 597 RAW-read-fail** — SSTORE plant
   tips attach at `k ≥ k_fail` (fail is typically early SLOAD origin);
   `build_continuation` keeps `k < k_fail` → empty `jump_snap` → **aj=0** even with
   `hsstore>0`. Same family as Iter11 multi-SSTORE refuse / Iter13 JUMP=1 hang.
3. **Plant TLS is the wrong capture grain for RAW-read fails** — need a live snap at
   the **end of the certified prefix** (last Bind/EffectBoundary before `k_fail`),
   not post-SSTORE. Still open without WaitHard livelock.
4. **Non-TLS RewindTo/ff_head widens regress wall** — synthetic mid RewindTo,
   ForceBind ff_head, park FullRetry ff_head raised 599 (often 597); late-k
   yield192 wall↑; force_bind on FullRetry re-introduced BO park tax.

## Attack landed (production = Iter17 tip / `abc-iter18`)

| Fix | Where |
|-----|--------|
| **Diagnosis** — production runtime unchanged from Iter17 | tip + `mod.rs` blurb |
| Jump/capture OFF; SoftWait Soft~0; stock SSTORE; no fan BO park | unchanged |
| Keep Iter12–17 pieces | unchanged |

### Measured / rejected this iter

| Trial | Result |
|-------|--------|
| 18a: thin SSTORE wrap + capture + suffix_jump | hsstore>0, **aj=0** (snaps after k_fail) — **falsified as opcode cut** |
| 18b: synthetic mid RewindTo + ForceBind ff_head + late-k yield192 | 597 N5 **14.8↑** — **falsified** |
| 18c: ForceBind/park ff_head + force_bind | 597 **14.8↑**; 599 **35↑** — **falsified** |
| 18d: park FullRetry ff_head only | 597 N5 **12.6** but 599 **31↑** / N10 **29↑** — **falsified** |
| 18e: park ff_head gated k_fail≥8 | 597 **14.3↑** — **falsified** |
| Fan BO park / SoftWait Soft / sticky-absorb | **not re-tried** |

## Multi-block table

Machine load was high (~8–9 on 8 cores); report SoftWait Soft=0 everywhere and
wall band. Primary quiet-ish N=5: **r4 med 12.3**; N5×5 meta-median **12.8**.

### Primary: N=5 (`abc-iter18` / r4 + N5×5 band)
| Block | SF wall med | SoftWait Soft | aj | notes |
|------:|------------:|--------------:|---:|-------|
| **597** | **12.3** (band 11.5–19.6; meta-med **12.8**) | **0** | 0 | hits <12.3; stretch <10 unmet |
| **599** | **~18.5–20** | **0** | 0 | ~ Iter17 when quiet |
| **097** | **~12–13** | **0** | 0 | ~ |
| **598** | **~2.2–2.6** | **0** | 0 | quiet OK |

### Secondary: N=10 (`abc-iter18-n10`)
| Block | SF wall med | SoftWait | aj | vs Iter17 N10 |
|------:|------------:|---------:|---:|---------------|
| **597** | **12.6** | **0** | 0 | ≤ Iter17 **13.0** |
| **599** | **19.5** | **0** | 0 | ≤ Iter17 **21.3** |
| **097** | **13.0** | **0** | 0 | ~ |
| **598** | **2.4** | **0** | 0 | quiet OK |

Last-row dig: SoftWait Soft **0**, aj=0, no hang. Opcode cut not proven.

Tests: `cargo test -p pevm --lib` **97 ok**; `--test specfence` **25 ok / 13 ignored**.

## Iter 18 diagnosis (required answers)

### 1. detect / avoid / resolve — which hole now?
**Resolve still primary for <10.** Avoid (Iter17 yield-spin) at plateau; SoftWait
Soft=0. Opcode cut **absent**: plant/jump cannot skip prefix on RAW-read-fail
SuffixRepair; FF widen wall↑ on 599. Detect OK.

### 2. Region / Fence / intra / inter
| Axis | Verdict |
|------|---------|
| **Region** | ℓ / `a` OK; early-read `k_fail` makes post-SSTORE plant tips unusable for jump. |
| **Fence** | SoftWait Soft dormant; abs-jump OFF; BO park not widened. |
| **Intra** | Stock SSTORE mass path; sticky-absorb OFF. |
| **Inter** | Quiet 598 OK; Storm 597 plateau. |

### 3. vs Iter17 / plateau
- N10 **12.6** ≤ Iter17 **13.0**; N5 hits **12.3** under quieter samples. SoftWait Soft **0**; aj=0.
- Stretch <10 unmet. Opcode cut **not** proven. Production code = Iter17 tip.

### 4. Cause for Iter 19 (named)
**Named cause:** Successful SuffixRepair still re-interprets certified-prefix
opcodes because capture grain is wrong for RAW-read fails; FF/scheduling
alternates either do not cut opcodes or tax 599.

1. **Hang-free live snap at certified-prefix end** (last Bind/EffectBoundary before
   `k_fail`) without plant TLS / WaitHard livelock — then memory-lite jump with
   aj>0∧seq≡par on ERC-20 RAW-read-fail resumes.
2. Or **CallOutcome / read-boundary short-circuit** without mass-path SSTORE wrap
   and without whole-block inspect.
3. Or **critical-path schedule** that removes SuffixRepair from the 597 makespan
   chain without BO park / sticky-absorb / ff_head tax on 599.
4. Do **not** re-enable SoftWait Soft 1.0, fan_hot BO park (17a/f), ForceBind/park
   ff_head seed (18c–e), synthetic mid RewindTo (18b), late-k yield192,
   sticky-absorb (16a), capture-without-jump mass tax, or multi-SSTORE abs jump.

## Artifacts
- Prod: `lab/results/abc-iter18-sf-occ.json` (r4), `abc-iter18-n10-sf-occ.json`
- N5×5: `abc-iter18-r1`…`r5`
- Falsified: `abc-iter18b`…`18e`, plant/capture trial logs

## Code touched
- `specfence/mod.rs` — Iter18 diagnosis blurb (runtime = Iter17 tip)
