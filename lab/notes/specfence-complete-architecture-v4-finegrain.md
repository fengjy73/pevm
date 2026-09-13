# SpecFence complete architecture v4.1 — Fine-Grain Learned OCC–PCC Hybrid (standalone SoT)

> **SUPERSEDED (grain SoT):** This is the **pre-freeze v4.1** fine-grain sketch (\(a\) still included `inc`). **AUTHORITATIVE** frozen grain lives in `lab/notes/specfence-complete-architecture-v4-frozen-grain.md` (user confirmed field selection from `specfence-decision-field-selection-from-99.md` @ `a13c4bd`). Hybrid OCC–PCC *identity* is absorbed there; do **not** implement from this file.


**Date:** 2026-09-13 (Asia/Shanghai, UTC+8)  
**Status:** **SUPERSEDED** (pre-freeze v4.1) — historical grain sketch only; see frozen SoT.  
**Branch / HEAD at write:** `cursor/specfence-complete-cc-63b0` @ `10c7249`  
**Supersedes (grain reading):** `lab/notes/specfence-complete-architecture-v4-occ-pcc-hybrid.md` — hybrid *identity* (OCC default + learned PCC overlay) **kept**; **decision / Avoid / Resolve / learning grain** pushed from transaction-level policy to **access-event / typed-edge / call-frame**.  
**Errata (why tx-grain hybrid is not enough):** `lab/notes/specfence-v4-txgrain-errata.md`  
**Evidence base:** post-v2 all-blocks SF/OCC@8 (n≈98, median SF/OCC **0.326**); focus 14689597 / 19606599 / 19469097; effect-RAW deep `effect-raw-deeper-b14689597.json`; per-tx `post-subgrain-per-tx-597-c8.json`  
**Vocab (frozen):** **Spec = Region** (not “speculate”). **Fence** = Bind / WaitFor / serial-lane / ordered-admission barriers on **Region-accesses**. **Unfenced** = optimistic — literally **OCC-cost for this access** when ¬PredictedEssential. Product name SpecFence stays.

This document is the **v4.1 fine-grain standalone**. It does **not** reopen v3 CostGate-as-face or v2 Fence-first. It answers: *hybrid at what grain?* — **access-event and typed edge**, never “tx *t* waits / ForcePrefix / sticky Wait” as primary π.

---

## Essence test (ONE paragraph)

**Fine-Grain Learned OCC–PCC Hybrid SpecFence** keeps OCC as the **default cost class per access**, and runs **always-on Detect + learning-driven timely PCC Avoid/Resolve** — but every control decision is keyed by an **access-event** \(a=(t,\mathrm{inc},k,\mathrm{depth},\ell,\mathrm{mode})\) and a **typed edge**, not by a transaction sticky bit. On each SLOAD / SSTORE / CALL / BALANCE boundary the plant chooses Bind / WaitFor / Unfenced **for this access only**; other accesses in the same tx (outer frame Unfenced, inner CALL Fence, later cold SLOAD OCC) may differ. Timely Resolve rebinds **this** read origin when value-stable, skips a **certified access/frame prefix**, and full-reincarnates only when grain identity is lost — never whole-tx SuffixRepair / SoftWait as default. Learning features and actuators are at **edge / frame / access-class** (PredictedEssential\((\ell,k,\mathrm{morph})\), first-wave broadcast per access class), not per-tx sticky Wait. Prediction miss ⇒ OCC reincarnation of the failing grain’s residual — still the cheap failure path.

**Decision grain one-liner:** *Primary π unit = access-event \(a=(t,\mathrm{inc},k,\mathrm{depth},\ell,\mathrm{mode})\) + typed edge — never “tx \(t\) waits.”*

**Hybrid one-liner:** *OCC by default **per access**; learned fine PCC Always-Detect + timely Avoid/Resolve on predicted-essential **Region-accesses / edges**; reincarnate cheap when prediction misses.*

---

## 0. Hard bans (non-negotiable)

| Ban | Why |
|-----|-----|
| SoftWait Soft storms | Wake≪reabort; soft=0 everywhere |
| **Tx-level SoftWait / sticky Wait-on-tx** | **v4.1** — Wait is an **access/edge** actuator, not a tx sticky bit that parks the whole incarnation |
| **Whole-tx `ForcePrefix` bool as π** | **v4.1** — ForcePrefix collapses many edges into one tx flag; invents serial-lane theater without grain identity |
| **Edge key flatten \((\ell,\mathrm{reader})\) without \(k\)/depth as SoT** | **v4.1** — loses call-frame / access ordinal; cannot mix Fence+Unfenced inside one tx |
| EV Await doors / AdaptiveParams-as-θ | Makespan EV is a **feature for learning**, not an Await verb |
| tip-identity Bind gate | Plant hygiene ≠ π |
| OCC-retry / Block-STM reincarnation as **control plane for contended Region-accesses** | Reincarnation is the **miss path**, not the policy that replaces PCC on known essentials |
| Morph Storm/Quiet as edge actuator | Morphology feeds learning prior only |
| 597 / bn hardcodes | Full-set median matches focus |
| Gate salad / OR-bool π | Signals ≠ Edge state / learned actuators |
| Dead AEC theater on access path | Delete |
| Celebrating abort↓ while ≪OCC | Wall/TPS vs OCC is the bar |
| Unfenced that is **more expensive than OCC** (per access) | Held — canary/Edge tax on ¬PredictedEssential access forbidden |
| Fence-first / residual Bind theater on cold ℓ | v2 failure mode — held deleted |
| **Sparse Detect / Detect-as-optional** | Detect always on at access grain |
| **Late-only Resolve (validate-end / whole-tx storms as first Avoid)** | Avoid/Resolve timely **at the failing access / certified prefix** |
| SuffixRepair-as-default Resolve cost class | Held deleted; prefer RebindThis / FrameSkip / OCC residual reincarnation |
| **Tx-coarse PredictedEssential(\(t\)) or sticky Wait after first hit** | **v4.1** — prediction is per \((\ell,k,\mathrm{morph})\) / typed edge class, not whole-tx |

---

## 1. Protocol identity

**Name:** Fine-Grain Learned OCC–PCC Hybrid SpecFence (architecture **v4.1**).

SpecFence is an **OCC-cost-class parallel executor** with a **first-class learning PCC overlay** whose **decision grain is the access-event / typed edge / call-frame**:

1. **Baseline path = OCC per access** — preset-order MVCC read on ¬PredictedEssential accesses. No Edge SM tax, no canary, no ForcePrefix sticky on cold accesses.  
2. **Detect always at access** — every storage/account/CALL boundary emits Observe/Publish features keyed by \(a\) and typed edge. Detect ≠ Fence.  
3. **Learning (first-class, edge/frame)** — PredictedEssential\((\ell,k,\mathrm{morph})\) / access-class posteriors; first-wave broadcast **per access class**, not per-tx sticky Wait.  
4. **Avoid at this access** — on *this* SLOAD/SSTORE/CALL boundary: Bind / WaitFor / Region-access serial-lane / ordered admission **iff** PredictedEssential for *this* grain; else Unfenced ≡ OCC for *this* access. Sibling accesses in the same tx may choose differently (inner frame Fence, outer Unfenced).  
5. **Timely Resolve at grain** — value-stable **rebind of this read origin**; **frame/suffix skip of certified prefix**; full reincarnation only when grain identity lost.  
6. **PCC overlay is per-Region-access** — Fence verbs scoped to predicted-essential accesses/edges; OCC cost class for accesses without predicted essential.  
7. **Miss path** — OCC reincarnation of residual work after certified prefix (or full tx only when identity lost) — never invent SuffixRepair / SoftWait cost class.

Family: early-visible MVCC + **continuous fine Detect at \(a\)** + **learned timely PCC Avoid/Resolve at edge/frame** + work-conserving schedule + OCC-cheap miss path.

**Not:** tx-level SoftWait meta-CC; whole-tx ForcePrefix π; flattened \((\ell,\mathrm{reader})\) SoT; Fence-first (v2); CostGate ROI-only (v3); tx-coarse hybrid reading of v4.0.

### 1.1 How v4.1 differs from v4.0 (hybrid kept, grain pushed)

| Axis | v4.0 OCC–PCC Hybrid (tx-coarse reading risk) | **v4.1 Fine-Grain Hybrid** |
|------|-----------------------------------------------|----------------------------|
| Decision unit | Often read as “tx / Region set waits” | **Access-event \(a\) + typed edge** |
| Avoid | PredictedEssential → PCC (could sticky across tx) | **Per access** Bind/WaitFor/Unfenced; mixed modes inside one tx |
| Resolve | Timely R1/E1/B0 still framed as incarnation ladder | **RebindThis / CertifiedPrefixSkip / reincarnate residual**; full reincarnation last |
| Learning | PredictedEssential\((\ell,\ldots)\) + first-wave | **PredictedEssential\((\ell,k,\mathrm{morph})\)**; broadcast per **access class** |
| PCC scope | “Predicted-essential Region” | **Per-Region-access**; OCC for other accesses in same tx |
| Ban additions | SoftWait Soft | SoftWait Soft **+ tx SoftWait + ForcePrefix-as-π + flat EdgeKey SoT** |

**User critique absorbed:** *还要再细一些，不能只是交易级别* — Detect / Avoid / timely Resolve / learning must sit on **access-event / edge / call-frame** SoT, not transaction-level policy.

---

## 2. System model

### 2.1 Execution

- Block = ordered txs `0..n-1`. Correct commit order = preset order.  
- Workers = P cores (lab: **8**). Useful parallelism ≤ `min(P, independent ready width at access/frontier)`.  
- Each tx incarnation interprets EVM; **every** storage/account/CALL touch is an **access-event** that **always Detects**, then either **PCC Avoid for this \(a\)** or **OCC proceed for this \(a\)**.

### 2.2 Objects (grain SoT)

| Object | Meaning | SoT? |
|--------|---------|------|
| **Access-event \(a\)** | \(a=(t,\mathrm{inc},k,\mathrm{depth},\ell,\mathrm{mode})\) — \(t\) tx, \(\mathrm{inc}\) incarnation, \(k\) effect/access ordinal, \(\mathrm{depth}\) call-frame depth, \(\ell\) location, \(\mathrm{mode}\in\{\mathrm{R},\mathrm{W},\mathrm{CALL},\ldots\}\) | **Yes — primary decision grain** |
| **Typed edge \(e\)** | Detect observation: writer?, kind (RAW/WAW/…), publish state (Estimate/Data/…), class (program/handler) | **Yes — Avoid/Resolve operand** |
| **Location \(\ell\)** | conflict object | Feature / Region carrier |
| **Call-frame / certified prefix** | Contiguous certified accesses from tx start (or last resume \(k\)) | Resolve skip unit |
| **Region (Spec)** | Contended dependency unit over accesses (hot \(\ell\), RAW star/chain) — **not** a tx lock | Fence scope = Region-**access** |
| **PredictedEssential** | Learning posterior on \((\ell,k,\mathrm{morph})\) / access class / typed edge template | Avoid gate |
| **Fence (PCC verb)** | Bind / WaitFor / serial-lane / ordered-admission on **this** predicted-essential access/edge | Actuator |
| **Baseline (Unfenced)** | OCC-cost optimistic **for this access** when ¬PredictedEssential | Default |
| **Miss path** | OCC residual reincarnation when prediction wrong / grain identity lost | Failure |

**Explicit non-SoT (banned as primary π):**

- “tx \(t\) waits” / sticky Wait bit on incarnation  
- whole-tx `ForcePrefix: bool`  
- Edge map keyed only by \((\ell,\mathrm{reader})\) without \(k\)/depth  

`EdgeKey` in plant **must** retain \((\ell,\mathrm{reader},k,\mathrm{depth})\) (or equivalent access identity). Flattening for metrics is allowed; flattening as control SoT is not.

### 2.3 Cost model (law)

```
wall = useful_EVM + wait_idle + abort_recovery + protocol_meta

OCC_wall ≈ useful_EVM + reincarnation_recovery

SF_v4.0_tx-coarse ≈ useful_EVM
                   + Σ_tx sticky_PCC_tax          # over-waits cold accesses in same tx
                   + Σ_miss whole_tx_repair       # loses certified prefix

SF_v4.1 ≈ useful_EVM
          + Σ_{a : PredictedEssential(a)} (timely_PCC_tax_on_a)
          + Σ_{a : ¬PredictedEssential} (OCC_read_meta≈0)
          + Σ_{prediction miss} (OCC_residual_reincarnation after certified prefix)
          + Detect_meta_cheap_per_access
```

**Invariants:**

1. If learning predicts **no** essential accesses, `SF_wall ≡ OCC_wall` (Detect noise only).  
2. If learning predicts essentials **correctly and timely at \(a\)**, PCC tax ≪ avoided abort cascades **and** sibling cold accesses stay OCC-width.  
3. If learning **misses**, Resolve = OCC residual reincarnation — **never** invent SoftWait / SuffixRepair / ForcePrefix cost class.  
4. **Ban:** Unfenced **access** cost class > OCC.  
5. **Ban:** one predicted-essential access must not sticky-Wait the **entire** remaining tx body by default.

### 2.4 Success metric

**Primary:** median SF/OCC TPS and wall @8 on all-blocks corrected set.  
**Bar today:** 0.326 / 3.05×.  
**Hybrid signature (fine):** fan_out shows **PCC only on predicted star accesses** (e.g. consumer \(k{=}6\) SLOAD of hot \(\ell\)) + **OCC on other accesses in same tx** + **OCC-width on wave independents**; quiet/META_COLD show **SF≡OCC**.  
**Grain falsifiers:** ForcePrefix-as-π count ≈ 0; sticky tx-Wait arms ≈ 0; EdgeKey flatten-without-\(k\) control path = 0; mixed Bind+Unfenced within one tx on fan_out consumers > 0 when morph says so.

---

## 3. Fine Detect (always on, cheap, at \(a\))

Three Detect layers — **all keyed through access-event identity**:

1. **L_record** — \(\ell\)  
2. **L_access** — \(a=(t,\mathrm{inc},k,\mathrm{depth},\ell,\mathrm{mode})\)  
3. **L_edge** — typed edge + publish state (writer, RAW/WAW, program/handler, Data/Estimate)

**Laws:**

- **Detect ≠ optional.** Every SLOAD/SSTORE/CALL/BALANCE/… boundary emits features for learning.  
- **Detect ≠ Fence.** Recording \(e\) does not install WaitFor/Bind until Avoid fires **for this \(a\)**.  
- **Detect must be cheap.** Feature write + EMA; no SoftWait Soft; no canary probe class; no full Edge SM on ¬PredictedEssential **accesses**.  
- Detect feeds: PredictedEssential\((\ell,k,\mathrm{morph})\), hot serial access sets, version-install readiness, ordered-admission heat, quiet/flip decay, **certified-prefix length**.

| Stage | Tx-coarse hybrid (forbidden reading) | **v4.1 Fine-Grain** |
|-------|--------------------------------------|---------------------|
| Detect | “tx touched hot ℓ” | **this \(a\)** + typed edge |
| Avoid | sticky Wait after first hit | **verb for this \(a\) only** |
| Resolve | whole-tx reincarnate / SuffixRepair | **RebindThis / PrefixSkip / residual** |
| Learning | per-tx or per-ℓ sticky | **per \((\ell,k,\mathrm{morph})\) / access class** |

---

## 4. Avoid at access (PCC verbs for **this** \(a\); else OCC)

### 4.1 Decision (primary loop)

```
on access-event a = (t, inc, k, depth, ℓ, mode):   # SLOAD/SSTORE/CALL/…
  Detect.record(a, typed_edge_candidates)          # ALWAYS, cheap
  update PredictedEssential(ℓ, k, morph) online    # first-wave / access class

  if PredictedEssential(ℓ, k, morph) for this a:
    verb := PCC_Avoid_for_a:
      Published Data     → Bind(version) for this read origin
      Unpublished anti-dep → WaitFor(w) | serial-lane(pred access) | ordered_admit
      # scope = this a / this edge — NOT sticky Wait for rest of tx
  else:
    BaselineOCC read for this a                    # literally OCC; no Edge tax
  # sibling a' in same t may take a different verb
```

**PCC Avoid verbs (fine Region-**access** Fence):**

| Verb | When | Effect | Scope |
|------|------|--------|-------|
| **Bind(version)** | Producer Data published / value-install ready for **this** read | Read certified version | **this \(a\)** |
| **WaitFor(w)** | Essential anti-dep; producer not ready; single waiter preferred | Barrier until w publishes **this** \(\ell\) | **this \(a\)** / edge — **not** tx SoftWait |
| **Region-access serial-lane** | Hot star/chain access class | One logical lane on contended accesses; other workers on independent **accesses/txs** | access class / Region |
| **Ordered admission** | Ready-set must respect predicted RAW order | Admit consumer **accesses** after producers | edge frontier |

**Laws:**

- Known / **predicted** essential **access** ⇒ Bind or WaitFor / serial-lane — never optimistic hang on **that** edge.  
- ¬PredictedEssential **access** ⇒ **OCC proceed** — even if another access in the same tx Fenced.  
- Hang-freedom = serial-lane progress or Bind race or steal from independents — **not** SoftWait Soft, **not** ForcePrefix whole-tx.  
- Prefer **serial-lane + ordered admission** over multi-worker WaitFor park (6196166 lesson).  
- **Inner frame may Fence while outer Unfenced** (and vice versa) — call-frame depth is part of \(a\).

### 4.2 Hot Region-access serialization vs cold parallel

| Partition | Mechanism | Worker policy |
|-----------|-----------|---------------|
| **Predicted-essential access / edge** | PCC Fence on **that** \(a\) | Progress serial lane / Bind; do **not** park fleet on unrelated accesses |
| **¬PredictedEssential access (same or other tx)** | Baseline OCC | Full P-way parallel |

This is the **hybrid schedule face at fine grain**: PCC on learned critical **accesses**; OCC width on the rest — **including other accesses inside a partially-Fenced tx**.

### 4.3 Explicit deletion of tx-coarse Avoid

| Anti-pattern | Fate |
|--------------|------|
| SoftWait Soft / sticky Wait-on-tx after first essential hit | **Delete** as π |
| `ForcePrefix: bool` forcing all subsequent reads in tx | **Delete** as π (may remain as debug counter → target 0) |
| Edge store SoT = map\((\ell,\mathrm{reader})\) only | **Delete**; SoT = \((\ell,\mathrm{reader},k,\mathrm{depth})\) / \(a\) |
| Canary UnfencedCold tax on ¬PredictedEssential access | **Delete** |
| Per-tx AvoidBroadcast that arms Wait for all future \(k\) | **Replace** with **per access-class** first-wave broadcast |

---

## 5. Timely Resolve at grain

| Rank | Name | When | Cost class |
|-----:|------|------|------------|
| **A0** | **Timely Avoid cut** | Mid-access Detect says essential **now** for this \(a\) | Avoid before body waste past \(k\) |
| **R1a** | **RebindThis** | Value-stable / FF match on **this** read origin | Near-zero; rebind **this** edge only |
| **R1b** | **CertifiedPrefixSkip** | Prefix accesses \(0..k^\star-1\) (or frame) certified; conflict at \(k^\star\) | Resume from \(k^\star\) / frame boundary — **not** full tx body |
| **E1** | Early abort + residual reincarnate | PredictedEssential ∧ value will change / dirty known early | Restart **from certified prefix** (OCC-identical residual) — sooner than validate-end |
| **B0** | Baseline reincarnation | Prediction miss / grain identity lost (control-flow diverged; \(k\) identity meaningless) | **OCC-identical** full incarnation |
| ~~R2~~ | ~~SuffixRepair / whole-tx ForcePrefix repair~~ | **Deleted as default** | Only if lab proves < residual reincarnation; else remove |

**Laws:**

- Prefer **RebindThis** over reincarnation when only the read origin of **this** edge moved and value is stable.  
- Prefer **CertifiedPrefixSkip / frame skip** over full restart when \(k\)-identity preserved.  
- Full reincarnation **only** when grain identity lost (PC/depth/effect ordinal no longer meaningful) — **not** whole-tx SuffixRepair default.  
- Do **not** defer first response on predicted essentials to end-of-tx validate storms.  
- Incarnation carry / residual maps may remain **inside PredictedEssential access classes** only; baseline accesses rediscover via OCC.

---

## 6. Learning (edge / frame / access-class actuators)

Learning is **not** an optional ROI gate and **not** a per-tx sticky Wait trainer. It trains Detect posteriors and Avoid/Resolve actuators **at edge/frame grain**.

### 6.1 What trains what

| Signal / feature | Trains | Actuator effect |
|------------------|--------|-----------------|
| abort_density / fan / RAW depth per \((\ell,k,\mathrm{morph})\) | Detect posterior | **PredictedEssential\((\ell,k,\mathrm{morph})\)** |
| first-wave Observe→abort latency **per access class** | Avoid timing | Fire WaitFor/Bind **earlier on that class** next access / next block — **not** sticky whole-tx Wait |
| Fence_tax_ns vs reincarnation_ns EMA **per access class** | Avoid aggressiveness | Soften/strengthen PCC on class; never SoftWait Soft |
| pack_top hot \(\ell\) / star cover + \(k\)-template | Region-access serial-lane set | Ordered admission + serial lane on **class** |
| writer_done / Done∅Data patterns | Avoid Bind residual | Only under PredictedEssential **access** |
| value_stable / true_suffix / **prefix_certifiable** rates | Resolve rank | R1a / R1b / E1 / B0 |
| park_idle / BlockingOther | Schedule Avoid | Prefer serial-lane over fleet WaitFor |
| quiet / flip priors | Inter-block decay | Protect quiet; do not sticky-plant H / Wait into quiet neighbors |
| call-frame depth mix (Fence inner / Unfenced outer) | Frame policy | Allow mixed verbs inside one tx |

### 6.2 First-wave + inter-block (access class, not tx sticky)

**First-wave (intra-block):**

1. Warm Detect from InterBlockPrior (hot \(\ell\), RAW \(k\)-templates, quiet bias, morph).  
2. As early writers publish (597: writers \(0..38\) on star \(\ell\)), Detect updates PredictedEssential **online per access class** — Avoid engages on consumer accesses matching the template (e.g. \(k{\approx}6\) program SLOAD) **without** arming Wait for unrelated later SLOADs in the same consumer.  
3. Resolve actuators update EMA on each abort (R1a/R1b hit rate, prefix-skip savings).

**Inter-block:**

- Warm-start PredictedEssential priors + hot **access-class** serial sets with flip/quiet decay.  
- Never plant PCC Fence / sticky Wait sets that flip quiet→fan_out without abort evidence.  
- Priors are performance-only; commit path remains seq≡par TCB.

### 6.3 Explicitly delete (do not “wire harder”)

| Item | Fate |
|------|------|
| Canary probe / canary_reopen as path | **Delete** |
| AEC choose_resolve / αβγδ Await | **Delete** |
| SoftWait meta / engagement Storm π | **Delete** |
| **Tx-level SoftWait / sticky Wait-on-tx** | **Delete** |
| **Whole-tx ForcePrefix bool as π** | **Delete** |
| **EdgeKey SoT flatten \((\ell,\mathrm{reader})\)** | **Delete** as control SoT |
| r1_first_bias chasing true_suffix value changes without RebindThis | **Delete** — use E1/B0 residual |
| SuffixRepair-as-default ladder | **Delete** |
| PreferAdmit heat as primary park fix | **Delete** if serial-lane lands |
| Morph heuristic as Fence actuator | **Delete**; prior only |
| CostGate default-deny silence | **Replace** — continuous learned PCC overlay **at \(a\)** |
| Learning that only moves counters not wall class | **Delete** |
| Per-tx AvoidBroadcast sticky for all future \(k\) | **Replace** — per access-class broadcast |

---

## 7. End-to-end control loop (access / scheduler ticks only)

```
begin_block:
  seed Detect priors + PredictedEssential access-class candidates from InterBlockPrior
  (quiet → PredictedEssential≈∅; hot star/chain → warm PCC readiness on (ℓ,k,morph) templates)
  # empty prediction ⇒ pure OCC cost class (Detect still on, Avoid off)

access_tick a = (t, inc, k, depth, ℓ, mode):     # every SLOAD/SSTORE/CALL/…
  Detect: record a + typed edge features (ALWAYS, cheap)
  update PredictedEssential(ℓ, k, morph) online (first-wave, access class)
  if PredictedEssential for this a:
    Avoid(a) := Bind | WaitFor | serial-lane | ordered_admit   # timely PCC on THIS a
  else:
    BaselineOCC(a)                                             # no Edge SM, no canary, no ForcePrefix
  # note: other a' in same t independently decide

resolve_tick (on early dirty / mid-flight known conflict on edge e of a):
  if value_stable: RebindThis(e)
  else if prefix_certifiable(t, k): CertifiedPrefixSkip / E1 residual reincarnate from k
  else if grain_identity_lost: B0 full reincarnation
  # do NOT: SoftWait Soft; ForcePrefix whole tx; SuffixRepair default; defer to validate-end only

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
                       Resolve R1a/R1b/E1/B0 rates, park_idle, force_prefix_count→0,
                       sticky_tx_wait→0, mixed_verb_intra_tx)
  pack_top hot access classes; quiet/flip decay
  emit falsifiers (SF/OCC, soft, await, pcc_fire_at_a, detect_ns, meta_ns,
                   park_idle, miss_reincarnation, force_prefix, flat_edgekey_hits)
```

Single live question: **for this access-event / typed edge, does learning predict essential conflict now?**  
- Yes → timely fine PCC Avoid/Resolve **on this grain**.  
- No → OCC proceed **on this grain**.  
- Miss → OCC residual reincarnation (cheap).

---

## 8. EVM / pevm map

| EVM / pevm | Fine-Grain Learned OCC–PCC Hybrid |
|------------|-----------------------------------|
| SLOAD / BALANCE / … | **Detect always** for this \(a\); PCC Avoid **iff** PredictedEssential\((\ell,k,\mathrm{morph})\) |
| SSTORE / publish | Detect publish; Bind consumers of **this** edge when predicted |
| CALL / frame enter-exit | \(\mathrm{depth}\) in \(a\); inner/outer verbs may differ |
| MvMemory publish | Bind when PredictedEssential ∧ Data ready **for this read**; else OCC read |
| Tx Ready / Executing / Done | Scheduler may still track tx containers; **π verbs attach to accesses/edges** |
| Validation fail | R1a / R1b / E1 / B0; no SuffixRepair default; no ForcePrefix π |
| Early dirty / ESTIMATE-class | Timely E1 / PrefixSkip when predicted; else OCC path |
| `EdgeKey` | **Must keep** \((\ell,\mathrm{reader},k,\mathrm{depth})\); flatten without \(k\)/depth **banned as SoT** |
| Journal / FF | R1a door on **this** origin; PrefixSkip when certifiable |
| OCC baseline runner | **Same code path** as Unfenced **per access** |
| `edge.rs` SM | Behind PredictedEssential **access**; not ROI silence; not ForcePrefix |
| `rem.rs` SuffixRepair | Remove from default lean path |
| `learner.rs` | First-class PredictedEssential\((\ell,k,\mathrm{morph})\) + access-class actuators; strip AEC/AdaptiveParams/tx-sticky Wait |
| `scheduler.rs` | Wave-first + Region-**access** serial lanes + ordered admission |
| `vm.rs::maybe_wait_specfence` | No-op when ¬PredictedEssential(**this \(a\)**); never tx SoftWait |
| `force_prefix` plant flag | **Not π**; debug/metrics only → drive to 0 |

---

## 9. Correctness

1. **Preset order** commit serialization.  
2. **Baseline OCC safety** unchanged when PredictedEssential≈∅ on accesses.  
3. **Fence soundness** only claimed on PredictedEssential **accesses/edges** — wrong prediction ⇒ performance only (abort/reincarnate), not wrong commit.  
4. **Hang-freedom:** serial-lane progress or Bind race or independent steal; no SoftWait Soft; no ForcePrefix deadlock theater.  
5. **Independence:** ¬PredictedEssential **access** never WaitFor-park.  
6. **No new speculation:** Unfenced baseline = OCC per access; Fence is barrier on grain, not guess.  
7. **Detect honesty:** missing Detect on an access is a protocol bug; false PredictedEssential is a performance bug.  
8. **Grain honesty:** mixed verbs inside one tx are allowed and expected; tx sticky Wait is a protocol bug.  
9. **Bans:** soft=0, await=0, no bn hardcodes, Unfenced ≰ OCC per access, Detect≠optional, Resolve≠validate-end-only on predicted essentials, ForcePrefix≠π, flat EdgeKey≠SoT.

---

## 10. Explicit principles (held + grain)

1. **Decision grain = access-event + typed edge** — never “tx \(t\) waits” as primary.  
2. **Detect ≠ optional** — always-on, cheap, at \(a\).  
3. **Avoid at access** — Bind/WaitFor/Unfenced for **this** boundary; siblings may differ.  
4. **Timely Resolve at grain** — RebindThis / CertifiedPrefixSkip; full reincarnation only if identity lost.  
5. **Learning at edge/frame** — PredictedEssential\((\ell,k,\mathrm{morph})\); first-wave per access class.  
6. **PCC overlay is per-Region-access**; OCC for accesses without predicted essential.  
7. **OCC reincarnation remains the cheap miss path** (residual after prefix when possible).  
8. **Timely Avoid/Resolve** — hybrid effect requires PCC **before** abort storms **on the right grain**.

---

## 11. Falsifiers from 99-block distribution (+ grain)

| Falsifier | Expect after v4.1 land | Today (post-v2) |
|-----------|------------------------|-----------------|
| soft / await | **0** | 0 |
| **tx SoftWait / sticky Wait-on-tx arms** | **0** | sticky patterns in history |
| **ForcePrefix-as-π / force_prefix_unfenced** | **0** | plant still carries flag |
| **flat EdgeKey SoT hits (no k/depth)** | **0** | risk if collapsed |
| writer_done / u_aa / hot_after_fence | **0** | 0 on digests |
| **¬PredictedEssential accesses: meta ≈ OCC** | META_COLD + quiet + cold accesses inside hot txs | canary/meta tax everywhere |
| median SF/OCC @8 | **≥ 0.7** then → **≥ 1.0** | **0.326** |
| fan_out median | **≥ 0.6** with PCC on **star accesses only** | 0.298 |
| worst N3 (19807137) | **≫ 0.08** via timely Bind/serial on star **accesses** + residual OCC | 0.076 |
| park_idle on fan_out | **≪ 0.25** via serial-lane not fleet WaitFor | 6196166 ≈1.19 |
| quiet cohort | **stay ≥1** | ~1.10 |
| Detect coverage | **100%** of storage/account/CALL boundaries | — |
| PCC fire timing | **before** abort-storm peak on predicted **access class** | late / sparse / sticky |
| **mixed verb intra-tx** on fan_out consumers | **>0** when morph has hot+cold accesses | often sticky Bind/canary mix without grain SoT |
| SuffixRepair default count | **≈ 0** | rewind dominant |
| PrefixSkip / RebindThis share of repairs | **rises**; full B0 only on identity loss | R2/full dominate |

Distribution must stay **fully classified** after land (every block in a mode).

---

## 12. Single-iteration land list (no P0/P1/P2)

One coherent cut — **all required together** (fine-grain hybrid):

1. **Unify Unfenced baseline with OCC path per access** — same read/validate/residual reincarnate; remove canary + Edge SM from ¬PredictedEssential **accesses** (`vm.rs`, `edge.rs` gate).  
2. **Access-event SoT** — plant decisions keyed by \(a=(t,\mathrm{inc},k,\mathrm{depth},\ell,\mathrm{mode})\) + typed edge; **ban** flat \((\ell,\mathrm{reader})\) control SoT.  
3. **Always-on cheap Detect at every access boundary** — L_record/L_access/L_edge features (`vm.rs` / plant).  
4. **First-class learning → PredictedEssential\((\ell,k,\mathrm{morph})\)** — intra first-wave **per access class** + InterBlockPrior; strip AEC/AdaptiveParams/tx-sticky Wait (`learner.rs`).  
5. **Timely PCC Avoid at this access** — Bind / WaitFor / Region-access serial-lane / ordered admission when PredictedEssential(**this \(a\)**); else OCC; allow mixed verbs in one tx (`edge.rs`, `scheduler.rs`, `vm.rs`).  
6. **Delete ForcePrefix-as-π and tx SoftWait** — counters may remain as falsifiers driven to 0.  
7. **Hot Region-access serial lane + ordered admission** — replace fleet WaitFor-park for predicted star/chain **access classes** (`scheduler.rs`).  
8. **Timely Resolve at grain** — RebindThis / CertifiedPrefixSkip / E1 residual; delete SuffixRepair-as-default (`pevm.rs`, `rem.rs`).  
9. **Miss path = OCC residual reincarnation** — full B0 only on grain identity loss.  
10. **Strip dead theater** — AEC/AdaptiveParams π, SoftWait Soft, Storm edge, PreferAdmit-as-primary, canary_reopen, CostGate-only-silence, sticky AvoidBroadcast.  
11. **Quiet / ¬PredictedEssential protection** — never seed PCC / sticky Wait on quiet priors; Detect still on.  
12. **Falsifier suite** — all-blocks SF/OCC + soft/await + detect coverage + pcc_fire_at_a timing + meta_ns + park_idle + force_prefix=0 + flat_edgekey=0 + mixed_verb_intra_tx + mode census.  
13. **Docs** — this SoT + `specfence-v4-txgrain-errata.md`; v4.0 hybrid kept as historical hybrid-identity parent; v2/v3 historical.

No staged P0/P1/P2. Partial land (PCC without OCC-identical baseline accesses, or Detect without timely Avoid-at-\(a\), or learning without edge/frame actuators, or Resolve still whole-tx default) is a **non-land**.

---

## 13. Worked example — opcode / access timeline (14689597 tx203 + star writer)

**Block:** `14689597` (597), morph=`fan_out`, wave≈434, star location \(\ell^\star=85335018835337005\), writers \(0..38\), ~448 program consumers.  
**Evidence:** `lab/results/effect-raw-deeper-b14689597.json` (tx203 first cross + edge); OCC@8 incarnation **4** on tx203 in occ8 sample; SF process still Bind/canary/cold mix on peers (`post-subgrain-per-tx-597-c8.json`).

### 13.1 Producer side (fan_out writer class — tx38)

Illustrative access timeline (effect-RAW):

| \(k\) (effect) | mode | \(\ell\) | v4.1 decision | Why |
|----------------|------|----------|---------------|-----|
| early account / cold slots | R/W | ≠\(\ell^\star\) | **Unfenced ≡ OCC** | ¬PredictedEssential access class |
| \(k{\approx}6\) | R | \(\ell^\star\) (reads prior writer 37) | **Bind** if Data ready / **WaitFor(37)** if predicted essential & unpublished | PredictedEssential\((\ell^\star,k{\approx}6,\mathrm{fan\_out})\) |
| later body SLOADs | R | private / cold | **Unfenced ≡ OCC** | same tx, different access — **must not** sticky-Wait |
| \(k{=}34\) | W | \(\ell^\star\) | Publish Data; Detect broadcast to **access class** consumers | first-wave: arm PredictedEssential for consumer template \(k{\approx}6\), **not** whole consumer txs |

**Anti-pattern (banned):** once tx38 hits \(\ell^\star\), ForcePrefix/sticky Wait on **all** subsequent reads in tx38 or in every consumer tx.

### 13.2 Consumer side — **tx203** (simple fan-out reader)

From effect-RAW (incarnation 0 discovery; OCC@8 reached inc 4):

| Field | Value |
|-------|-------|
| `first_program_cross_k` | **6** |
| `first_program_cross_location` | \(\ell^\star=85335018835337005\) |
| `first_program_producer_tx` | **38** |
| `total_db_effects` | **7** (3 writes) |
| `gas_used` / cross gas | 40284 / **37978** |
| `opcode_steps` at cross / total | 196 / 254 |
| `depth_frac_gross_work` | ≈ **0.94** |
| `depth_frac_opcode` | ≈ **0.77** |
| producer at discovery (serial journal) | validated / Data |

**Per-access hybrid decisions on tx203:**

| Tick | Access \(a\) | Detect | PredictedEssential? | Avoid / path | Resolve if dirty |
|------|--------------|--------|---------------------|--------------|------------------|
| \(k{=}0..5\) | cold / setup reads | record | no (unless class says) | **OCC Unfenced** | — |
| **\(k{=}6\)** SLOAD \(\ell^\star\) | \(a=(203,\mathrm{inc},6,\mathrm{depth},\ell^\star,\mathrm{R})\) | typed RAW edge ← writer 38 | **yes** (fan_out star class) | If Data: **Bind(38)**; if unpublished: **WaitFor(38)** or serial-lane — **this \(a\) only** | If value changes later: **RebindThis** if stable else **PrefixSkip from \(k{=}6\)** / E1 residual — **not** SuffixRepair whole tx |
| \(k{=}7..\) remaining effects | other \(\ell\) | record | typically **no** | **OCC Unfenced** (same tx!) | B0 only if identity lost |

**Scheduler ticks around the same window:**

1. Writers \(0..38\) publish \(\ell^\star\) → first-wave learning raises PredictedEssential for access class \((\ell^\star,k{\approx}6,\mathrm{program})\).  
2. Wave independents (non-star accesses / non-dependent txs) keep **OCC width** on P=8.  
3. tx203 at \(k{=}6\): PCC Bind/WaitFor **once**; does **not** park on SoftWait Soft; does **not** set ForcePrefix for \(k{>}6\).  
4. If OCC raced before publish: abort at validate → **PrefixSkip** resume at \(k{=}6\) with Bind, not full body redo when prefix certifiable.  
5. Neighbor quiet block 14689599: PredictedEssential≈∅ → all access ticks OCC; Detect still on for flip hygiene.

### 13.3 What tx-coarse hybrid would do wrong on tx203

| Tx-coarse move | Failure |
|----------------|---------|
| “tx203 waits” after first hot observe | Parks / serializes **all** remaining opcode-seconds; burns wall (WAIT_PARK class) |
| ForcePrefix=true on tx203 | Every later access pays Fence tax; invents essential where none predicted |
| EdgeKey \((\ell^\star,203)\) without \(k\) | Cannot tell \(k{=}6\) star read from a later cold read of unrelated slot; sticky Bind/canary theater |
| Whole-tx reincarnation on value change | Re-pays \(k{=}0..5\) useful work; loses CertifiedPrefixSkip |
| Per-tx AvoidBroadcast sticky | Arms Wait for access classes that should stay OCC |

### 13.4 Quiet / META_COLD / chain / park (compressed)

- **Quiet / META_COLD:** all access ticks ¬PredictedEssential → SF≡OCC; Detect cheap.  
- **Long chain (599-class):** PredictedEssential along RAW **access** depth; ordered admission on edges; off-chain accesses OCC.  
- **Park worst (6196166):** serial-lane on predicted **accesses**, never 8-way BlockingOther Soft/Wait.  
- **Prediction miss:** false positive = small PCC tax on **those accesses**; false negative = OCC residual reincarnation → up-weight that access class.

---

## 14. What v4.0 got right (absorb) vs what was still too coarse (replace)

| Absorb from v4.0 | Replace / push finer |
|------------------|----------------------|
| OCC cost-class default | Tx-sticky reading of Avoid |
| Always-on Detect + timely PCC overlay identity | Detect/Avoid keyed as “tx/Region set” without \(a\) |
| Soft/Await/AEC/Storm/canary bans | ForcePrefix-as-π still plant-shaped |
| Delete SuffixRepair default | Resolve still incarnation-ladder framed |
| Hot serial-lane > fleet WaitFor | Lane must be **access-class**, not whole-tx park |
| Quiet seed never plants H | First-wave broadcast must be **per access class** |
| Unfenced ≤ OCC | Must hold **per access**, including cold accesses inside hot txs |

---

## 15. Pause

**No coding in this task.** User confirms before any Fine-Grain Learned OCC–PCC Hybrid land. Implementation map only after confirm.

---

## Appendix A — Evidence pointers

- Corrected summary: `lab/results/arch-v2-all-blocks-sf-occ-sweep-corrected-summary.json`  
- Deep evidence: `lab/notes/specfence-post-v2-all-blocks-deep-evidence.md`  
- Effect-RAW (tx203 / star): `lab/results/effect-raw-deeper-b14689597.json`  
- Per-tx process: `lab/results/post-subgrain-per-tx-597-c8.json`  
- Parent hybrid identity (historical grain): `lab/notes/specfence-complete-architecture-v4-occ-pcc-hybrid.md`  
- Tx-grain errata: `lab/notes/specfence-v4-txgrain-errata.md`  
- Prior v3/v2 SoTs: historical  
- First-principles REM (aligned intent): `lab/notes/specfence-cc-architecture-v4-first-principles.md`
