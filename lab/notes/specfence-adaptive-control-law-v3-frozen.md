# Adaptive control law v3 — FROZEN from plant-measured effect RAW

**Date:** 2026-09-07 (Asia/Shanghai)  
**Status:** FROZEN for next implementation (choose_action), not yet wired into production SpecFence  
**Authority:** `lab/notes/specfence-evm-cc-first-principles-from-effect-raw.md` + `lab/results/effect-raw-journal-stream.*`  
**Commit evidence tip:** `da89cbb`  
**Rejects:** chasing external RAW tables; gas/limit as primary depth; HotSet `H_w`/`H_a` as sole Wait gate; block-wide EarlyAbort@1%

---

## Measured facts (OCC@1 journal stream, location last-writer RAW)

| Block | RAW | prog/hand | morphology | gross-work p50 | opcode p50 |
|-------|----:|----------:|------------|---------------:|-----------:|
| 14689597 | 647 | 605/42 | fan-out ≈448 | **0.94** | 0.77 |
| 19606599 | 584 | 439/145 | mixed | 0.38 | 0.21 |
| 19469097 | 410 | 341/69 | longer chain | 0.61 | 0.71 |
| 19606598 | 44 | 32/12 | quiet contrast | — | — |
| 19469096 | 232 | 213/19 | long WAW spine | — | — |

- Instance edges (no pcl dedupe); mean/pcl≈1.09 on 597 is empirical.
- **≪1% gross-work discovery: absent** in sample.
- ~25 heavy consumers on 597: gw≈0.11 / opcode≈0.05 (morphology minority).
- M-A/M-D (597, gross-work): redo_saved≈427, wait_added≈108.

---

## Control law (per RAW edge at discovery)

Depth \(d = \texttt{gas\_at\_cross} / \texttt{tx\_gas\_used}\) (opcode fraction secondary).

At consumer discovery of edge \(e=(p\to c,\ell)\), class ∈ {program, handler}:

1. **Bind** if producer Data published (optional morphology prior agrees).  
2. Else if **program** and (fan-out prior high **or** \(d\) large): **WaitHard+park** (M2 steal) — EarlyAbort EV weak when \(d\gtrsim 0.9\).  
3. Else if **handler** / short-lag chatter: **SpecRead** unless abort@ℓ spikes.  
4. **EarlyAbort** only if morphology says heavy-tx **and** \(d\) small (order 0.1), not a block-wide 1% rule.  
5. Long **WAW** spine (19469096): prefer schedule/steal over WaitHard on every multi-writer basic.

HotSet / writer counts = **tracking cache only**, not the decision.

---

## Learning

**Intra-block:** update per-ℓ fan-out, class mix, running \(d\) histogram as edges appear; refresh EV.  
**Inter-block:** carry `{program_frac, handler_frac, expected_fanout, expected_program_path, gw_depth_prior, opcode_depth_prior, abort_rate}`; **decay on morphology flip** (598↔599).

---

## Next implementation (when authorized)

1. Wire `choose_action(e, d_gw, class, producer_ready, morphology)` replacing HotSet Wait gate.  
2. Keep default execute = `Handler::run` (no inspect).  
3. Smoke 14689597 + 19606598/599 + wide block; then sweep.  
4. Journal stream remains research measurement flag.

---

## Bottom line

True adaptive on **this** plant = **gross-work-aware per-edge EV**, Wait/Bind on fan-out program storms, SpecRead on handler chatter, morphology-conditioned EarlyAbort for the heavy minority — learned online and across contiguous blocks, not retuned thresholds.
