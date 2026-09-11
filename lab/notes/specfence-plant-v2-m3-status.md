# Plant v2 M3 status — prior Bind from learned WŜ/RŜ

**Date:** 2026-09-05 (Asia/Shanghai)  
**Branch:** `specfence` @ fengjy73/pevm  
**Parent tip:** M1h `aab8a98` / `60d0cae`  
**Scope:** SpecFence path only; OCC/PCC unchanged; M2 park/steal intact; M1h scaffolds untouched

## Goal (iron-law B)

Cut **first-incarnation** waste by Binding predicted reads **before** first interpret touch when prior RW knowledge is good enough — without claiming TPS ≫ OCC until jump covers ERC-20 (M1i) or meta is off (M4).

## What landed

| Piece | Status |
|-------|--------|
| `RwPriorMap` process-local write / co-access frequencies | **Landed** (`specfence/prior.rs`) |
| Observe WŜ on SpecFence `record` + validate success / abort | **Landed** |
| Inter-block decay with write floor (≥1 once learned) | **Landed** |
| Bind-before-touch when published Data + process/residual prior | **Landed** (`vm::maybe_wait` escalate + `choose_resolve` placeholder) |
| Metrics `prior_bind_hits` / `prior_bind_miss` / `first_pass_validate_fail` | **Landed** |
| Abort residual Bohm-lite WŜ (unchanged) | **Kept** |
| Successful WS → residual publish | **Not default** — caused WaitHard storms / M2 hangs on ERC-20 |
| Process-prior WaitHard escalate | **Not default** — same hang class |
| Per-code-hash / entry priors | **Not landed** (optional Spec v1; Basic/location freq sufficient for M3 gate) |

## When Bind fires (M3)

1. **Published Data** for ℓ from a lower writer (`last_data_before`) **and** writer done.  
2. **Plus** prior signal: process `RwPriorMap.predicts_write(ℓ)` and/or abort **residual** WŜ and/or PartialRetry force-prefix.  
3. π then prefers **Bind** over SpecRead (placeholder_ready / escalate). Learning ∉ TCB — wrong prior → more SpecRead / validate fail / repair; sequential ≡ parallel still holds.

## Interaction with Bayes cost π

- Cost-aware WaitHard vs SpecRead **unchanged** for unfinished writers (no process-prior WaitHard bias).  
- `write_confidence` mildly boosts `posterior_bind_success` in `PolicyCtx`.  
- `observe_bind_hit` / `observe_bind_miss` still update Bayes bind-useful.  
- Sticky Wait revoke + τ_very_high safety valve retained.

## Test

- `specfence_m3_prior_bind_cuts_first_pass_waste` — same-sender WW learns WŜ on block1; block2 shows `prior_bind_hits > 0`, seq≡par, bounded validate fails.  
- Full `specfence` suite green (retry flaky inspect hangs; M1h still `#[ignore]`).

## Honest TPS expectation

| Claim | Honest status |
|-------|----------------|
| prior_bind_hits > 0 on contended same-sender | **Shown** |
| Fewer first-pass SpecRead mistakes when Data already published | **Shown** (metric) |
| SF/OCC TPS ≫ 1 on 7-block mainnet | **Not claimed** — still need Storage/nested jump (M1i) + M4 adaptive meta |
| M3 alone beats OCC on ERC-20 abort gas | **No** — Bind helps when version known; concurrent first wave still SpecReads |

## Remaining gaps

1. **M1i / write-prefix + valued CALL** — cover ERC-20 abort gas with safe absolute jump.  
2. **M4 adaptive engagement** — SpecFence meta off on conflict-free blocks.  
3. **7-block sweep** with `prior_bind_hits`, `evm_entries`, `absolute_jump_applied`, SF/OCC.  
4. Optional: safe block-local successful-WS residual publish under a gate; code-hash priors.

## Files

- `crates/pevm/src/specfence/prior.rs` — `RwPriorMap`  
- `crates/pevm/src/specfence/{mod,resolve,metrics,bayes}.rs` — wire + metrics + π placeholder  
- `crates/pevm/src/{vm,pevm,mv_memory}.rs` — observe / Bind escalate / optional `publish_ws_prior`  
- `crates/pevm/tests/specfence.rs` — M3 integration test  
- `lab/notes/specfence-plant-v2-m3-status.md` — this note  
