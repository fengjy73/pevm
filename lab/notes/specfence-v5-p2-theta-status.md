# SpecFence V5-P2 — θ into EV + meta tax — status

**Date:** 2026-09-07 (Asia/Shanghai)  
**Branch:** `specfence`  
**Parent tip:** `efb8aa1` (V5-P1 single Lean repair)  
**Authority:** `lab/notes/specfence-v5-first-principles-clean-slate.md` §7 V5-P2

## Goal

Purify AEC so EV uses **measured** continuous θ, not Boolean leftovers:

1. Wake latency / cascade / idle-steal → `EV_Wait` (LiveLearner EMAs).
2. `E_reexec` (V5-P1 Lean ForceBind≈1.2 vs FullRestart≈2.2) → `EV_Spec`.
3. Meta budget: measured meta tax → SpecRead on tiny EV gaps (no SoftWait storm).
4. Features only (HotSet / morph / d / fanout) — **never** Boolean Wait gates / fanout→WaitHard.

## θ wired

| θ | Source | EV role |
|---|--------|---------|
| `E_wait_time(ℓ)` | SoftWait arm→wake ns EMA (`note_wait_latency`) | Multiplies EV_Wait |
| `E_cascade(ℓ)` | abort cascade EMA (`note_abort`) | Inside EV_Spec via β |
| `E_idle_steal` | park/steal proxy EMA (`note_steal_or_park_proxy`) | Excess-over-prior term in EV_Wait, scaled `1/(1+fanout)` |
| `E_reexec` | Lean ForceBind / RewindTo / FullRestart samples (`note_reexec_cost`) | Inside EV_Spec via γ; EV_Early additive |
| `meta_tax` | `meta_ops/useful` after warmup | Tiny-gap Spec bias; ρ hard Spec |

Steal must **not** collapse idle tax (would cheapen Wait on fan-out and serialize dependents).

## EV formula diffs (vs AEC / abort-cheapening)

```text
# before (P1 / abort-cheapening)
EV_Wait  = E_wait_time * (1 + α * fanout)
EV_Spec  = P_abort * (W_remain + β * E_cascade + 0.5 * E_reexec)

# after V5-P2
EV_Wait  = E_wait_time * (1 + α * fanout)
         + δ * max(0, E_idle − e_idle_prior) / (1 + fanout)
EV_Spec  = P_abort * (W_remain + β * E_cascade + γ * E_reexec)
EV_Early = W_prefix(d) + E_reexec          # unchanged niche
π = argmin; ties → SpecRead
+ meta_budget_exceeded → SpecRead
+ measured meta_tax > 0 ∧ (EV_Spec − EV_Wait) < meta_gap_eps*(1+meta_tax) → SpecRead
```

`γ` is `AdaptiveParams.gamma_reexec` (default **0.50**, same half-weight as before).  
Cold start: idle excess = 0 ⇒ EV_Wait matches P1 until park raises idle.

### AdaptiveParams (rates/priors, not Wait cuts)

| Param | Default | Role |
|-------|--------:|------|
| `alpha_fanout` | 0.22 | fanout ↑ EV_Wait |
| `gamma_reexec` | 0.50 | E_reexec weight in EV_Spec |
| `delta_idle` | 0.10 | idle-excess weight |
| `e_idle_prior` | 0.20 | idle EMA prior / excess baseline |
| `meta_budget_rho` | 0.30 | hard Spec when meta/useful > ρ |
| `meta_gap_eps` | 0.05 | tiny Wait-win → Spec under meta_tax |

## Files

- `crates/pevm/src/specfence/resolve.rs` — EV_Wait idle excess; γ·E_reexec; meta tiny-gap; unit tests
- `crates/pevm/src/specfence/learner.rs` — `E_idle_steal` EMA; `meta_tax_ratio`; params; unit tests
- `crates/pevm/src/specfence/mod.rs` — wire `e_idle_steal` / `meta_tax` into `PolicyCtx`
- `lab/notes/specfence-v5-p2-theta-status.md` — this note
- `lab/results/g7-v5-p2-smoke.run.log`, `g7-sf-occ-smoke.json`

## Tests

```text
cargo test -p pevm --lib
  → 85 passed (incl. v5_p2_* wake/idle/reexec/meta-gap)

cargo test -p pevm --test specfence
  → 23 passed, 0 failed, 13 ignored (M1* research-only)
```

## Smoke (G7 harness @8)

```text
cargo run -p pevm --release --config 'profile.release.lto=false' --example specfence_g7_smoke
```

Canonical run (this tip):

| Block | SoftWait | WaitHard | SF/OCC | vs P1 note |
|-------|----------|----------|--------|------------|
| **14689597** | **30** | 30 | **0.166** | P1 SoftWait 27 / SF/OCC 0.154 |
| 19606599 | 18 | 18 | 0.312 | P1 SoftWait 17 / 0.336 |
| 19469097 | 63 | 64 | 0.362 | P1 SoftWait 66 / 0.368 |
| 19606598 | 5 | 5 | 0.311 | P1 SoftWait 2 / 0.296 |
| **mean** | | | **0.288** | P1 mean 0.288 |

**SoftWait on 597 stays scarce** (30 ≪ G7 428; in ~20–40 band). SF/OCC ≥ P1 ~0.154 on the recorded run.  
Same-machine P1 tip recheck earlier today: SoftWait 41 / SF/OCC 0.143 (noise). Repeat smoke can move SoftWait ~10–40 and SF/OCC ~0.10–0.19 under load — do **not** chase SoftWait up.

## Forbidden (unchanged)

Restoring `fanout_hint → WaitHard`, Boolean Wait ladders, Heat/account sticky Wait, or default-on inspect to chase SF/OCC.

## Remaining (V5-P3+)

- Bind quality / mid-tx RewindTo+FF without inspect tax (hang-free only)
- Research plant graduation only if beats OCC on 597 hang-free
- Later: delete unused Bayes bool helpers / HeatMap demote / module rename
