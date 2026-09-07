# SpecFence A+B+C Iter 7 status

**Date:** 2026-09-08 (Asia/Shanghai)  
**Branch:** `specfence`  
**Base tip:** `d47e17d` (Iter6)  
**Authority:** `specfence-abc-unified-protocol.md`, `specfence-abc-iter6-status.md`, `specfence-global-cc-evm-rethink.md`

## Mandate

Make **2nd SuffixRepair succeed more often** so Iter6's halved FullRestart stops wasting opcode-seconds on failed 2nd resumes. SoftWait Soft~0; no Lean inspect_run/live_prime; no empty-memory abs jump; keep head-FF + Iter6 RebindOnly/extra SuffixRepair depth. Storm fanout-Await strengthen stays falsified.

## Attack chosen (evidence-backed)

### Landed (production = `abc-iter7d` / N=10 tip)
1. **Sticky + force_bind-extend fail locs after first SuffixRepair fail** (`was_force_bind`) — next resume `force_prefix`-Awaits conflict ℓ until writers Executed/Validated. First repair keeps sticky-note only (early force_bind-extend wall↑).
2. **Storm ∧ Executing-writer BO park before 2nd resume** — `add_dependency_from_aborting` + `finish_validation_fenced_barrier_park`; metric `second_repair_await` (sra). Not SoftWait Soft; not storm-wide fanout Await (fail locs only; Executing-only hang-free).
3. **vm.rs 2nd-repair prefer_await** — when `suffix_repair_depth ≥ 1`, force_prefix|sticky always BO Await on unfinished Data (Validated gate).
4. **Memory-lite jump gate** — `suffix_jump_eligible` requires non-empty live `jump_snap.memory`; production jump still OFF without inspect/live_prime (aj=0). Empty-memory abs jump remains forbidden (seq≠par).

### Measured / rejected this iter
| Trial | Result |
|-------|--------|
| Broad unfinished (Ready/Aborting) park + first-repair force_bind-extend + long spins (`abc-iter7`) | **resume/fb↓** but 597 N=5 wall **13.5** (↑ vs Iter6 13.0); sra high |
| Executing-only + first-repair force_bind-extend (`abc-iter7b`) | fr/fb/resume↓ further; wall **13.9** (↑); 599 regress |
| Sticky-first + Executing park all modes (`abc-iter7c`) | 597≈13.1; **599 wall 21.9** (↑) |
| Storm-gated Executing park + sticky-first (`abc-iter7d`) | **chosen** — resume/fb/fr↓; wall≈plateau; 599≈Iter6 |

## Multi-block table

### Primary: N=5 (`abc-iter7d`)
| Block | SF wall med | OCC wall med | SoftWait Soft | SF aborts med | full_restart (last) | fb_reabort (last) | sra (last) | resume (last) | vs Iter6 N=5 |
|------:|------------:|-------------:|--------------:|--------------:|--------------------:|------------------:|-----------:|--------------:|--------------|
| **597** | **13.2** | ~3.7 | **0** | 158 | **45** (vs 49) | **90** (vs 105) | 24 | **138** (vs 155) | ≈ wall; **wasted resume↓** |
| **599** | 19.9 | ~10.0 | **0** | 215 | **52** (vs 69) | **90** (vs 119) | 22 | **117** (vs 135) | ≈ wall; fr/fb↓ |
| **097** | 12.4 | ~6.1 | **0** | 198 | 52 | 75 | 15 | 69 | ≈/slight↑ wall |
| **598** | 2.3 | ~1.3 | **0** | 14 | 2 | 4 | 0 | 5 | quiet OK |

### Secondary: N=10 (`abc-iter7-n10`)
| Block | SF wall med | SoftWait | aborts med | full_restart (last) | fb_reabort (last) | sra | vs Iter6 N=10 |
|------:|------------:|---------:|-----------:|--------------------:|------------------:|----:|---------------|
| **597** | **13.9** | **0** | 153 | **38** (vs 49) | **90** (vs 93) | 30 | **↓ wall; fr↓** |
| **599** | 19.7 | **0** | 214 | 53 | 86 | 15 | ↓ vs Iter6 20.4 |
| **097** | 13.6 | **0** | 204 | 41 | 72 | 12 | slight↑ |
| **598** | 2.5 | **0** | 14 | 2 | 3 | 0 | quiet OK |

`absolute_jump_applied=0`. SoftWait Soft **0**. No hang on 597/599.

Tests: `cargo test -p pevm --lib` **97 ok**; `--test specfence` **23 ok / 13 ignored**.

## Iter 7 diagnosis (required answers)

### 1. detect / avoid / resolve — which hole now?
**Resolve still primary, shape improved.** Sticky BO Await before 2nd SuffixRepair cuts **fb_reabort / resume / full_restart** (597 N=5 fb 105→90, resume 155→138, fr 49→45; N=10 fr 49→38). Wall stays near plateau (N=5 13.2≈13.0; N=10 **13.9↓** vs 14.0). Remaining cost = **successful SuffixRepair opcode-seconds + schedule**, not only wasted 2nd repairs.  
**Avoid** OK for this bet (SoftWait Soft=0; fail-loc sticky Await; storm fanout-Await not re-enabled).  
**Detect** OK.

### 2. Region / Fence / intra / inter
| Axis | Verdict |
|------|---------|
| **Region** | Fail-loc sticky/force_bind at ℓ / `a` OK; park is tx-dep on writer of fail ℓ. |
| **Fence** | BO Await (prefer-steal) at 2nd repair; SoftWait Soft dormant; no Lean jump. |
| **Intra** | Hot sticky + second_repair prefer_await; no SoftWait Soft arms. |
| **Inter** | Quiet\|Storm gates park (storm-only); Quiet 598 unaffected (sra=0). |

### 3. vs Iter6 / plateau
- N=5: **13.2** ≈ Iter6 13.0; **wasted resume/fb/fr↓**.  
- N=10: **13.9** ↓ vs Iter6 14.0; fr↓.  
- SoftWait Soft **0**. No hang. aj=0. Stretch &lt;10 unmet.

### 4. Cause for Iter 8 (named)
**Named cause:** 2nd-repair waste is reduced but **wall still ≈13–14** because even *successful* SuffixRepair resumes re-run suffix opcodes (and schedule). Memory-lite abs jump stays **unavailable** without hang-free non-empty memory snap (inspect/live_prime falsified). RebindOnly still scarce on true_suffix value-changing RAW.

**Iter8 bets (falsifiable):**
1. **Hang-free memory snap ≠ inspect_run** — capture non-empty memory at EffectBoundary without plant×Await livelock; then memory-lite jump can cut resume opcode-seconds (aj&gt;0 + seq≡par).
2. **RebindOnly on certified-prefix-only / Storage-stable invalid sets** after Estimate clears — absorb more fails without SuffixRepair.
3. **Cheaper successful SuffixRepair** — journal FF / write-prefix skip depth without Lean jump.
4. Do **not** re-enable storm-wide live_fanout Await, depth≥3, empty-memory jump, or broad Ready/Aborting parks without wall proof.

## Artifacts
- `lab/results/abc-iter7d-sf-occ.json`, `abc-iter7d-flip.json`, `abc-iter7d.run.log` (N=5 production)
- `lab/results/abc-iter7-n10-sf-occ.json`, `abc-iter7-n10-flip.json`, `abc-iter7-n10.run.log`
- Intermediate: `abc-iter7`, `abc-iter7b`, `abc-iter7c` (falsified tunings)

## Code touched
- `pevm.rs` — after first SuffixRepair fail: sticky + force_bind-extend; storm∧Executing BO park before 2nd resume (`second_repair_await`)
- `vm.rs` — second_repair prefer_await on force_prefix|sticky; memory-lite jump eligibility gate (production jump OFF)
- `metrics.rs` — `second_repair_await`
- `mod.rs` — resolve blurb
- `examples/specfence_g7_smoke.rs` — sra dig + recursion_limit
