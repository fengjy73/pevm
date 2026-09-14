# SpecFence v9.4 — timely Resolve land (WaitForDependency resume + partial_abort win)

**Vocabulary:** [`specfence-cc-glossary.md`](specfence-cc-glossary.md)  
**Date:** 2026-09-14  
**Parent:** `351f035` / `4b76dab` (PR #11) + why-slower autopsy  
**Posture:** correct the land. No SoT redesign. Soft=0. No P0/P1/P2.

## Why OCC was still faster

WAW/RAW/WAR beat OCC by **Detect → Avoid → Resolve at fail-a**, not by reincarnation.
The v9.4 welds moved **labels** (WaitForDependency↑, OrderedAdmit↓, refuse↑) while repair stayed OCC full_abort_reexecute
and WaitForDependency wake was **FullAbortReexecute** (`resume_count=0`, partial_abort **3/2439**).

```
WaitForDependency park (no rem checkpoint, armed_at_k=0)
  → wake same-incarnation + try_arm_park_resume_at_k → FullAbortReexecute
    → head reexec + park idle OCC never pays
      → validate_specfence cert theater → validate_occ_kernel full_abort_reexecute
```

## What this land does (existing files / fns)

| # | Duty | Where | Done when |
|---|------|-------|-----------|
| 1 | **wait_for_dependency resumes** | OptimisticRead PE-on reads `maybe_note_value`. `arm_wait_for_dependency_checkpoint` journals **snapped** prefix only (wait loc not certified). First-access / empty / **k<8** → FullAbortReexecute (tiny ResumeAtK was 0.17/0.09 tax; synthetic k=1 hung 19807137). Wake ResumeAtK without force-ordered_admit only when prefix skip is real; `repair_armed` only with FF values | mid-tx k≥8 `park_resume_at_k`; Soft=0 |
| 2 | **partial_abort wins when strips cover** | PartialAbortRebind value-stable; PartialAbortRewind `try_arm_partial_abort_rewind` strips-only, **one** RewindTo (`suffix_repair_depth==0`). No `partial_abort_attempt` unless PartialAbortRewind arms. Second strip-cover → honest OCC full_abort_reexecute (never-full_abort_reexecute ForceOrderedAdmit hung 19807137) | partial_abort path not theater; no RewindTo train |
| 3 | **Schedule-first Avoid** | `scheduler.rs:try_execute_ready` refuses known consumers while `w` **Ready or Executing**; `admit_spine(w)` so ProducerStage progresses (no v6 yield-spin) | refuse on Ready; 19807137 refuse ≫ 0 |
| 4 | **Kill known-edge ReadyCanary** | `fence_act::act_wait_for`: Ready → WaitForDependency; DoneOptimisticRead **cert=false** (no partial-abort-bait strip) | canary only Aborting/unknown |
| 5 | **Storage true-k** | `admit_seed_begin_block` hint-fan (≥16-tx account) floor=2 + InterPrior storage PE; `vm::specfence_access_gate` plants Storage(addr,slot) PE at **live stream k** when Basic(addr) is a star | PE-on at real conflict ℓ |
| 6 | Soft=0 | No SoftWait Soft arm. WaitForDependency / PartialAbortRewind journal FF only | `soft_wait_arms=0` |

Already OK / not re-landed: OrderedAdmit rare EV; cert strip survival (M5); spine unity; dual-π delete.

## Honesty bars (must re-sweep)

Ship only if Soft=0 ∧ beat median **0.703** / fan **0.449** ∧ partial_abort win rate up materially
∧ abort_SF ≤ OCC on fan_out when PE+certs present. **Not** if only wait_for_dependency↑ / refuse↑.

This package is the plant. Sweep JSON is the verdict.
