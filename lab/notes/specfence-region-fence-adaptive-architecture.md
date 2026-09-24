# SpecFence architecture: implementable regions, fences, and adaptive control

**Date:** 2026-09-07 (Asia/Shanghai)  
**Status:** ARCHITECTURE (implementable contract)  
**Inputs:** learn/region/fence from blocks (`a84b371`), dual-horizon howto (`24094eb`), control law v3, REM Spec v1, existing `crates/pevm/src/specfence/*`  
**Non-goal:** redesign Block-STM TCB; learning stays off commit path  

---

## 0. Design thesis

Make three things first-class and separable:

1. **Region plant** — emit/consume \(a=(t,k,\ell,m)\) and versions on \(\ell\).  
2. **Fence graph** — soft/hard constraints π arms; wave park executes Wait.  
3. **Adaptive controller** — dual-horizon learner fills `PolicyCtx`; `choose_action` only place that picks Bind/Wait/SpecRead/EarlyAbort.

Today these are tangled (`RegionTable` sticky Wait ≈ fence, HotSet ≈ gate, Bayes ≈ both prior and decision). The architecture **unzips** them so each can adapt without breaking seq≡par.

```
┌─────────────────────────────────────────────────────────────┐
│  L5 Engagement   LeanOCC vs HotLocal meta (execute path tax) │
├─────────────────────────────────────────────────────────────┤
│  L4 Learner      Intra Bayes/EMA + Inter prior/flip-decay    │
├─────────────────────────────────────────────────────────────┤
│  L3 Policy π     choose_action(PolicyCtx) → ResolveAction    │
├─────────────────────────────────────────────────────────────┤
│  L2 FenceGraph   SoftWait / BindTarget / revoke / wake       │
├─────────────────────────────────────────────────────────────┤
│  L1 RegionPlant  RegionAccess events, MV versions, validate  │
├─────────────────────────────────────────────────────────────┤
│  L0 Block-STM    Scheduler, MvMemory, ESTIMATE, seq≡par TCB  │
└─────────────────────────────────────────────────────────────┘
```

Only L0–L1 touch correctness. L2–L5 may be wrong → performance only.

---

## 1. L1 RegionPlant — make \(a=(t,k,\ell,m)\) real

### 1.1 Types (stabilize)

```text
LocationId     = MemoryLocationHash          // conflict identity
RegionAccess   = { t, k, ℓ, m ∈ {R,W}, class, warm?, gas_so_far?, call_depth? }
RegionEvent    =
  | Observe(a, producer_status, producer?)   // read path
  | Publish(a, value_meta)                   // write path
  | Abort(t, inc, fail_ℓs, cascade)
  | Certify(t, ℓ)                            // validate ok for ℓ
```

**Already have:** `rem::RegionAccess`, finegrain effect log, MV versions.  
**Must ensure production SpecFence path (even without inspect):** every cold MV read/write emits `Observe`/`Publish` with monotonic `k` per `(t,inc)` — journal ordinal is enough when inspect off; gas/`d` optional.

### 1.2 Invariants

- Conflict / Wait key = **ℓ only** (drop account Wait from control path; keep address only for Basic(ℓ) hashing).  
- Beneficiary + `basic_lazy` never generate SoftWait.  
- `k` monotonic per incarnation; fences attach to a specific `Observe` (first-cross), not tx start.

### 1.3 Module map

| Concern | Module (target) | Today |
|---------|-----------------|-------|
| Emit access | `rem` + `vm` Db hooks | Partial; finegrain research-complete |
| Versions | `mv_memory` | OK |
| Per-ℓ validate | existing validate + selective invalidate | OK |
| Checkpoints for EarlyAbort/FF | `rem::Checkpoint` | Plant v2 present; EarlyAbort underused |

---

## 2. L2 FenceGraph — implementable fences

### 2.1 Fence kinds

| Kind | Meaning | Data structure | Satisfy / revoke |
|------|---------|----------------|------------------|
| **SoftWait(ℓ, waiter_t, armed_at_k)** | Do not progress waiter past this observe until last writer `<t` has Data | `FenceGraph.waits: ℓ → [{waiter, k, armed_ms}]` + `WaveParkTable` | Publish Data → wake; Bayes revoke / morph waw\|quiet → clear |
| **BindTarget(ℓ, waiter_t, TxVersion)** | Read exact version | ephemeral in `ResolveAction::Bind` | Immediate when Data ready |
| **HardValidate** | TCB | Block-STM validate | N/A (always on) |
| **EarlyAbort(t, at_k)** | Cut incarnation at early bad cross | flag + `RewindTo` / FullRetry | Research → production when hang-free |

Sticky `RegionMode::Wait` and `SpecDag.is_wait` become **mirrors of SoftWait**, not independent control. Prefer one source of truth: **FenceGraph**.

### 2.2 Arming site (critical)

```text
on Observe(a) for read of ℓ with potential prior writer:
  ctx ← Learner.features(a) ∪ MV.status(ℓ)
  match choose_action(ctx):
    Bind(v)     → read v; Learner.credit_bind
    WaitHard    → FenceGraph.arm_soft(ℓ, t, k); return Blocking → WavePark
    SpecRead    → OrderedDirtyRead; no SoftWait
    EarlyAbort  → abort incarnation at k (if armed)
```

Fence is **at first unresolved Observe**, matching block analysis (597 late SLOAD / early heavy cross).

### 2.3 Wave execution

- Keep **M2 tx-grain park** as v1 execution of SoftWait (worker steals).  
- Future v2: park at `(t,k)` continuation when RewindTo reliable — same FenceGraph, finer resume.  
- Wake: `Publish` on ℓ notifies all SoftWait waiters with writer `< waiter`.

### 2.4 Module map

| Piece | Target API | Evolve from |
|-------|------------|-------------|
| `FenceGraph` | `arm_soft`, `clear`, `wake_on_publish`, `iter_waiters` | `dag::SpecDag` + `RegionTable` Wait bits |
| Park/steal | unchanged contract | `rem::WaveParkTable` |
| Resolve | `choose_action` only | `resolve.rs` |

**Remove from hot path:** `should_wait_account` as Wait authority; HotSet **forbid Wait off-set** (today R1) → HotSet only sets `fanout_hint` / dense tracking (v3 already in resolve; finish deleting hard gate in `mod.rs`).

---

## 3. L3 Policy π — single decision choke point

### 3.1 `PolicyCtx` (complete feature vector)

```text
PolicyCtx {
  ℓ, is_program,
  writer_known, writer_done, bind_version, placeholder_ready,
  fanout_hint,                    // from HotSet / live fanout
  gross_work_depth: Option<f64>,  // inspect or None
  posterior_conflict, posterior_bind_success,
  morph_weights: {fan_out, mixed, waw_spine, quiet},  // NEW
  waw_spine_hint: bool,           // NEW from WAW/RAW or morph
  tx_heavy_hint: bool,            // NEW gas band / prior
}
```

### 3.2 `choose_action` (frozen law, tunable constants)

Order fixed; **constants adaptive** via Learner (± offline L3):

1. Bind if Data ready  
2. WaitHard if program ∧ writer known ∧ !done ∧ (fanout_hint ∨ d≥D_WAIT ∨ P≥TAU_VERY_HIGH) ∧ !waw_spine_hint  
3. Else SpecRead  
4. EarlyAbort niche: heavy ∧ d≤D_EARLY ∧ program (when plant supports)

Constants `D_WAIT`, `D_EARLY`, `TAU_*`, `C_RETRY` live in `AdaptiveParams` (process + optional block override), not scattered magic.

---

## 4. L4 Learner — how adaptive regulation works

### 4.1 State

```text
PerLocationStat {
  class_ema,
  fanout_live, fanout_ema,
  abort_alpha_beta,      // Beta
  bind_alpha_beta,
  status_hist,
  writers_recent,
}
BlockMorphPosterior { w_fan_out, w_mixed, w_waw, w_quiet }  // Dirichlet/EMA
AdaptiveParams { D_WAIT, D_EARLY, TAU_VERY_HIGH, ... }
InterBlockPrior {
  morph_ema,
  top_ℓ: Vec<(ℓ, fanout_ema, abort_rate)>,
  class_mix_ema,
  flip_sensitivity,
}
```

### 4.2 Update hooks (wire to RegionEvents)

| Event | Intra update | May adapt |
|-------|--------------|-----------|
| Observe | fanout++, status hist, optional d hist | morph weights; fanout_hint |
| Publish | wake; bind credit | posterior_bind |
| Abort | abort Beta; cascade EMA; HotSet densify ℓ | morph→fan_out; escalate tracking |
| SoftWait revoke | wait_useful− | clear fence |
| End block | pack InterBlockPrior | α normal vs α_flip |

### 4.3 Inter-block (warm-start only)

```text
block_start:
  morph ← InterBlockPrior.morph_ema
  for ℓ in prior.top_ℓ: HotSet.track(ℓ); Bayes.seed(ℓ)
  // NEVER FenceGraph.arm from prior alone
block_end:
  if KL(morph_hat, morph_ema) > κ: α = α_flip else α_normal
  morph_ema ← (1-α)morph_ema + α morph_hat
  refresh top_ℓ
```

### 4.4 What “自适应调节” means operationally

| Knob | Who moves it | Feedback |
|------|--------------|----------|
| SoftWait arm/disarm per Observe | π + FenceGraph | bind success, abort, steal wait time |
| HotSet membership | Learner (fanout/abort) | tracking density only |
| morph weights | Learner online | edge class + fanout + WAW/RAW |
| D_WAIT / D_EARLY | Offline L3 + slow EMA | wasteΔ lab / optional online regret |
| Lean vs inspect | L5 Engagement | abort_rate / meta tax |
| Inter prior strength | flip detector | 598→599 style KL |

---

## 5. L5 Engagement — meta tax adapter

Keep LeanOCC default (`Handler::run`). Escalation:

- Dense HotSet tracking + FenceGraph activity **does not** require inspect.  
- Inspect / gross-work `d` only when research flag or engagement says “depth probe on sample txs”.  
- Avoid v7 failure mode: never default-on inspect for all txs.

---

## 6. End-to-end control loop (one Observe)

```text
Vm read ℓ
  → RegionPlant.Observe(a)
  → Learner.refresh_ctx(a) → PolicyCtx
  → π.choose_action
  → FenceGraph + MvMemory + WavePark
  → (later) Publish/Abort/Validate → Learner.update
```

Adaptive regulation is **closed-loop on RegionEvents**, not a block-start mode flip.

---

## 7. Mapping: delete / keep / add

| Component | Action |
|-----------|--------|
| `choose_action` / `PolicyCtx` | **Keep**; extend morph/waw/heavy fields |
| `WaveParkTable` | **Keep** as SoftWait executor |
| `BayesMap` | **Keep** as per-ℓ posterior store; stop using as sole Wait bool |
| `HotSet` | **Keep** as tracking/fanout_hint; **remove** hard Wait gate in `should_wait_*` |
| `RegionTable` account Wait | **Deprecate** for SpecFence control |
| `RegionTable` location Wait / `SpecDag` | **Merge into FenceGraph** (or make thin facade) |
| `AdaptiveEngagement` | **Keep** for execute-path tax only |
| `FenceGraph` (new or rename SpecDag) | **Add** explicit SoftWait lifecycle |
| `InterBlockPrior` | **Add** (may live beside BayesMap/Pevm) |
| `AdaptiveParams` | **Add** single struct for π constants |
| Account `should_wait_account` | **Bypass** on SpecFence (always false for control) |

---

## 8. Implementation phases (architecture order)

### P0 — Unzip control (no new plant)

1. HotSet → hint only (delete Wait forbidden-if-absent).  
2. Disable account Wait on SpecFence path.  
3. Route all location decisions through `choose_action` + record SoftWait in dag/FenceGraph.  
4. Metrics: arms, revokes, bind, wait, spec by class/morph.

### P1 — Learner dual-horizon

1. Live fanout + morph posterior online.  
2. `waw_spine_hint` from WAW/RAW or multi-writer basic without RAW.  
3. InterBlockPrior EMA + flip decay; seed HotSet/Bayes only.  
4. Wire `PolicyCtx` morph/waw/heavy.

### P2 — FenceGraph cleanup

1. Explicit SoftWait API; RegionTable Wait = mirror.  
2. Wake path tied to Publish.  
3. Revoke policy unified (`τ_revoke` + morph).

### P3 — EarlyAbort fence

1. On heavy∧early, abort/rem rewind at `k` instead of WaitHard.  
2. Measure vs 597 minority; gate on seq≡par tests.

### P4 — Optional (t,k) park

Resume at armed `k` when continuation proven hang-free.

---

## 9. Test & measurement contract

- seq≡par unchanged (TCB).  
- Unit: `choose_action` matrix (program fanout, handler, waw, bind ready, early d).  
- Lab: L3 wasteΔ regression on 597/599/097/098/096.  
- Sweep: SF/OCC@8 on seven blocks + contiguous cores; track SoftWait arms ≠ HotSet size.  
- Flip test: run 598 then 599 — prior must not leave sticky SoftWaits into 599.

---

## 10. Bottom line

**Region** is implemented as a **RegionPlant** of accesses \(a=(t,k,\ell,m)\) on location \(\ell\).  
**Fence** is implemented as a **FenceGraph** of SoftWait/Bind/EarlyAbort armed at Observe by **one** π.  
**Adaptive** is a **Learner + AdaptiveParams + Engagement** closed loop on those events, with inter-block priors that warm-start tracking but never arm fences alone.

That is the architecture that makes the block-derived region/fence story shippable without collapsing back into sticky account Wait or HotSet gates.
