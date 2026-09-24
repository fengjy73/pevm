# True dynamic adaptive vs threshold ladders — meta-gap as architecture

**Date:** 2026-09-07 (Asia/Shanghai)  
**Status:** DESIGN REFRAME (authoritative critique of current “adaptive”)  
**Evidence:** G7 smoke `lab/results/g7-sf-occ-smoke.json` (tip ~`e2460ed`); learn-from-blocks; region/fence architecture  
**Claim:** The SF≪OCC meta gap after G1–G7 is **mostly architectural**, not “need better thresholds.” Much of what we call adaptive is still a **boolean policy ladder** with hand constants.

---

## 0. Smoking gun (597 @8)

| Mode | TPS | SoftWait arms | WaitHard | Aborts |
|------|----:|--------------:|---------:|-------:|
| OCC | ~128k | 0 | 0 | 47 |
| SpecFence | ~22k (**SF/OCC≈0.17**) | **428** | **2828** | **81** |

Fan-out morphology: hundreds of SoftWaits. OCC keeps the wave wide and pays **few** aborts; SpecFence **serializes** the hot storage clique via WaitHard and still aborts more. That is not “learning too slow” — it is the wrong control objective encoded as rules:

```text
if program ∧ (fanout_hint ∨ d≥D_WAIT ∨ P≥τ): WaitHard
```

On 597, `fanout_hint` is true for the storm → almost always Wait → \(T_{\mathrm{crit}}\) explodes → meta tax dominates any abort savings L3 predicted in **offline wasteΔ** (which ignored steal/idle/makespan under 8 cores).

**Architecture bug:** L3 offline EV optimized **redo+wait gas units**, not **parallel makespan**. The online policy copied that as thresholds → Wait-heavy → loses to OCC.

---

## 1. Inventory: what is still a threshold ladder

| Mechanism | Looks like learning | Actually |
|-----------|---------------------|----------|
| `fanout_hint` / `live_fanout≥8` / HotSet `H_w=8`,`H_a=3` | density tracking | **hard Boolean** into Wait |
| `d≥D_WAIT=0.5`, `d≤D_EARLY=0.15` | depth-aware | **fixed cuts** on a proxy |
| morph `wait_depth_prior≈0.9` if fan_out≥0.35 | morphology | **fixed late-d injection** → more Wait |
| `P≥τ_very_high=0.75` | Bayes | **scalar threshold** on Beta mean |
| `dominant_waw≥0.35`, `quiet≥0.45`, `FLIP_KL=0.35` | morph posterior | **argmax + cut** |
| `heavy_gas_limit=200k` | tx band | **gas_limit threshold** (not even gas_used) |
| `waw_writer_floor=8` | WAW detect | **count threshold** |
| `cost_wait∈{0,1}`, `C_RETRY=3`, `COST_MARGIN=0.4` | cost model | **frozen units**, not fit to measured park/abort times |
| Engagement abort_rate≥0.08 | escalate | **ladder** |
| Inter-block EMA α∈{0.25,0.65} | dual-horizon | **two-level switch** |

Bayes Beta updates are real **local** learning of \(P(\mathrm{conflict}\mid\ell)\), but π **discards continuous EV** and collapses to Boolean Wait/Spec/EarlyAbort via the table above. That is adaptive **features**, threshold **policy**.

---

## 2. What “truly dynamic adaptive” must mean here

Under fixed commit order, the only performance-relevant objective is approximately:

\[
\min \; \mathbb{E}\big[T_{\mathrm{makespan}}\big]
= \mathbb{E}\big[T_{\mathrm{crit}}(\widehat G) + T_{\mathrm{idle}} + T_{\mathrm{redo}} + T_{\mathrm{meta}}\big]
\]

A decision at Observe \(a_c\) is adaptive iff it **estimates this EV online** and picks

\[
\pi^\star \in \{\mathrm{Bind},\mathrm{Wait},\mathrm{SpecRead},\mathrm{EarlyAbort}\}
= \arg\min_a \widehat{\mathrm{EV}}_a(\mathrm{features})
\]

with \(\widehat{\mathrm{EV}}\) **updated from outcomes** (park duration, steal success, abort+cascade work, bind hit), not from whether a feature crossed 0.5.

### 2.1 Required learned quantities (continuous)

| Symbol | Meaning | Update from |
|--------|---------|-------------|
| \(\hat P_{\mathrm{abort}}(x)\) | abort if SpecRead | validate fail / EarlyVal |
| \(\hat T_{\mathrm{wait}}(x)\) | wall time until Data if Wait | SoftWait arm→wake latency |
| \(\hat W_{\mathrm{remain}}(x)\) | remaining interpreter work | \(1-d\), effect progress |
| \(\hat C_{\mathrm{cascade}}(x)\) | expected contagion if abort | cascade size hist |
| \(\hat U_{\mathrm{parallel}}(x)\) | cores that stay busy if Spec vs Wait | steal/idle counters |

Then (sketch):

\[
\begin{aligned}
\mathrm{EV}_{\mathrm{Wait}} &\approx \hat T_{\mathrm{wait}} + \text{idle penalty on dependents} \\
\mathrm{EV}_{\mathrm{Spec}} &\approx \hat P_{\mathrm{abort}}\cdot(\hat W_{\mathrm{remain}}+\hat C_{\mathrm{cascade}}) \\
\mathrm{EV}_{\mathrm{Early}} &\approx \hat W_{\mathrm{remain}}^{\mathrm{prefix}}+\hat T_{\mathrm{reexec}} \\
\mathrm{EV}_{\mathrm{Bind}} &\approx 0 \quad\text{if Data ready}
\end{aligned}
\]

**No \(D\_WAIT\) cut.** Depth \(d\) enters only as a feature inside \(\hat W_{\mathrm{remain}}\) and \(\hat P_{\mathrm{abort}}\). Fan-out enters as a feature inside \(\hat T_{\mathrm{wait}}\) and idle penalty — on 597, high fan-out should **raise** EV_Wait (serialize many readers), not force Wait.

### 2.2 Structural vs learned (keep structure, kill ladders)

**Keep as structure (not thresholds):**
- Conflict object \(\ell\); access \(a=(t,k,\ell,m)\)
- FenceGraph SoftWait / Bind / EarlyAbort **actions**
- seq≡par validate TCB
- beneficiary/lazy exclusion
- Dual-horizon: prior = **initialization of \(\hat\theta\)**, not Boolean Wait seed

**Demote to features only (never Boolean gates):**
- HotSet membership, writer count, morph weights, live fanout, \(d\), heavy hint, WAW ratio

**Remove as policy:**
- `if fanout_hint: WaitHard`
- `if d≥0.5: WaitHard`
- `if morph fan_out: d:=0.9` as Wait force
- HotSet `H_w`/`H_a` as anything but cache admission for dense stats

---

## 3. Why current architecture creates the meta gap

1. **Wrong objective in the plant–policy link:** L3 wasteΔ ≠ 8-core makespan. Wait looks good offline, catastrophic online on fan-out.  
2. **Wait serializes the wide wave:** OCC’s virtue on 597 is parallelism despite conflict; SpecFence converts conflict into **queueing**.  
3. **Hint→action short circuit:** fanout_hint bypasses continuous cost comparison (`cost_wait` is 0 or 1 — useless for EV).  
4. **Morph prior amplifies Wait:** fan_out→d̂=0.9→more WaitHard exactly when Wait hurts most.  
5. **Meta tax unmodeled:** SoftWait/Bayes/HotSet bookkeeping + park storms not in L3.  
6. **Engagement still ladder:** Lean vs densify by abort_rate cut.

---

## 4. Target architecture: Adaptive EV Controller (AEC)

```text
RegionPlant Observe(a)
    → FeatureVec x (ℓ class, fanout, d, morph, status, heavy, …)   # no Boolean policy
    → EVEstimator.θ  predicts EV[Bind/Wait/Spec/Early | x]
    → π = argmin EV     # sole decision
    → FenceGraph execute
    → Outcome (wake latency, abort, cascade, steal) → θ ← update
InterBlockPrior: carry θ / sufficient stats only (warm-start), never SoftWait
```

### 4.1 Minimal implementable AEC (phase A)

Replace Boolean `want_wait` with:

```text
ev_bind  = 0 if Data else +∞
ev_wait  = E_wait_time(ℓ) * (1 + α * dependent_fanout)   # α learned
ev_spec  = P_abort(ℓ,x) * (W_remain(d) + β * E_cascade)
ev_early = W_prefix(d) + E_reexec   # only if d known & heavy features
pick argmin; ties → SpecRead (OCC-like default)
```

Initialize \(E\_wait\_time, P\_abort, E\_cascade\) from Bayes/histories; **update every outcome**.  
`AdaptiveParams` become **learning rates / priors**, not decision cuts.

### 4.2 Makespan-aware L3 (phase B)

Rebuild offline lab to simulate **P-core schedule** under Wait vs Spec policies on L1/L2 traces (not gas wasteΔ alone). Gate online π changes on makespan EV, not redo scalars.

### 4.3 Meta budget (phase C)

Explicit constraint: `meta_ops / useful_effects < ρ` learned or capped; if exceeded → force SpecRead (OCC fallback) for cold ℓ. This is still a control law but on **measured tax**, not HotSet membership.

---

## 5. What to stop doing

- Retuning `D_WAIT`, `H_w`, morph 0.9, `τ` to chase SF/OCC.  
- Treating G1–G7 “compliance” as done adaptive CC.  
- Using L3 wasteΔ as authorization for Wait-heavy online policy.  
- Adding more Boolean niches (another morphology cut).

---

## 6. What to do next (ordered)

1. **Freeze this reframe** — adaptive = online makespan EV, thresholds = bugs.  
2. **Instrument outcomes:** SoftWait arm→wake ns, abort cascade work, steal idle — feed Learner.  
3. **Replace `want_wait` Boolean with argmin EV** (phase A); default SpecRead on EV ties.  
4. **Re-run 597 first** — success = SoftWait arms ≪ 428 and SF/OCC ↑ without new constants.  
5. **Rebuild L3 makespan lab** (phase B); retire wasteΔ-as-gate.  
6. **Meta budget** (phase C) if bookkeeping still hurts quiet/mixed.

---

## 7. Bottom line

Current SpecFence after G1–G7 is a **well-instrumented threshold automaton** with Bayesian *inputs*. True dynamic adaptive CC is an **online EV controller over FenceGraph actions** whose parameters are fit to **makespan-relevant outcomes**, with morphology/fan-out/\(d\) as features — not switches. The meta gap on 597 is the proof: Wait-by-threshold destroyed the wave OCC preserves.
