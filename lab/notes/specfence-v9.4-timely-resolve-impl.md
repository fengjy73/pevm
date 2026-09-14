# SpecFence v9.4 — timely Resolve land (Pin resume + R1 win)

**Date:** 2026-09-14  
**Parent:** `351f035` / `4b76dab` (PR #11) + why-slower autopsy  
**Posture:** correct the land. No SoT redesign. Soft=0. No P0/P1/P2.

## Why OCC was still faster

WAW/RAW/WAR beat OCC by **Detect → Avoid → Resolve at fail-a**, not by reincarnation.
The v9.4 welds moved **labels** (PinHold↑, Bind↓, refuse↑) while repair stayed OCC B0
and PinHold wake was **FullRetry** (`resume_count=0`, R1 **3/2439**).

```
PinHold park (no rem checkpoint, armed_at_k=0)
  → wake same-incarnation + try_arm_park_resume_at_k → FullRetry
    → head reexec + park idle OCC never pays
      → validate_specfence cert theater → validate_occ_kernel B0
```

## What this land does (existing files / fns)

| # | Duty | Where | Done when |
|---|------|-------|-----------|
| 1 | **PinWithoutThrow resumes** | Unfenced PE-on reads `maybe_note_value`. `arm_pinhold_checkpoint` journals **snapped** prefix only (wait loc not certified). First-access / empty / **k<8** → FullRetry (tiny ResumeAtK was 0.17/0.09 tax; synthetic k=1 hung 19807137). Wake ResumeAtK without force-bind only when prefix skip is real; `repair_armed` only with FF values | mid-tx k≥8 `park_resume_at_k`; Soft=0 |
| 2 | **R1 wins when strips cover** | R1a value-stable; R1b `try_arm_r1b_covered` strips-only, **one** RewindTo (`suffix_repair_depth==0`). No `r1_attempt` unless R1b arms. Second strip-cover → honest OCC B0 (never-B0 ForceBind hung 19807137) | R1 path not theater; no RewindTo train |
| 3 | **Schedule-first Avoid** | `scheduler.rs:try_execute_ready` refuses known consumers while `w` **Ready or Executing**; `admit_spine(w)` so ProducerStage progresses (no v6 yield-spin) | refuse on Ready; 19807137 refuse ≫ 0 |
| 4 | **Kill known-edge ReadyCanary** | `fence_act::act_wait_for`: Ready → PinHold; DoneUnfenced **cert=false** (no R1-bait strip) | canary only Aborting/unknown |
| 5 | **Storage true-k** | `admit_seed_begin_block` hint-fan (≥16-tx account) floor=2 + InterPrior storage PE; `vm::specfence_access_gate` plants Storage(addr,slot) PE at **live stream k** when Basic(addr) is a star | PE-on at real conflict ℓ |
| 6 | Soft=0 | No SoftWait Soft arm. PinHold / R1b journal FF only | `soft_wait_arms=0` |

Already OK / not re-landed: Bind rare EV; cert strip survival (M5); spine unity; dual-π delete.

## Honesty bars (must re-sweep)

Ship only if Soft=0 ∧ beat median **0.703** / fan **0.449** ∧ R1 win rate up materially
∧ abort_SF ≤ OCC on fan_out when PE+certs present. **Not** if only pin↑ / refuse↑.

This package is the plant. Sweep JSON is the verdict.
