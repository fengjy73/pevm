# SpecFence Adaptive EV Controller (AEC) — status

**Date:** 2026-09-07 (Asia/Shanghai)  
**Branch:** `specfence`  
**Authority:** `lab/notes/specfence-true-adaptive-vs-threshold-ladders.md`  
**Tip:** `1fefc36`

---

## 1. Design mapping

| Authority requirement | Implementation |
|----------------------|----------------|
| adaptive = online makespan EV over FenceGraph actions | `choose_action` = argmin `{Bind, Wait, Spec, Early}` via `compute_ev` |
| Boolean Wait ladders are bugs | Removed `want_wait` / `fanout_hint→WaitHard` / `d≥D_WAIT→WaitHard` / morph `d:=0.9` Wait force / `P≥τ` Wait force |
| HIGH fanout **raises** EV_Wait | `EV_Wait = E_wait_time * (1 + α * fanout)` |
| Default on EV tie → SpecRead | Strict `<` only; ties keep SpecRead |
| EarlyAbort only when `d` is Some | `early_abort_candidate` + finite `EV_Early` only if known d ∧ heavy |
| Learning ∉ TCB; SoftWait never from inter-block prior alone | Unchanged `InterBlockPrior` contract; θ warm-start only |
| Conflict key = MemoryLocation | Unchanged (P0) |
| Do not retune D_WAIT/H_w as solution | `d_wait` retained for lab JSON only — **not** a π Wait gate; HotSet = feature/admission |
| Meta budget ρ | `LiveLearner::meta_budget_exceeded` → force SpecRead after warmup |
| Outcome instrumentation | SoftWait arm→wake ns, abort+cascade EMA, bind/wait_useful, steal/park proxies |

### Formulas (phase A)

```text
EV_Bind  = 0 if Data ready else +∞
EV_Wait  = E_wait_time(ℓ) * (1 + α * fanout_feature)   # α = AdaptiveParams.alpha_fanout
EV_Spec  = P_abort(ℓ,x) * (W_remain(d) + β * E_cascade)
EV_Early = W_prefix(d) + E_reexec   # only if d known & heavy features
π = argmin EV; ties → SpecRead
```

`AdaptiveParams` are **learning rates / priors** (`alpha_fanout`, `beta_cascade`, `lr_*`, `e_*_prior`, `meta_budget_rho`), not decision cuts.

Features only (never Boolean Wait gates): HotSet membership, live fanout, morph weights, measured/proxy `d`, heavy hint, WAW spine.

---

## 2. Code touchpoints

- `crates/pevm/src/specfence/resolve.rs` — AEC `choose_action` / `compute_ev`
- `crates/pevm/src/specfence/learner.rs` — EV estimators, latency/cascade EMA, meta ρ
- `crates/pevm/src/specfence/dag.rs` — arm→wake latency drain
- `crates/pevm/src/specfence/mod.rs` + `vm.rs` + `pevm.rs` — wiring
- `lab/scripts/l3_makespan_ev_lab.py` — phase B makespan EV (not wasteΔ gate)
- `lab/results/l3-makespan-ev.{json,md}`

---

## 3. Block 14689597 @8 — SoftWait / WaitHard / SF/OCC

| Metric | G7 (threshold ladders) | AEC (this tip) | Δ |
|--------|------------------------:|---------------:|--|
| SoftWait arms | **428** | **28** | **−93%** |
| WaitHard | **2828** | **503** | **−82%** |
| SF TPS | ~22k | ~16k | − |
| OCC TPS | ~129k | ~155k (run variance) | − |
| **SF/OCC** | **≈0.17** | **≈0.104** | lower (abort-heavy Spec path) |
| SF aborts | 81 | 325 | ↑ (Spec wave preserves width, pays validate fails) |
| OCC aborts | 47 | 59 | − |

**Success criterion met:** SoftWait arms and WaitHard dropped a lot vs G7 (428/2828).  
**SF/OCC** on 597 is not yet recovered to G7/v8 — expected while Wait-by-threshold is removed and abort recovery (M1 inspect/jump) stays research-only. Further SF/OCC gains must come from makespan-aware abort recovery / Bind quality, **not** restoring Boolean Wait ladders.

### Other smoke blocks (AEC)

| Block | Morph | SoftWait | WaitHard | SF/OCC |
|------:|-------|--------:|---------:|-------:|
| 19606599 | mixed | 13 | 182 | 0.276 |
| 19469097 | long_chain | 28 | 140 | 0.259 |
| 19606598 | quiet | 2 | 2 | 0.359 |
| **mean** | | | | **≈0.250** (v8 ref ~0.325) |

G7 mean SF/OCC was ≈0.269 on the same set.

---

## 4. L3 makespan lab (phase B)

`lab/results/l3-makespan-ev.md`: on fan_out 597, Wait-heavy / Spec ≈ **3.6×** makespan; AEC ≈ Spec; EV_Wait ≫ EV_Spec at high fanout.  
**wasteΔ must not gate Wait-heavy online policy.**

---

## 5. Validation

- Unit tests: high fanout → Spec over Wait; Bind when Data; EarlyAbort only with known early d; tie→SpecRead; no fanout_hint Boolean Wait — **green**
- `cargo test -p pevm --lib` — **green** (69)
- Integration: `specfence`, `small_blocks`, … — **green**
- Hang-free smoke on 597/599/097/598 @8

---

## 6. Blockers / next

1. **597 SF/OCC** still ≪ v8 while SoftWait is correctly scarce — needs better abort/cascade recovery under Spec-default (M1 path remains research-gated; LeanOCC default).
2. E_wait_time units are coarse (ns→work via 1e6); steal/idle proxies are best-effort.
3. Do **not** reintroduce `D_WAIT` / `fanout_hint→WaitHard` to chase SF/OCC.
