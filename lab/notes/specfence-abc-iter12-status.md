# SpecFence A+B+C Iter 12 status

**Date:** 2026-09-08 (Asia/Shanghai)  
**Branch:** `specfence`  
**Base tip:** `b2692a0` (Iter11)  
**Authority:** `specfence-abc-unified-protocol.md`, `specfence-abc-iter11-status.md`, `specfence-global-cc-evm-rethink.md`

## Mandate

Cut SuffixRepair/FullRestart **opcode-seconds without abs jump**: strengthen successful
2nd repair (sticky BO) / reduce wasted resumes; serial-barrier evidence that cuts
ESTIMATE races without park tax; more RebindOnly / head-FF effectiveness; hang-free
opcode-skip credit only if no mass-path tax. SoftWait Soft~0; jump/capture OFF;
stock SSTORE; no Lean inspect/live_prime; no plant pre-sload warm.

Goal: 597 median &lt;12.7 toward &lt;10; Soft scarce; no hang; wall not regress.

## Root cause (diagnosed this iter)

1. **Bind-on-Executed ≠ Validated** — 2nd SuffixRepair / serial-barrier head reexec
   still SpecReads / Binds published Data that later aborts → ESTIMATE → wasted
   resume / fb_reabort. `is_done` covers Executed|Validated; no lock-free Validated gate.
2. **SpecRead-through-ESTIMATE on 2nd-repair force_prefix** — repair π always SpecReads
   ESTIMATE markers; on the 2nd SuffixRepair that immediately reaborts (opcode waste).
3. **Doomed 2nd SuffixRepair** — when fail-loc writers are ESTIMATE/Aborting but an
   Executing spine writer exists, another SuffixRepair burns interpreter-seconds that
   a serial-barrier FullRestart behind Data would avoid.
4. **Falsified this iter:** abort-path Validated/Estimate evidence spins (pevm validate
   critical section) and broad unfinished ESTIMATE→BO — **resume↓ but wall↑** (12→13.5+,
   12b/12c worse). First-repair Estimate park remains falsified (Iter8).

## Attack landed (production = `abc-iter12` = 12d lineage)

| Fix | Where |
|-----|--------|
| **Lock-free `is_validated`** (`validated_flags`) | `scheduler.rs` |
| **Validated-strict spin** (≤32, no park) after 2nd-repair prefer_await done | `vm.rs` |
| **2nd-repair force_prefix ESTIMATE→BO** only when writer **Executing** | `vm.rs` |
| **Doomed-2nd-repair escalate** when ESTIMATE/Aborting ∧ Executing spine → serial-barrier | `pevm.rs` |
| **Longer RebindOnly Estimate→Data spin** (72) when force_bind / ff_head | `pevm.rs` |
| Jump/capture OFF; stock SSTORE; SoftWait Soft~0 | unchanged |

### Measured / rejected this iter

| Trial | Result |
|-------|--------|
| Abort-path Validated + Estimate evidence spins + broad ESTIMATE→BO (`abc-iter12` early) | resume/fb/fr↓; 597 N=5 **13.5↑** — **falsified for wall** |
| Narrowed spins + Executing-only ESTIMATE BO (`abc-iter12b`) | 597 **14.1↑** wait_hard↑ — **falsified** |
| pevm-only evidence + minimal Validated (`abc-iter12c`) | 597 **14.8↑** — **falsified** |
| Validated prefer_await + Executing ESTIMATE BO + doomed escalate + rebind spin (`abc-iter12` prod / 12d) | **chosen** — wall↓; Soft=0 |
| Same without doomed escalate (`abc-iter12e`) | 597 **13.3** worse than 12d 13.0 / prod 12.5 |

## Multi-block table

### Primary: N=5 (`abc-iter12`)
| Block | SF wall med | OCC wall med | SoftWait Soft | hsstore | aj | notes |
|------:|------------:|-------------:|--------------:|--------:|---:|-------|
| **597** | **12.5** | 3.3 | **0** | 0 | 0 | **↓ vs Iter11 12.7** |
| **599** | **19.4** | 9.6 | **0** | 0 | 0 | ↓ vs Iter11 20.1 |
| **097** | **11.3** | 5.6 | **0** | 0 | 0 | **↓ vs Iter11 12.4** |
| **598** | **2.2** | 1.1 | **0** | 0 | 0 | quiet OK |

### Secondary: N=10 (`abc-iter12-n10`)
| Block | SF wall med | SoftWait | aj | vs Iter11 N10 |
|------:|------------:|---------:|---:|---------------|
| **597** | **14.0** | **0** | 0 | **↓ vs Iter11 14.2** |
| **599** | **19.7** | **0** | 0 | ↓ vs Iter11 20.6 |
| **097** | **11.7** | **0** | 0 | **↓ vs Iter11 12.4** |
| **598** | **2.1** | **0** | 0 | quiet OK |

Last-row dig (597 SF N=5): resume 105 (vs Iter11 114), fb_reabort 86 (vs 91),
sra 10 (vs 22), sb_res 21, fr 55 (doomed escalate trades some SuffixRepair for
serial-barrier FullRestart), rebind_only 0, SoftWait Soft **0**, aj=0, hsstore=0.
No hang.

Tests: `cargo test -p pevm --lib` **97 ok**; `--test specfence` **25 ok / 13 ignored**.

## Iter 12 diagnosis (required answers)

### 1. detect / avoid / resolve — which hole now?
**Resolve still primary for &lt;10**, but shape improved without abs jump: ESTIMATE races
on 2nd repair / serial-barrier path are gated (Validated spin + Executing-only
ESTIMATE→BO + doomed→barrier escalate). Wall **12.5 / 14.0** ≤ Iter11 **12.7 / 14.2**.
Abort-path busy-wait evidence spins cut resume counts harder but **regress wall** —
falsified. RebindOnly still scarce on 597 true_suffix value-changing RAW (rb≈0).
**Avoid** OK (SoftWait Soft=0; no SoftWait Soft arms). **Detect** OK.

### 2. Region / Fence / intra / inter
| Axis | Verdict |
|------|---------|
| **Region** | ℓ / `a` OK; Validated gate is tx-status evidence on fail-ℓ writers. |
| **Fence** | BO Await narrowed (Executing ESTIMATE on 2nd repair); SoftWait Soft dormant; abs-jump OFF. |
| **Intra** | Hot prefer_await Validated spin only on 2nd repair; no mass-path tax. |
| **Inter** | Quiet\|Storm gates doomed escalate (storm-only); Quiet 598 unaffected. |

### 3. vs Iter11 / plateau
- N=5: **12.5** ≤ Iter11 12.7; SoftWait Soft **0**; aj=0.
- N=10: **14.0** ≤ Iter11 14.2.
- 097/599 also ↓. Stretch &lt;10 unmet.
- Multi-SSTORE abs jump remains OFF (Iter11 falsified).

### 4. Cause for Iter 13 (named)
**Named cause:** Wall moved below Iter11 plateau without abs jump, but **successful
SuffixRepair still re-runs certified-prefix interpreter-seconds** (head-FF helps DB
skip only; rb≈0 on 597 value-changing fan-out). Doomed-escalate trades some resumes
for FullRestart — fr not collapsing. Remaining gap to &lt;10 needs a **hang-free
opcode cut on successful SuffixRepair** that does not re-enable abs jump / plant /
abort-path evidence spins:

1. **Certified-prefix opcode-skip credit** that is *not* PC restore — e.g. deeper
   journal FF hit-rate on Storage when origin incarnation bumps but value-stable
   *after* Validated gate (prior bare value-stable FF falsified Iter10; retry only
   with Validated evidence).
2. Or **stronger serial-barrier clique** that collapses fan-out FullRestarts into one
   spine reexec without sibling-park hang (Iter4 sibling park falsified).
3. Or prove a **narrow single-SSTORE / empty-memory-refused** jump subset with
   aj&gt;0∧seq≡par under pevm MV (multi-SSTORE still refused).
4. Do **not** re-enable abort-path Validated evidence spins, broad unfinished
   Estimate park, capture-without-jump, live_prime inspect, or SoftWait Soft 1.0.

## Artifacts
- `lab/results/abc-iter12-sf-occ.json`, `abc-iter12-flip.json`, `abc-iter12.run.log` (N=5)
- `lab/results/abc-iter12-n10-sf-occ.json`, `abc-iter12-n10-flip.json`, `abc-iter12-n10.run.log`
- Falsified: `abc-iter12b`, `abc-iter12c`, `abc-iter12e`; early broad-spin retained in logs only

## Code touched
- `scheduler.rs` — `validated_flags` / `is_validated`
- `vm.rs` — 2nd-repair Validated spin; Executing ESTIMATE→BO
- `pevm.rs` — doomed-2nd-repair escalate; longer force_bind/ff_head RebindOnly spin
- `mod.rs` — Iter12 blurb
