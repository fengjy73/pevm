# SpecFence complete architecture v4.1 — Frozen Decision Grain (AUTHORITATIVE SoT)

**Date:** 2026-09-13 (Asia/Shanghai, UTC+8)  
**Status:** **AUTHORITATIVE** design SoT — **analysis + design only**; **π grain frozen** from empirical field selection. **Pause before coding.**  
**Branch / tip at freeze:** `cursor/specfence-complete-cc-63b0` @ `a13c4bd` (+ this note)  
**Empirical basis (field table):** `lab/notes/specfence-decision-field-selection-from-99.md` @ `a13c4bd` — 99-block SF/OCC@8 DecisionFieldAgg (252 758 decisions / 80 blocks with ≥1 decision).  
**Companion JSON:** `lab/results/decision-field-99-selection.json`, `decision-field-99-aggregate.json`, `decision-field-99-sf-occ-sweep.json`  
**Supersedes (grain SoT):**  
- `lab/notes/specfence-complete-architecture-v4-finegrain.md` — **pre-freeze v4.1** (kept `inc` in \(a\); recommendation-era). Historical grain sketch only.  
- `lab/notes/specfence-complete-architecture-v4-occ-pcc-hybrid.md` — **v4.0** tx-coarse hybrid reading. Hybrid *identity* absorbed; grain SoT replaced.  
**Errata (why tx-grain hybrid is not enough):** `lab/notes/specfence-v4-txgrain-errata.md`  
**Vocab (frozen):** **Spec = Region** (not “speculate”). **Fence** = Bind / WaitFor / serial-lane / ordered-admission on **Region-accesses**. **Unfenced** = optimistic — literally **OCC-cost for this access** when ¬PredictedEssential ∧ ¬independence shortcut. Product name SpecFence stays.

This document **freezes** the decision grain π. It does **not** reopen v3 CostGate-as-face or v2 Fence-first. Hybrid identity (OCC default + learned timely PCC) is **kept**; live Avoid/Resolve/learning actuators use **only** fields that enter π.

---

## Frozen π (authoritative)

```
a = (t, k, depth, ℓ, mode)                    # access-event identity (inc NOT in Avoid key)
e_vis = (writer?, published_Data?, edge_kind) # EdgeVisibility operand
gate  = PredictedEssential(ℓ, k, morph) ∨ independence_certified
verb  = Bind | WaitFor|serial-lane|ordered_admit | Unfenced≡OCC
        # THIS a only — never sticky Wait-on-tx
```

**Decision grain one-liner:** *Primary π unit = \(a=(t,k,\mathrm{depth},\ell,\mathrm{mode})\) + \(e_{\mathrm{vis}}\) + gate — never “tx \(t\) waits,” never `inc` as Avoid key.*

**Hybrid one-liner:** *OCC by default **per access**; learned fine PCC Always-Detect + timely Avoid/Resolve on predicted-essential **Region-accesses / edges**; reincarnate cheap when prediction misses.*

**Correction vs pre-freeze v4.1 sketch:** pre-freeze wrote \(a=(t,\mathrm{inc},k,\mathrm{depth},\ell,\mathrm{mode})\). Freeze **drops `inc` from Avoid π**. Incarnation remains Resolve / reincarnation **bookkeeping** only (observe / Resolve context), never Avoid/sticky key.

---

## Essence test (ONE paragraph)

**Frozen-Grain Learned OCC–PCC Hybrid SpecFence** keeps OCC as the **default cost class per access**, and runs **always-on Detect + learning-driven timely PCC Avoid/Resolve** — but every **live** control decision is keyed by frozen π: access-event \(a=(t,k,\mathrm{depth},\ell,\mathrm{mode})\), EdgeVisibility \(e_{\mathrm{vis}}=(\mathrm{writer?},\mathrm{published\_Data?},\mathrm{edge\_kind})\), and gate \(\mathrm{PredictedEssential}(\ell,k,\mathrm{morph})\lor\mathrm{independence\_certified}\). On each SLOAD / SSTORE / CALL / BALANCE boundary the plant chooses Bind / WaitFor / serial-lane / ordered_admit / Unfenced≡OCC **for this \(a\) only**; other accesses in the same tx may differ. Timely Resolve rebinds **this** read origin when value-stable, skips a **certified access/frame prefix**, and full-reincarnates only when grain identity is lost — never whole-tx SuffixRepair / SoftWait / ForcePrefix as default. Learning **inputs** may use observe-only fields; **live verbs** never OR-in exclude-set sticky/meta. Prediction miss ⇒ OCC reincarnation of the failing grain’s residual — still the cheap failure path.

---

## 0. Hard bans (non-negotiable) — includes exclude set

| Ban | Why / 99-block citation |
|-----|-------------------------|
| SoftWait Soft storms | Wake≪reabort; soft=0 everywhere |
| **Tx-level SoftWait / sticky Wait-on-tx** | Wait is an **access/edge** actuator; exclude sticky Wait-on-tx |
| **Whole-tx `ForcePrefix` bool as π** | MI≈0.40 but sticky tx-grain — **correlation ≠ license**; metrics→0 |
| **`inc` as Avoid / sticky key** | inc0 fence≈7% vs inc1+≈74%; repair-state proxy, not essentialness — **exclude** |
| **Canary as live verb / UnfencedCold tax** | MI≈0.16; true→mostly Unfenced discovery; meta tax on ¬PredictedEssential |
| **`H` membership as Wait OR-door** | MI≈0.25 but redundant; observe/prior only — banned as OR-bool |
| **Morph as Fence actuator** | Storm/Quiet banned as edge actuator; prior decay / warm-start only |
| **`writer_validated` as Bind gate** | A3: Data→Bind even if !validated; tip-identity class Bind delay — exclude |
| **Flat `EdgeKey(ℓ,reader)` without \(k\)/depth as SoT** | Loses call-frame / access ordinal; cannot mix Fence+Unfenced inside one tx |
| EV Await doors / AdaptiveParams-as-θ | Makespan EV is a **feature for learning**, not an Await verb |
| tip-identity Bind gate | Plant hygiene ≠ π |
| OCC-retry as **control plane for contended Region-accesses** | Reincarnation is the **miss path**, not the policy that replaces PCC on known essentials |
| 597 / bn hardcodes | Full-set median matches focus |
| Gate salad / OR-bool π (`force_prefix∨avoid∨essential` sticky) | Signals ≠ Edge state / learned actuators |
| Dead AEC theater on access path | Delete |
| Celebrating abort↓ while ≪OCC | Wall/TPS vs OCC is the bar |
| Unfenced that is **more expensive than OCC** (per access) | Held — canary/Edge tax on ¬PredictedEssential access forbidden |
| Fence-first / residual Bind theater on cold ℓ | v2 failure mode — held deleted |
| **Sparse Detect / Detect-as-optional** | Detect always on at access grain |
| **Late-only Resolve (validate-end / whole-tx storms as first Avoid)** | Avoid/Resolve timely **at the failing access / certified prefix** |
| SuffixRepair-as-default Resolve cost class | Held deleted; prefer RebindThis / FrameSkip / OCC residual reincarnation |
| **Tx-coarse PredictedEssential(\(t\)) or sticky Wait after first hit** | Prediction is per \((\ell,k,\mathrm{morph})\) / typed edge class, not whole-tx |

---

## 1. Field table — enter π / observe / exclude (from 99-block evidence)

**Source:** `lab/notes/specfence-decision-field-selection-from-99.md` (RECOMMENDATION confirmed → now frozen here).

| Field | Enter π? | Evidence (99-block) | Why |
|-------|----------|---------------------|-----|
| **`t` (tx / reader)** | **identity_only** (in \(a\)) | `EdgeKey.reader`; sticky Wait-on-tx banned | Needed in \(a\); must not sticky-Wait whole incarnation |
| **`inc` (incarnation)** | **exclude** (Avoid key) | inc0 fence≈7% vs inc1+≈74%; reinc_share↔wall weak (+0.12); ForcePrefix pathology | Repair-state proxy — harmful Avoid key; **Resolve bookkeeping / observe only** |
| **`k` (effect ordinal)** | **yes** (in \(a\)) | k1_3 fence≈19% vs k16p≈48%; focus 597 tx203 essential at \(k{\approx}6\) | Access-class / PredictedEssential\((\ell,k,\mathrm{morph})\); enables mixed verbs in one tx |
| **`depth` (call depth)** | **yes** (in \(a\)) | d0 fence≈12% vs d4_7≈91%; **deep_share↔wall +0.40** (strongest block corr) | Frame policy; part of \(a\); highly verb-discriminative |
| **`ℓ` (MemoryLocation)** | **yes** (in \(a\)) | All Bind/Avoid keyed by location; hot fan-out ℓ pattern | Primary Region / conflict object |
| **`mode` (R/W)** | **identity_only** (in \(a\)) | `mode_read` MI=0 (maybe_wait is read-only today) | Keep in \(a\); write side is Detect/publish until write Avoid exists |
| **edge kind wr/rw/ww** | **yes** (in \(e_{\mathrm{vis}}\); observe-promote for rw/ww) | Decision path records Wr only today | EdgeVisibility; promote actuators when rw/ww live |
| **writer status** | **yes** (visibility) | validated→Bind; none→Wait\|Unfenced; **wait_no_writer↔wall +0.30** | EdgeVisibility for Bind vs WaitFor vs serial-lane — **not** PredictedEssential gate |
| **published Data?** | **yes** (in \(e_{\mathrm{vis}}\)) | MI(writer_published;verb)≈**0.31**; true→100% Bind | A3 Bind visibility bit |
| **Avoid membership / PredictedEssential** | **yes** (gate) | MI(essential_antidep)≈**0.66** (strongest); avoid_broadcast MI≈0.34; true→100% Fence | Primary learned Avoid predicate at \((\ell,k,\mathrm{morph})\) |
| **independence_certified** | **yes** (gate) | MI≈0.08; true→100% Unfenced | A4 Unfenced / OCC baseline certificate |
| **H membership** | **observe_only** | MI≈0.25 but redundant with avoid/prior | Learning/prior; **banned as Wait OR-door** |
| **canary** | **exclude** | MI≈0.16; true→mostly Unfenced discovery | Meta tax — delete as live verb |
| **gas / opcode class** | **observe_only** | Not in EdgeView; depth/gross-work proxy | Learning / depth proxy |
| **storage vs account / `is_program`** | **observe_only** | `is_program` MI≈**0.005** (noise) | Diagnostic filter |
| **selector / to** | **observe_only** | Not on 99-block decision path | Inter-block morph prior only |
| **morph cluster** | **observe_only** (feature into PredictedEssential; **not** actuator) | Banned Storm/Quiet actuator | Prior decay / warm-start; enters gate as morph label only |
| **prior warm / sticky features** | **observe_only** | MI(prior_warm)≈0.34 ≈ avoid — redundant OR-bool | Feeds PredictedEssential learning; not separate π bit |
| **park / wait history** | **observe_only** | wait_frac↔wall +0.30; 6196166 park lesson | Schedule learning; prefer serial-lane over fleet Wait |
| **validate fail reason** | **observe_only** | Resolve learning (value_stable / true_suffix) | R1a/R1b/E1/B0 selection — not Avoid key |
| **R1/R2/R4 path** | **observe_only** | Resolve ladder / rewind_to_cp | Falsifier + Resolve actuator choice — not Avoid π |
| **force_prefix bool** | **exclude** | MI≈**0.40**; true→95% Wait; rate↔wall **+0.35** | Sticky tx-grain — exclude from live π; metrics→0 |
| **`inc` as Resolve context** | **observe / Resolve only** | reincarnation bookkeeping | Allowed for PrefixSkip / B0 identity; **not** Avoid key |
| **writer_validated as Bind gate** | **exclude** | MI≈0.22 but A3: Data→Bind even if !validated | Tip-identity Bind delay — exclude |
| **`writer_validated` as metric** | **observe_only** | quality counter | Metrics only |

### Observe-only set (learning / metrics inputs — never live OR-bools)

`H` membership · `prior_warm`/`sticky` (as learning inputs, not OR-bools) · gas/opcode class · storage vs account / `is_program` · selector/`to` · morph cluster (prior only) · park/wait history · validate-fail reason · R1/R2/R4 path rates · `inc` as Resolve context · `writer_validated` as metric · rare edge kinds

### Exclude set (banned from live π)

`inc` as Avoid/sticky key · canary as live verb · **`force_prefix` bool as π** · `H` as Wait OR-door · morph as Fence actuator · `writer_validated` as Bind gate · tx-level sticky Wait · flatten `EdgeKey(ℓ,reader)` without `k`/depth

**One-line rationale (from 99-block note):** Verb is already almost completely determined by **PredictedEssential/avoid + published Data + independence**; **`k` and `depth` are the missing identity axes** that make mixed Fence/Unfenced inside one tx possible; **`inc`/`force_prefix`/`canary`/`H-OR` correlate with Fence but are sticky/meta tax** — exclude from live π.

---

## 2. Protocol identity (OCC–PCC hybrid kept)

**Name:** Frozen-Grain Learned OCC–PCC Hybrid SpecFence (architecture **v4.1-frozen**).

SpecFence is an **OCC-cost-class parallel executor** with a **first-class learning PCC overlay** whose **decision grain is frozen π**:

1. **Baseline path = OCC per access** — preset-order MVCC read when ¬PredictedEssential ∧ (independence_certified ∨ no essential gate). No Edge SM tax, no canary, no ForcePrefix sticky on cold accesses.  
2. **Detect always at access** — every storage/account/CALL boundary emits Observe/Publish features. Detect records both **π fields** and **observe-only** features. Detect ≠ Fence.  
3. **Learning (first-class, edge/frame)** — PredictedEssential\((\ell,k,\mathrm{morph})\) / access-class posteriors from **observe inputs**; first-wave broadcast **per access class**, not per-tx sticky Wait. Live gate uses only PredictedEssential ∨ independence_certified.  
4. **Avoid at this access** — on *this* SLOAD/SSTORE/CALL boundary: using \(a\), \(e_{\mathrm{vis}}\), gate → Bind / WaitFor / Region-access serial-lane / ordered admission **iff** PredictedEssential for *this* grain; else Unfenced ≡ OCC for *this* access. Sibling accesses in the same tx may choose differently.  
5. **Timely Resolve at grain** — value-stable **rebind of this read origin**; **frame/suffix skip of certified prefix** (`inc` only as bookkeeping); full reincarnation only when grain identity lost.  
6. **PCC overlay is per-Region-access** — Fence verbs scoped to predicted-essential accesses/edges; OCC cost class for accesses without predicted essential.  
7. **Miss path** — OCC reincarnation of residual work after certified prefix (or full tx only when identity lost) — never invent SuffixRepair / SoftWait / ForcePrefix cost class.

Family: early-visible MVCC + **continuous fine Detect at \(a\)** + **learned timely PCC Avoid/Resolve at edge/frame** + work-conserving schedule + OCC-cheap miss path.

**Not:** tx-level SoftWait meta-CC; whole-tx ForcePrefix π; flattened \((\ell,\mathrm{reader})\) SoT; `inc`-keyed Avoid; Fence-first (v2); CostGate ROI-only (v3); tx-coarse hybrid reading of v4.0; pre-freeze v4.1 with `inc` in Avoid \(a\).

### 2.1 How frozen grain differs from v4.0 / pre-freeze v4.1

| Axis | v4.0 (tx-coarse risk) | Pre-freeze v4.1 finegrain | **v4.1-frozen (this SoT)** |
|------|------------------------|---------------------------|----------------------------|
| Decision unit | “tx / Region set waits” | \(a=(t,\mathrm{inc},k,\mathrm{depth},\ell,\mathrm{mode})\) | **\(a=(t,k,\mathrm{depth},\ell,\mathrm{mode})\)** — **no `inc` in Avoid** |
| Visibility | implicit | typed edge | **\(e_{\mathrm{vis}}=(\mathrm{writer?},\mathrm{published\_Data?},\mathrm{edge\_kind})\)** |
| Gate | PredictedEssential → sticky risk | PredictedEssential\((\ell,k,\mathrm{morph})\) | **PredictedEssential ∨ independence_certified** |
| Avoid | could sticky across tx | per access; mixed OK | **same + exclude set enforced** (`force_prefix`, canary, H-OR, morph actuator, flat EdgeKey, tx sticky Wait, `inc` Avoid) |
| Resolve | incarnation ladder | RebindThis / PrefixSkip / residual | **unchanged intent**; `inc` bookkeeping only |
| Learning | per-tx / per-ℓ sticky | per \((\ell,k,\mathrm{morph})\) | **observe fields → learning only**; live verbs from π only |
| Empirical basis | post-v2 sweeps | effect-RAW + errata | **+ 99-block DecisionFieldAgg @ `a13c4bd`** |

---

## 3. System model

### 3.1 Execution

- Block = ordered txs `0..n-1`. Correct commit order = preset order.  
- Workers = P cores (lab: **8**). Useful parallelism ≤ `min(P, independent ready width at access/frontier)`.  
- Each tx incarnation interprets EVM; **every** storage/account/CALL touch is an **access-event** that **always Detects**, then either **PCC Avoid for this \(a\)** or **OCC proceed for this \(a\)** under frozen π.

### 3.2 Objects (grain SoT)

| Object | Meaning | SoT? |
|--------|---------|------|
| **Access-event \(a\)** | \(a=(t,k,\mathrm{depth},\ell,\mathrm{mode})\) — \(t\) tx, \(k\) effect/access ordinal, \(\mathrm{depth}\) call-frame depth, \(\ell\) location, \(\mathrm{mode}\in\{\mathrm{R},\mathrm{W},\mathrm{CALL},\ldots\}\) | **Yes — primary decision identity** (`inc` **not** here) |
| **EdgeVisibility \(e_{\mathrm{vis}}\)** | \((\mathrm{writer?},\mathrm{published\_Data?},\mathrm{edge\_kind})\) | **Yes — Avoid/Resolve operand** |
| **Gate** | PredictedEssential\((\ell,k,\mathrm{morph})\) ∨ independence_certified | **Yes — live Fence vs Unfenced** |
| **Incarnation `inc`** | reincarnation counter | **Bookkeeping / Resolve context only** — not Avoid key |
| **Location \(\ell\)** | conflict object | Feature / Region carrier |
| **Call-frame / certified prefix** | Contiguous certified accesses from tx start (or last resume \(k\)) | Resolve skip unit |
| **Region (Spec)** | Contended dependency unit over accesses — **not** a tx lock | Fence scope = Region-**access** |
| **Fence (PCC verb)** | Bind / WaitFor / serial-lane / ordered-admission on **this** predicted-essential access/edge | Actuator |
| **Baseline (Unfenced)** | OCC-cost optimistic **for this access** | Default |
| **Miss path** | OCC residual reincarnation when prediction wrong / grain identity lost | Failure |

**Explicit non-SoT (banned as primary π):** see §0 exclude set.

`EdgeKey` in plant **must** retain \((\ell,\mathrm{reader},k,\mathrm{depth})\) (or equivalent access identity aligned with \(a\)). Flattening for metrics is allowed; flattening as control SoT is not. Incarnation may appear on reincarnation maps; it must **not** key Avoid.

### 3.3 Cost model (law)

```
wall = useful_EVM + wait_idle + abort_recovery + protocol_meta

OCC_wall ≈ useful_EVM + reincarnation_recovery

SF_v4.0_tx-coarse ≈ useful_EVM
                   + Σ_tx sticky_PCC_tax
                   + Σ_miss whole_tx_repair

SF_pre-freeze_v4.1_with_inc_Avoid ≈ useful_EVM
                   + risk(inc-keyed sticky ForcePrefix smell)

SF_v4.1_frozen ≈ useful_EVM
          + Σ_{a : PredictedEssential(a)} (timely_PCC_tax_on_a)
          + Σ_{a : ¬PredictedEssential} (OCC_read_meta≈0)
          + Σ_{prediction miss} (OCC_residual_reincarnation after certified prefix)
          + Detect_meta_cheap_per_access
```

**Invariants:**

1. If learning predicts **no** essential accesses, `SF_wall ≡ OCC_wall` (Detect noise only).  
2. If learning predicts essentials **correctly and timely at \(a\)**, PCC tax ≪ avoided abort cascades **and** sibling cold accesses stay OCC-width.  
3. If learning **misses**, Resolve = OCC residual reincarnation — **never** invent SoftWait / SuffixRepair / ForcePrefix / canary / H-OR cost class.  
4. **Ban:** Unfenced **access** cost class > OCC.  
5. **Ban:** one predicted-essential access must not sticky-Wait the **entire** remaining tx body by default.  
6. **Ban:** `inc` must not enlarge Fence rate via repair-state proxy (99-block: inc1+ ≈74% Fence smell).

### 3.4 Success metric

**Primary:** median SF/OCC TPS and wall @8 on all-blocks corrected set.  
**Bar today:** ≈0.319–0.326 / ~3×.  
**Hybrid signature (fine):** fan_out shows **PCC only on predicted star accesses** + **OCC on other accesses in same tx** + **OCC-width on wave independents**; quiet/META_COLD show **SF≡OCC**.  
**Grain falsifiers:** ForcePrefix-as-π count ≈ 0; sticky tx-Wait arms ≈ 0; EdgeKey flatten-without-\(k\) control path = 0; canary live verb = 0; H-OR Wait door = 0; `inc`-keyed Avoid hits = 0; mixed Bind+Unfenced within one tx on fan_out consumers > 0 when morph says so; `wait_no_writer` driven down via serial-lane / ordered-admit with writer identity (99-block: 71 711 / 252 758).

---

## 4. Fine Detect (always on, cheap, at \(a\))

Three Detect layers — keyed through frozen access-event identity:

1. **L_record** — \(\ell\)  
2. **L_access** — \(a=(t,k,\mathrm{depth},\ell,\mathrm{mode})\) (+ `inc` recorded as **observe/Resolve**, not Avoid key)  
3. **L_edge** — \(e_{\mathrm{vis}}\) + publish state

**Laws:**

- **Detect ≠ optional.** Every SLOAD/SSTORE/CALL/BALANCE/… boundary emits features for learning.  
- **Detect ≠ Fence.** Recording does not install WaitFor/Bind until Avoid fires **for this \(a\)** under gate.  
- **Detect must be cheap.** Feature write + EMA; no SoftWait Soft; no canary probe class; no full Edge SM on ¬PredictedEssential **accesses**.  
- Detect feeds: PredictedEssential\((\ell,k,\mathrm{morph})\), hot serial access sets, version-install readiness, ordered-admission heat, quiet/flip decay, **certified-prefix length**, plus observe-only learning inputs listed in §1.

| Stage | Live uses π fields only | Observe-only inputs |
|-------|-------------------------|---------------------|
| Detect | record \(a\), \(e_{\mathrm{vis}}\) | H, prior_warm, gas/opcode, morph prior, park history, … |
| Avoid | gate + \(e_{\mathrm{vis}}\) → verb on **this \(a\)** | — |
| Resolve | RebindThis / PrefixSkip / residual (uses \(k\); `inc` bookkeeping) | validate-fail reason, R-path rates |
| Learning | updates PredictedEssential / independence posteriors | full observe set |

---

## 5. Avoid at access (live verbs from π only)

### 5.1 Decision (primary loop)

```
on access-event a = (t, k, depth, ℓ, mode):   # SLOAD/SSTORE/CALL/…  — inc NOT in Avoid key
  Detect.record(a, e_vis, observe_features)   # ALWAYS, cheap; observe_features → learning only
  update PredictedEssential(ℓ, k, morph) online
  update independence_certified online

  gate := PredictedEssential(ℓ, k, morph) ∨ independence_certified

  if PredictedEssential(ℓ, k, morph) for this a:
    verb := PCC_Avoid_for_a using e_vis:
      published_Data?     → Bind(version) for this read origin
      writer? unpublished → WaitFor(w) | serial-lane | ordered_admit
      # scope = this a / this edge — NOT sticky Wait for rest of tx
      # NEVER: force_prefix, canary, H-OR, morph actuator, writer_validated Bind gate
  else if independence_certified:
    BaselineOCC / Unfenced≡OCC for this a
  else:
    BaselineOCC read for this a                 # literally OCC; no Edge tax
  # sibling a' in same t may take a different verb
  # inc may increment on reincarnation — does NOT re-key Avoid
```

**PCC Avoid verbs (fine Region-access Fence):**

| Verb | When (π fields) | Effect | Scope |
|------|-----------------|--------|-------|
| **Bind(version)** | PredictedEssential ∧ published_Data? | Read certified version | **this \(a\)** |
| **WaitFor(w)** | PredictedEssential ∧ writer? ∧ ¬published; single waiter preferred | Barrier until w publishes **this** \(\ell\) | **this \(a\)** / edge — **not** tx SoftWait |
| **Region-access serial-lane** | Hot star/chain access class | One logical lane on contended accesses | access class / Region |
| **Ordered admission** | Ready-set must respect predicted RAW order | Admit consumer **accesses** after producers | edge frontier |
| **Unfenced≡OCC** | ¬PredictedEssential ∨ independence_certified | OCC-cost proceed | **this \(a\)** |

**Laws:**

- Known / **predicted** essential **access** ⇒ Bind or WaitFor / serial-lane — never optimistic hang on **that** edge.  
- ¬PredictedEssential **access** ⇒ **OCC proceed** — even if another access in the same tx Fenced.  
- Hang-freedom = serial-lane progress or Bind race or steal from independents — **not** SoftWait Soft, **not** ForcePrefix, **not** canary, **not** H-OR.  
- Prefer **serial-lane + ordered admission** over multi-worker WaitFor park (6196166; `wait_no_writer` plant smell).  
- **Inner frame may Fence while outer Unfenced** — `depth` is part of \(a\).  
- **`inc` never changes the Avoid key** for the same \((t,k,\mathrm{depth},\ell,\mathrm{mode})\).

### 5.2 Hot Region-access serialization vs cold parallel

| Partition | Mechanism | Worker policy |
|-----------|-----------|---------------|
| **Predicted-essential access / edge** | PCC Fence on **that** \(a\) via \(e_{\mathrm{vis}}\) | Progress serial lane / Bind; do **not** park fleet on unrelated accesses |
| **¬PredictedEssential / independence_certified** | Baseline OCC | Full P-way parallel |

### 5.3 Explicit deletion of exclude-set Avoid

| Anti-pattern | Fate |
|--------------|------|
| SoftWait Soft / sticky Wait-on-tx after first essential hit | **Delete** as π |
| `ForcePrefix: bool` forcing all subsequent reads in tx | **Delete** as π (debug counter → target 0) |
| `inc` as Avoid / sticky key | **Delete** from Avoid; Resolve bookkeeping only |
| Edge store SoT = map\((\ell,\mathrm{reader})\) only | **Delete**; SoT = \((\ell,\mathrm{reader},k,\mathrm{depth})\) / \(a\) |
| Canary UnfencedCold tax / canary live verb | **Delete** |
| `H` as Wait OR-door | **Delete**; observe/prior only |
| Morph Storm/Quiet as Fence actuator | **Delete**; prior into PredictedEssential only |
| `writer_validated` as Bind gate | **Delete**; Data→Bind (A3) |
| Per-tx AvoidBroadcast that arms Wait for all future \(k\) | **Replace** with **per access-class** first-wave broadcast |

---

## 6. Timely Resolve at grain

| Rank | Name | When | Cost class |
|-----:|------|------|------------|
| **A0** | **Timely Avoid cut** | Mid-access Detect says essential **now** for this \(a\) | Avoid before body waste past \(k\) |
| **R1a** | **RebindThis** | Value-stable / FF match on **this** read origin | Near-zero; rebind **this** edge only |
| **R1b** | **CertifiedPrefixSkip** | Prefix accesses \(0..k^\star-1\) (or frame) certified; conflict at \(k^\star\) | Resume from \(k^\star\) / frame boundary — **not** full tx body |
| **E1** | Early abort + residual reincarnate | PredictedEssential ∧ value will change / dirty known early | Restart **from certified prefix** (OCC-identical residual) |
| **B0** | Baseline reincarnation | Prediction miss / grain identity lost | **OCC-identical** full incarnation (`inc++` bookkeeping) |
| ~~R2~~ | ~~SuffixRepair / whole-tx ForcePrefix repair~~ | **Deleted as default** | Only if lab proves < residual reincarnation; else remove |

**Laws:**

- Prefer **RebindThis** over reincarnation when only the read origin of **this** edge moved and value is stable.  
- Prefer **CertifiedPrefixSkip / frame skip** over full restart when \(k\)-identity preserved.  
- Full reincarnation **only** when grain identity lost — **not** whole-tx SuffixRepair / ForcePrefix default.  
- Do **not** defer first response on predicted essentials to end-of-tx validate storms.  
- **`inc` is Resolve bookkeeping**, not a reason to Fence on re-exec (99-block exclude).  
- Observe-only validate-fail / R-path rates train Resolve rank selection — they are **not** Avoid π keys.

---

## 7. Learning (observe inputs → π actuators)

Learning is **not** an optional ROI gate and **not** a per-tx sticky Wait trainer. It trains Detect posteriors and Avoid/Resolve actuators **at edge/frame grain**. **Live verbs read only π fields**; observe fields are training inputs / metrics.

### 7.1 What trains what

| Signal / feature | Class | Trains | Actuator effect |
|------------------|-------|--------|-----------------|
| abort_density / fan / RAW depth per \((\ell,k,\mathrm{morph})\) | observe → posterior | Detect posterior | **PredictedEssential\((\ell,k,\mathrm{morph})\)** (π gate) |
| first-wave Observe→abort latency **per access class** | observe | Avoid timing | Fire WaitFor/Bind **earlier on that class** — **not** sticky whole-tx Wait |
| Fence_tax_ns vs reincarnation_ns EMA **per access class** | observe | Avoid aggressiveness | Soften/strengthen PCC on class; never SoftWait Soft |
| pack_top hot \(\ell\) / star cover + \(k\)-template | observe | Region-access serial-lane set | Ordered admission + serial lane on **class** |
| writer_done / Done∅Data / published_Data patterns | π visibility + observe | Avoid Bind residual | Only under PredictedEssential **access** |
| value_stable / true_suffix / **prefix_certifiable** rates | observe | Resolve rank | R1a / R1b / E1 / B0 |
| park_idle / BlockingOther / wait_no_writer | observe | Schedule Avoid | Prefer serial-lane over fleet WaitFor |
| quiet / flip / H / prior_warm | observe | Inter-block decay | Protect quiet; do **not** sticky-plant H / Wait OR-door |
| call-frame depth mix | π (`depth` in \(a\)) + observe | Frame policy | Allow mixed verbs inside one tx |
| `inc` reincarnation share | observe / Resolve only | — | **Never** Avoid key |
| force_prefix / canary rates | falsifier | — | Drive to **0**; not training targets for live OR |

### 7.2 First-wave + inter-block (access class, not tx sticky)

**First-wave (intra-block):**

1. Warm Detect from InterBlockPrior (hot \(\ell\), RAW \(k\)-templates, quiet bias, morph) — **observe → PredictedEssential**, not morph/H OR-doors.  
2. As early writers publish (597: writers \(0..38\) on star \(\ell\)), Detect updates PredictedEssential **online per access class** — Avoid engages on consumer accesses matching the template (e.g. \(k{\approx}6\) program SLOAD) **without** arming Wait for unrelated later SLOADs in the same consumer.  
3. Resolve actuators update EMA on each abort (R1a/R1b hit rate, prefix-skip savings).

**Inter-block:**

- Warm-start PredictedEssential priors + hot **access-class** serial sets with flip/quiet decay.  
- Never plant PCC Fence / sticky Wait / H-OR / ForcePrefix sets that flip quiet→fan_out without abort evidence.  
- Priors are performance-only; commit path remains seq≡par TCB.

### 7.3 Explicitly delete (do not “wire harder”)

| Item | Fate |
|------|------|
| Canary probe / canary_reopen as path | **Delete** |
| AEC choose_resolve / αβγδ Await | **Delete** |
| SoftWait meta / engagement Storm π | **Delete** |
| **Tx-level SoftWait / sticky Wait-on-tx** | **Delete** |
| **Whole-tx ForcePrefix bool as π** | **Delete** |
| **`inc` as Avoid key** | **Delete** |
| **`H` as Wait OR-door** | **Delete** |
| **Morph as Fence actuator** | **Delete**; prior only |
| **`writer_validated` as Bind gate** | **Delete** |
| **EdgeKey SoT flatten \((\ell,\mathrm{reader})\)** | **Delete** as control SoT |
| r1_first_bias chasing true_suffix without RebindThis | **Delete** — use E1/B0 residual |
| SuffixRepair-as-default ladder | **Delete** |
| PreferAdmit heat as primary park fix | **Delete** if serial-lane lands |
| CostGate default-deny silence | **Replace** — continuous learned PCC overlay **at \(a\)** |
| Per-tx AvoidBroadcast sticky for all future \(k\) | **Replace** — per access-class broadcast |

---

## 8. End-to-end control loop (π fields for live verbs)

```
begin_block:
  seed Detect priors + PredictedEssential access-class candidates from InterBlockPrior
  (quiet → PredictedEssential≈∅; hot star/chain → warm PCC readiness on (ℓ,k,morph) templates)
  # empty prediction ⇒ pure OCC cost class (Detect still on, Avoid off)
  # do NOT seed: force_prefix, H-OR Wait, canary, morph Storm actuator, inc-sticky Avoid

access_tick a = (t, k, depth, ℓ, mode):     # every SLOAD/SSTORE/CALL/… — no inc in Avoid key
  Detect: record a + e_vis + observe_features (ALWAYS, cheap)
  update PredictedEssential(ℓ, k, morph) online (first-wave, access class)
  update independence_certified online
  if PredictedEssential for this a:
    Avoid(a) := Bind | WaitFor | serial-lane | ordered_admit   # using e_vis; THIS a only
  else:
    BaselineOCC(a) / Unfenced≡OCC                              # no Edge SM, no canary, no ForcePrefix
  # note: other a' in same t independently decide
  # note: observe_features (H, prior_warm, gas, park, …) update learning only

resolve_tick (on early dirty / mid-flight known conflict on edge e of a):
  if value_stable: RebindThis(e)
  else if prefix_certifiable(t, k): CertifiedPrefixSkip / E1 residual reincarnate from k
  else if grain_identity_lost: B0 full reincarnation  # inc++ bookkeeping only
  # do NOT: SoftWait Soft; ForcePrefix whole tx; SuffixRepair default;
  #         H-OR; canary; inc-keyed re-Fence; defer to validate-end only

validate_tick (miss or residual at commit check):
  R1a if value_stable on failing edges
  R1b/E1 if certified prefix remains
  else B0 reincarnate (OCC-identical)

scheduler_tick:
  fill P from independent ready (wave-first)                   # OCC width at frontier
  progress PredictedEssential Region-access serial lanes       # PCC without fleet park
  ordered admission for predicted RAW consumer accesses
  steal: never SoftWait Soft wake storms

end_block:
  update learning EMA (PredictedEssential per (ℓ,k,morph), Avoid tax vs abort savings,
                       Resolve R1a/R1b/E1/B0 rates, park_idle / wait_no_writer,
                       force_prefix_count→0, sticky_tx_wait→0, canary_live→0,
                       H_OR_door→0, inc_avoid_hits→0, flat_edgekey→0, mixed_verb_intra_tx)
  pack_top hot access classes; quiet/flip decay
  emit falsifiers (SF/OCC, soft, await, pcc_fire_at_a, detect_ns, meta_ns,
                   park_idle, miss_reincarnation, exclude-set counters, mixed_verb_intra_tx)
```

Single live question: **for this access-event / EdgeVisibility, does gate predict essential conflict (or certify independence) now?**  
- PredictedEssential → timely fine PCC Avoid/Resolve **on this grain**.  
- independence_certified or ¬PredictedEssential → OCC proceed **on this grain**.  
- Miss → OCC residual reincarnation (cheap).

---

## 9. EVM / pevm map

| EVM / pevm | Frozen-Grain Learned OCC–PCC Hybrid |
|------------|-------------------------------------|
| SLOAD / BALANCE / … | **Detect always** for this \(a\); PCC Avoid **iff** PredictedEssential\((\ell,k,\mathrm{morph})\) |
| SSTORE / publish | Detect publish; Bind consumers of **this** edge when predicted ∧ published_Data |
| CALL / frame enter-exit | \(\mathrm{depth}\) in \(a\); inner/outer verbs may differ |
| MvMemory publish | Bind when PredictedEssential ∧ Data ready **for this read**; else OCC read |
| Tx Ready / Executing / Done | Scheduler may still track tx containers; **π verbs attach to accesses/edges** |
| Validation fail | R1a / R1b / E1 / B0; no SuffixRepair default; no ForcePrefix π |
| Early dirty / ESTIMATE-class | Timely E1 / PrefixSkip when predicted; else OCC path |
| `EdgeKey` | **Must keep** \((\ell,\mathrm{reader},k,\mathrm{depth})\); flatten without \(k\)/depth **banned as SoT** |
| Journal / FF | R1a door on **this** origin; PrefixSkip when certifiable |
| OCC baseline runner | **Same code path** as Unfenced **per access** |
| `edge.rs` SM | Behind PredictedEssential **access**; not ROI silence; not ForcePrefix; not canary |
| `rem.rs` SuffixRepair | Remove from default lean path |
| `learner.rs` | First-class PredictedEssential\((\ell,k,\mathrm{morph})\) + access-class actuators; strip AEC/AdaptiveParams/tx-sticky Wait/`inc` Avoid/H-OR |
| `scheduler.rs` | Wave-first + Region-**access** serial lanes + ordered admission |
| `vm.rs::maybe_wait_specfence` | No-op when ¬PredictedEssential(**this \(a\)**); never tx SoftWait; never `inc`-keyed Fence |
| `force_prefix` plant flag | **Not π**; debug/metrics only → drive to 0 |
| `inc` on incarnation maps | Resolve/reexec bookkeeping only |

---

## 10. Correctness

1. **Preset order** commit serialization.  
2. **Baseline OCC safety** unchanged when PredictedEssential≈∅ on accesses.  
3. **Fence soundness** only claimed on PredictedEssential **accesses/edges** — wrong prediction ⇒ performance only (abort/reincarnate), not wrong commit.  
4. **Hang-freedom:** serial-lane progress or Bind race or independent steal; no SoftWait Soft; no ForcePrefix deadlock theater.  
5. **Independence:** ¬PredictedEssential **access** / independence_certified never WaitFor-park.  
6. **No new speculation:** Unfenced baseline = OCC per access; Fence is barrier on grain, not guess.  
7. **Detect honesty:** missing Detect on an access is a protocol bug; false PredictedEssential is a performance bug.  
8. **Grain honesty:** mixed verbs inside one tx are allowed and expected; tx sticky Wait / `inc` Avoid / ForcePrefix / canary / H-OR / flat EdgeKey / morph actuator / writer_validated Bind gate are protocol bugs.  
9. **Bans:** soft=0, await=0, no bn hardcodes, Unfenced ≰ OCC per access, Detect≠optional, Resolve≠validate-end-only on predicted essentials, full exclude set ≠π.

---

## 11. Explicit principles (held + frozen grain)

1. **Decision grain = frozen π** — \(a=(t,k,\mathrm{depth},\ell,\mathrm{mode})\) + \(e_{\mathrm{vis}}\) + gate; never “tx \(t\) waits”; never `inc` as Avoid key.  
2. **Detect ≠ optional** — always-on, cheap, at \(a\); observe fields recorded, not OR'd into verbs.  
3. **Avoid at access** — Bind/WaitFor/Unfenced for **this** boundary; siblings may differ.  
4. **Timely Resolve at grain** — RebindThis / CertifiedPrefixSkip; full reincarnation only if identity lost.  
5. **Learning at edge/frame** — PredictedEssential\((\ell,k,\mathrm{morph})\); first-wave per access class; observe→train, π→actuate.  
6. **PCC overlay is per-Region-access**; OCC for accesses without predicted essential.  
7. **OCC reincarnation remains the cheap miss path** (residual after prefix when possible).  
8. **Timely Avoid/Resolve** — hybrid effect requires PCC **before** abort storms **on the right grain**.  
9. **Exclude set is non-negotiable** — correlation (high MI) does not license sticky/meta into live π.

---

## 12. Falsifiers from 99-block distribution (+ frozen grain)

| Falsifier | Expect after land | Today / 99-block smell |
|-----------|-------------------|------------------------|
| soft / await | **0** | 0 |
| **tx SoftWait / sticky Wait-on-tx arms** | **0** | sticky patterns in history |
| **ForcePrefix-as-π / force_prefix_unfenced** | **0** | MI≈0.40 plant; rate↔wall +0.35 |
| **`inc` as Avoid key hits** | **0** | inc1+ fence≈74% smell |
| **canary live verb** | **0** | MI≈0.16 discovery tax |
| **H as Wait OR-door** | **0** | MI≈0.25 redundant |
| **morph Fence actuator** | **0** | banned Storm/Quiet |
| **writer_validated Bind gate** | **0** | A3 Data→Bind |
| **flat EdgeKey SoT hits (no k/depth)** | **0** | risk if collapsed |
| writer_done / u_aa / hot_after_fence | **0** | 0 on digests |
| **¬PredictedEssential accesses: meta ≈ OCC** | META_COLD + quiet + cold accesses inside hot txs | canary/meta tax |
| median SF/OCC @8 | **≥ 0.7** then → **≥ 1.0** | ≈0.319–0.326 |
| fan_out median | **≥ 0.6** with PCC on **star accesses only** | ~0.298 |
| worst N3 (19807137) | **≫ 0.08** via timely Bind/serial on star **accesses** + residual OCC | ~0.076 |
| park_idle / wait_no_writer on fan_out | **≪ 0.25** via serial-lane not fleet WaitFor | wait_no_writer 71 711/252 758; 6196166 ≈1.19 |
| quiet cohort | **stay ≥1** | ~1.10 |
| Detect coverage | **100%** of storage/account/CALL boundaries | — |
| PCC fire timing | **before** abort-storm peak on predicted **access class** | late / sparse / sticky |
| **mixed verb intra-tx** on fan_out consumers | **>0** when morph has hot+cold accesses | often sticky without grain SoT |
| SuffixRepair default count | **≈ 0** | rewind dominant |
| PrefixSkip / RebindThis share of repairs | **rises**; full B0 only on identity loss | R2/full dominate |

Distribution must stay **fully classified** after land (every block in a mode).

---

## 13. Single-iteration land list (no P0/P1/P2)

One coherent cut — **all required together** (frozen-grain hybrid):

1. **Unify Unfenced baseline with OCC path per access** — same read/validate/residual reincarnate; remove canary + Edge SM from ¬PredictedEssential **accesses** (`vm.rs`, `edge.rs` gate).  
2. **Frozen access-event SoT** — plant decisions keyed by \(a=(t,k,\mathrm{depth},\ell,\mathrm{mode})\) + \(e_{\mathrm{vis}}\) + gate; **ban** flat \((\ell,\mathrm{reader})\) control SoT; **`inc` not in Avoid key**.  
3. **Always-on cheap Detect at every access boundary** — L_record/L_access/L_edge; record observe-only for learning (`vm.rs` / plant).  
4. **First-class learning → PredictedEssential\((\ell,k,\mathrm{morph})\) ∨ independence_certified** — intra first-wave **per access class** + InterBlockPrior; strip AEC/AdaptiveParams/tx-sticky Wait/`inc` Avoid/H-OR (`learner.rs`).  
5. **Timely PCC Avoid at this access** — Bind / WaitFor / Region-access serial-lane / ordered admission when PredictedEssential(**this \(a\)**) using \(e_{\mathrm{vis}}\); else OCC; allow mixed verbs in one tx (`edge.rs`, `scheduler.rs`, `vm.rs`).  
6. **Delete exclude-set π** — ForcePrefix-as-π, tx SoftWait, canary live verb, H Wait OR-door, morph Fence actuator, writer_validated Bind gate, `inc` Avoid key; counters remain as falsifiers driven to 0.  
7. **Hot Region-access serial lane + ordered admission** — replace fleet WaitFor-park / wait_no_writer for predicted star/chain **access classes** (`scheduler.rs`).  
8. **Timely Resolve at grain** — RebindThis / CertifiedPrefixSkip / E1 residual; delete SuffixRepair-as-default (`pevm.rs`, `rem.rs`); `inc` bookkeeping only.  
9. **Miss path = OCC residual reincarnation** — full B0 only on grain identity loss.  
10. **Strip dead theater** — AEC/AdaptiveParams π, SoftWait Soft, Storm edge, PreferAdmit-as-primary, canary_reopen, CostGate-only-silence, sticky AvoidBroadcast.  
11. **Quiet / ¬PredictedEssential protection** — never seed PCC / sticky Wait / H-OR on quiet priors; Detect still on.  
12. **Falsifier suite** — all-blocks SF/OCC + soft/await + detect coverage + pcc_fire_at_a timing + meta_ns + park_idle/wait_no_writer + full exclude-set counters=0 + mixed_verb_intra_tx + mode census.  
13. **Docs** — this SoT AUTHORITATIVE; point to `specfence-decision-field-selection-from-99.md`; banners on pre-freeze v4.1 / v4.0; v2/v3 historical.

No staged P0/P1/P2. Partial land (PCC without OCC-identical baseline accesses, or Detect without timely Avoid-at-\(a\), or learning without edge/frame actuators, or Resolve still whole-tx default, or any exclude-set field wired as live OR) is a **non-land**.

---

## 14. Worked example — opcode / access timeline (14689597 tx203 + star writer)

**Block:** `14689597` (597), morph=`fan_out`, wave≈434, star location \(\ell^\star=85335018835337005\), writers \(0..38\), ~448 program consumers.  
**Evidence:** `lab/results/effect-raw-deeper-b14689597.json`; 99-block DecisionFieldAgg @ `a13c4bd`.

### 14.1 Producer side (fan_out writer class — tx38)

| \(k\) (effect) | mode | \(\ell\) | Frozen decision | Why |
|----------------|------|----------|-----------------|-----|
| early account / cold slots | R/W | ≠\(\ell^\star\) | **Unfenced ≡ OCC** | ¬PredictedEssential access class |
| \(k{\approx}6\) | R | \(\ell^\star\) | **Bind** if published_Data / **WaitFor(37)** if predicted ∧ unpublished | PredictedEssential\((\ell^\star,k{\approx}6,\mathrm{fan\_out})\) + \(e_{\mathrm{vis}}\) |
| later body SLOADs | R | private / cold | **Unfenced ≡ OCC** | same tx, different \(a\) — **must not** sticky-Wait / ForcePrefix |
| \(k{=}34\) | W | \(\ell^\star\) | Publish Data; Detect broadcast to **access class** consumers | first-wave: arm PredictedEssential for consumer template \(k{\approx}6\), **not** whole consumer txs |

**Anti-pattern (banned):** ForcePrefix/sticky Wait / `inc`-keyed re-Fence / H-OR / canary on subsequent reads.

### 14.2 Consumer side — **tx203**

| Tick | Access \(a\) | Detect | Gate | Avoid / path | Resolve if dirty |
|------|--------------|--------|------|--------------|------------------|
| \(k{=}0..5\) | cold / setup | record \(a\), \(e_{\mathrm{vis}}\), observe | ¬PredictedEssential | **OCC Unfenced** | — |
| **\(k{=}6\)** SLOAD \(\ell^\star\) | \(a=(203,6,\mathrm{depth},\ell^\star,\mathrm{R})\) | typed RAW ← writer 38 | **PredictedEssential** (fan_out star) | If Data: **Bind(38)**; if unpublished: **WaitFor(38)** or serial-lane — **this \(a\) only** | **RebindThis** / **PrefixSkip from \(k{=}6\)** / E1 — **not** SuffixRepair; reincarnation bumps `inc` bookkeeping only |
| \(k{=}7..\) | other \(\ell\) | record | typically **no** | **OCC Unfenced** (same tx!) | B0 only if identity lost |

### 14.3 What banned / pre-freeze moves would do wrong on tx203

| Move | Failure |
|------|---------|
| “tx203 waits” after first hot observe | Parks remaining opcode-seconds (WAIT_PARK) |
| ForcePrefix=true on tx203 | Later accesses pay Fence tax (exclude; MI trap) |
| EdgeKey \((\ell^\star,203)\) without \(k\) | Cannot tell \(k{=}6\) star read from later cold read |
| Avoid keyed by `inc` | Reexec Fence≈74% smell — repair proxy not essentialness |
| H-OR / canary / morph actuator | Sticky/meta tax; exclude set |
| Whole-tx reincarnation on value change | Re-pays \(k{=}0..5\); loses CertifiedPrefixSkip |
| Per-tx AvoidBroadcast sticky | Arms Wait for access classes that should stay OCC |

### 14.4 Quiet / META_COLD / chain / park (compressed)

- **Quiet / META_COLD:** all access ticks ¬PredictedEssential → SF≡OCC; Detect cheap; no H-OR seed.  
- **Long chain:** PredictedEssential along RAW **access** depth; ordered admission on edges; off-chain accesses OCC.  
- **Park worst (6196166) / wait_no_writer:** serial-lane on predicted **accesses** with writer identity, never 8-way BlockingOther Soft/Wait.  
- **Prediction miss:** false positive = small PCC tax on **those accesses**; false negative = OCC residual reincarnation → up-weight that access class.

---

## 15. Pause

**No protocol coding in this task.** User confirmed field selection (`可以，改写吧`); this note **freezes** the grain SoT. Implementation map only after explicit go-ahead to code. Design-only land list in §13.

---

## Appendix A — Evidence pointers

- **Empirical field selection (basis for freeze):** `lab/notes/specfence-decision-field-selection-from-99.md` @ `a13c4bd`  
- Selection / aggregate / sweep JSON: `lab/results/decision-field-99-selection.json`, `decision-field-99-aggregate.json`, `decision-field-99-sf-occ-sweep.json`  
- Corrected summary: `lab/results/arch-v2-all-blocks-sf-occ-sweep-corrected-summary.json`  
- Deep evidence: `lab/notes/specfence-post-v2-all-blocks-deep-evidence.md`  
- Effect-RAW (tx203 / star): `lab/results/effect-raw-deeper-b14689597.json`  
- Per-tx process: `lab/results/post-subgrain-per-tx-597-c8.json`  
- Pre-freeze v4.1 (superseded grain): `lab/notes/specfence-complete-architecture-v4-finegrain.md`  
- v4.0 hybrid identity parent (superseded grain): `lab/notes/specfence-complete-architecture-v4-occ-pcc-hybrid.md`  
- Tx-grain errata: `lab/notes/specfence-v4-txgrain-errata.md`  
- Prior v3/v2 SoTs: historical  
- First-principles REM (aligned intent): `lab/notes/specfence-cc-architecture-v4-first-principles.md`
