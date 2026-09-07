# SpecFence P0 + P1 + P2 status

**Date:** 2026-09-07 (Asia/Shanghai)  
**Branch:** `specfence`  
**Authority:** `specfence-region-fence-adaptive-architecture.md` (5e306f7), learn-from-blocks, control-law v3  
**Tip:** `5eb5854` (implementation); docs on branch tip after push

---

## What changed

### P0 — Unzip control
- Removed hard HotSet Wait gate in `SpecFenceCtx::should_wait_location` (HotSet = fanout/tracking hint only).
- SpecFence `should_wait_account` always returns `false` (account Wait diagnostic-only; conflict key = `MemoryLocation`).
- SpecFence no longer seeds account Wait via `seed_wait_regions` (PCC only).
- Beneficiary + `basic_lazy` never SoftWait on SpecFence path.
- Every SpecFence location resolve still goes through `choose_action` / `choose_resolve`; WaitHard arms SoftWait.
- Metrics: `soft_wait_arms`, `cost_chose_{wait,spec}_{program,handler}`, plus existing bind/wait/spec/revoke counters.

### P1 — Dual-horizon Learner
- New `crates/pevm/src/specfence/learner.rs`:
  - `MorphWeights`, `AdaptiveParams`, `LiveLearner`, `InterBlockPrior`, `TopLocPrior`
  - Online morph + live fanout updated on Observe / Abort / Publish hooks
  - `waw_spine_hint`, `tx_heavy_hint` (gas-limit band heuristic)
- `PolicyCtx` extended with `morph_weights`, `waw_spine_hint`, `tx_heavy_hint`, `params`
- `choose_action`: WaitHard suppressed when `waw_spine_hint`; EarlyAbort niche flagged via `early_abort_candidate` (P3 TODO, not armed)
- Block start: InterBlockPrior seeds HotSet (`track_from_prior`) + mild Bayes — **never** SoftWait from prior alone
- Block end: morph EMA with flip → higher α (`α_flip`)

### P2 — FenceGraph
- `dag.rs` evolved to explicit `FenceGraph` (`SpecDag` type alias):
  - `arm_soft(ℓ, waiter_t, k, writer?)`, `clear` / `clear_waiter`, `wake_on_publish`, `clear_for_writer`, `iter_waiters`
- SoftWait is source of truth; RegionTable location Wait / wait-flags are mirrors
- Publish/finish → `FenceGraph.clear_for_writer` + existing WavePark `wake_writer_done` unpark
- Unified revoke: `try_revoke_unified` (`τ_revoke` ∨ morph quiet/waw)

---

## Key APIs

| API | Module | Role |
|-----|--------|------|
| `choose_action(PolicyCtx)` | `resolve` | Sole π choke point |
| `AdaptiveParams` | `learner` / `resolve` | `D_WAIT`, `D_EARLY`, `TAU_*`, WAW/heavy knobs |
| `LiveLearner::note_observe/abort/publish` | `learner` | Intra-block features |
| `InterBlockPrior::{morph_ema,top_locations,end_block}` | `learner` | Inter-block warm-start |
| `FenceGraph::arm_soft / wake_on_publish / clear` | `dag` | SoftWait lifecycle |
| `HotSet::track_from_prior / writer_count` | `hotset` | Tracking hint only |

---

## Tests

```
cargo test -p pevm --lib
# 52 passed

cargo test -p pevm --test specfence --test raw_transfers --test small_blocks --test mixed
# specfence: 22 passed, 13 ignored (M1* research)
# raw_transfers: 8 passed
# small_blocks: 2 passed
# mixed: 1 passed
```

New/extended unit coverage in `resolve::tests`:
- program+fanout → WaitHard
- handler → SpecRead
- waw_spine → SpecRead
- Bind when ready
- HotSet absent still SpecRead/Wait via π (not gate)
- EarlyAbort candidate flagged but not armed

---

## Out of scope (ready for later)
- **P3** EarlyAbort production arm (hook: `early_abort_candidate` / TODO in `choose_action`)
- **P4** `(t,k)` park (SoftWait already records `armed_at_k`)

---

## Hard constraints respected
- seq≡par TCB unchanged (integration tests green)
- OCC/PCC largely unchanged (PCC still seeds account Wait)
- Production default: Handler::run / LeanOCC, no inspect tax
- Learning ∉ TCB
- Conflict key = MemoryLocation; no account Wait as SpecFence control
- No secrets in git
