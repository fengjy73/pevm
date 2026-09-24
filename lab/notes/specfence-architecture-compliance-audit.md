# Architecture compliance audit (learn/region/fence + adaptive layers)

**Date:** 2026-09-07 (Asia/Shanghai)  
**Against:** `specfence-learn-region-fence-from-blocks.md`, `specfence-region-fence-adaptive-architecture.md`  
**Code tip:** `832bddf` / `43323b8`  
**Status:** AUDIT — gaps ranked by impact on the design thesis

---

## Verdict (one line)

**Skeleton of P0–P4 is real, but the closed loop the docs require is only partially closed:** FenceGraph + `choose_action` + Learner/InterBlockPrior exist, yet production never supplies gross-work \(d\), RegionPlant events are HotSet-gated / not first-class, π is still bypassed by post-hoc escalates, and AdaptiveParams are not L3-calibrated.

---

## Scorecard vs learn-from-blocks

| Requirement | Status | Evidence / gap |
|-------------|--------|----------------|
| Conflict key = \(\ell\) only | **Mostly** | `should_wait_account` → false on SpecFence; but `promote_account` still called on WW overlap (`vm.rs` finish path) |
| Region atom \(a=(t,k,\ell,m)\) | **Partial** | `PartialRetry.current_k` + SoftWait `armed_at_k`; `note_effect` / `note_access` only when `hotset.contains` — cold ℓ never advances plant \(k\) |
| Learn class / fan-out / morph / WAW | **Partial** | `LiveLearner.note_observe` on SpecFence resolve; no per-ℓ status_hist, no \(d\) hist, thin bind/wait_useful loop |
| Learn producer_status at discovery | **Weak** | MV writer_done/bind_version used in π; not stored as learner feature |
| Learn gross-work \(d\) | **Missing in prod** | `choose_resolve(..., None)` hardcoded — EarlyAbort + \(d\)-Wait never fire on LeanOCC |
| Dual-horizon prior warm-start only | **Yes** | `InterBlockPrior.end_block` / seed HotSet+Bayes; no SoftWait from prior |
| Fence at first-cross \(a_c\) | **Partial** | Arm on `maybe_wait` Observe-equivalent; without mid-tx \(k\) on cold path, fence site ≈ first HotLocal touch |
| WaitHard program fan-out / late \(d\) | **Partial** | fanout_hint works; late \(d\) needs inspect |
| No Wait on handler / WAW spine | **Yes** | `waw_spine_hint` + `is_program` in `choose_action` |
| EarlyAbort early-heavy | **API only** | Wired but inert without known \(d\) |
| Never prior-only SoftWait | **Yes** | |
| Never account Wait control | **Mostly** | Control false; metrics still call `should_wait_account`; promotions still touch accounts |

---

## Scorecard vs adaptive architecture L0–L5

| Layer | Doc intent | Status |
|-------|------------|--------|
| L0 Block-STM TCB | unchanged | **OK** |
| L1 RegionPlant | every cold R/W → Observe/Publish with \(k\) | **Gap** — events implicit in `maybe_wait`; HotSet-gated `note_effect` |
| L2 FenceGraph | SoftWait SoT; wake on Publish | **Mostly OK** — `arm_soft` / wake / revoke; RegionTable still dual-written as mirror |
| L3 π single choke | only `choose_action` | **Gap** — after π, `vm.maybe_wait` escalates SpecRead→Bind/WaitHard on prior_ws / high P (**second policy**) |
| L4 Learner | full PerLocationStat + AdaptiveParams from L3 | **Partial** — morph/fanout/abort yes; no L3→params; note_publish almost no-op |
| L5 Engagement | Lean default; sample depth probe | **Gap** — no “depth probe on sample txs”; \(d\) always None in prod |

### P0–P4 checklist

| Phase | Claimed | Real residual |
|-------|---------|---------------|
| P0 HotSet hint-only | Done on SpecFence `maybe_wait` | EarlyVal still HotSet-gated (OK); finish-path account promote remains |
| P1 dual-horizon | Structs live | Feature inventory incomplete; no morph-prior substitute for \(d\) when inspect off |
| P2 FenceGraph | Done | Two authorities still: Bayes `should_wait_hard` in legacy `should_wait_location` (PCC/legacy), plus post-π escalate |
| P3 EarlyAbort | Code path | **Production-inert** by design until \(d\) known — docs said plant gap; not closed |
| P4 (t,k) park | Data plane | ResumeAtK rare under Lean (no mid-tx cps) — honest but limited |

---

## Critical gaps (what still needs changing)

### G1 — Supply \(d\) without v7 inspect tax (highest)

Docs: attach \(d\) at first-cross; else **morph/class prior for \(d\)**.  
Code: always `None`.

**Fix direction (pick one, prefer A+B):**
- **A.** Cheap proxy: `gas_used_so_far / tx_gas_used` from revm interpreter counters **without** full inspect_run (or sample N txs / HotSet-only depth probe under Engagement).  
- **B.** When `d` unknown: use morph prior — fan_out → treat as late (\(d\)≈0.9) for Wait EV; heavy+fan_out early minority still needs real \(d\) or gas-so-far.  
- **C.** Do **not** use gas/limit (already correctly rejected for EarlyAbort).

Without G1, EarlyAbort and late-Wait discrimination from 597 **cannot** work in production.

### G2 — RegionPlant on all SpecFence MV touches

`note_effect` / `note_access` only if HotSet member → cold program reads never get ordinal \(k\) → SoftWait `armed_at_k=0` → P4 always FullRetry.

**Fix:** always bump per-tx \(k\) on SpecFence cold MV R/W; HotSet only densifies learner/HotLocal, not plant emission.

### G3 — Single π choke point

Post-`choose_action` escalates in `vm.rs` (SpecRead→Bind/WaitHard). That reintroduces a second policy and can Wait against WAW/handler morph.

**Fix:** fold residual/prior_ws / very-high P into `PolicyCtx` + `choose_action` only; delete post-match mutate (except force_prefix PartialRetry, which is repair not π).

### G4 — Close learner outcome loop

Missing / thin: `wait_useful`, bind success credit on Bind hit, status_hist, \(d\) hist, Publish→bind credit beyond counter.

**Fix:** on Bind success / SoftWait wake useful / abort@ℓ update Beta already partially in Bayes — wire Learner symmetrically and feed `posterior_*` from one place.

### G5 — Stop account-side promotions on SpecFence

`promote_account` / multi-writer account promote still run; contradicts “account diagnostic only.”

**Fix:** gate `promote_account` / account RegionMode behind `mode==Pcc` only.

### G6 — AdaptiveParams ← L3 (and flip test)

Constants still hand defaults. Docs require offline wasteΔ recalibration + 598→599 flip test that SoftWaits don’t stick.

**Fix:** script writes `AdaptiveParams` JSON/overrides from `l3-offline-ev.json`; CI or lab flip smoke.

### G7 — Measurement contract unfinished

No post-P0–P4 SF/OCC@8 sweep; SoftWait arms ≠ HotSet size not validated on mainnet cores.

---

## What is actually good (do not regress)

- FenceGraph SoftWait API + wake + morph/Bayes revoke  
- Account Wait control disabled on SpecFence  
- HotSet not a hard Wait gate on SpecFence resolve  
- `waw_spine_hint` / `is_program` / InterBlockPrior flip α  
- EarlyAbort depth rule refuses gas/limit proxy  
- P4 hang-free FullRetry fallback  

---

## Recommended next work order

1. **G1+G2** — plant \(k\) always + depth (probe or morph prior) so π matches block analysis  
2. **G3+G5** — purify choose_action choke; kill SpecFence account promote  
3. **G4** — outcome feedback  
4. **G6+G7** — L3→params + flip smoke + SF/OCC sweep vs v8  

Until G1–G3 land, claiming “architecture implemented” overstates: the **control plane scaffolding** is there; the **block-derived adaptive behavior** is not yet fully executable on LeanOCC mainnet path.
