# SpecFence complete architecture v4 — Learned OCC–PCC Hybrid (standalone SoT)

> **SUPERSEDED (grain SoT):** v4.0 OCC–PCC hybrid *identity* (OCC default + learned PCC) is **kept** in `lab/notes/specfence-complete-architecture-v4-frozen-grain.md`. Tx-coarse decision grain is replaced by frozen π; do **not** implement Avoid/Resolve from this file.


> **Grain supersession (2026-09-13, updated):** For Detect / Avoid / Resolve / learning **decision grain**, read **`lab/notes/specfence-complete-architecture-v4-frozen-grain.md` (v4.1-frozen)** as authoritative. Pre-freeze finegrain sketch is also superseded. This v4.0 note remains the parent statement of **OCC–PCC hybrid identity** (OCC default + learned PCC overlay). Tx-coarse reading of this file is **not** SoT — see `lab/notes/specfence-v4-txgrain-errata.md`.


**Date:** 2026-09-13 (Asia/Shanghai, UTC+8)  
**Status:** **SUPERSEDED** (v4.0 grain) — hybrid *identity* parent only; AUTHORITATIVE grain SoT is `specfence-complete-architecture-v4-frozen-grain.md`.  
**Branch / HEAD at write:** `cursor/specfence-complete-cc-63b0` @ `40903a3`  
**Evidence base:** post-v2 all-blocks SF/OCC@8 across **99** ethereum snapshots (corrected n=98, median SF/OCC **0.326**); focus subgrain 597/599/097; tip process digests; v3 CostGate critique  
**Companion evidence:** `lab/notes/specfence-post-v2-all-blocks-deep-evidence.md`  
**Errata (v3 → v4):** `lab/notes/specfence-v3-to-v4-errata.md`  
**Vocab (frozen):** **Spec = Region** (not “speculate”). **Fence** = Bind / WaitFor / serial-lane / ordered-admission barriers on Regions. **Unfenced** = optimistic — in v4, **literally OCC-cost** when no predicted essential anti-dep. Product name SpecFence stays.

This document is a **brand-new complete** architecture. It is **not** a patch on v3. It replaces CostGate-as-face (**OCC-default / Fence-on-ROI-only**) with **Learned OCC–PCC Hybrid SpecFence**: OCC cost class as the default execute path, plus a **always-on fine Detect** and a **learning-driven, timely PCC overlay** (Avoid + Resolve actuators) that runs continuously — achieving hybrid effect without Fence-first tax and without sparse ROI-only Fence.

---

## Essence test (ONE paragraph)

**Learned OCC–PCC Hybrid SpecFence** keeps the **OCC cost class as default** (execute → validate → cheap reincarnation; no Edge/canary/SuffixRepair tax on the hot path when no essential conflict is predicted), but **never treats Detect as optional and never waits for end-of-tx validate storms to “discover” known structure**. On that OCC baseline it continuously runs **fine-grained, learning-driven Detect / Avoid / timely Resolve**: Detect always records access/edge grain; learning predicts essential anti-deps and fires **PCC verbs** (WaitFor / Bind-version-install / Region serial-lane / ordered admission) **in time** — before abort cascades — while prediction misses fall back to **OCC reincarnation** (still the cheap failure path). The effect is an **OCC + PCC hybrid**: optimistic width wherever learning says independence; fine Region Fence wherever learning says essential contention — without v2’s Fence-everywhere meta and without v3’s sparse “Fence only if ROI proves” that arrived too late / too rarely.

**Hybrid one-liner:** *OCC by default; learned fine PCC Always-Detect + timely Avoid/Resolve on predicted essential Regions; reincarnate cheap when prediction misses.*

---

## 0. Hard bans (non-negotiable)

| Ban | Why |
|-----|-----|
| SoftWait Soft storms | Wake≪reabort; soft=0 everywhere |
| EV Await doors / AdaptiveParams-as-θ | Makespan EV is a **feature for learning**, not an Await verb |
| tip-identity Bind gate | Plant hygiene ≠ π |
| OCC-retry / Block-STM reincarnation as **control plane for contended Regions** | Reincarnation is the **miss path**, not the policy that replaces PCC on known essentials |
| Morph Storm/Quiet as edge actuator | Morphology feeds learning prior only |
| 597 / bn hardcodes | Full-set median matches focus |
| Gate salad / OR-bool π | Signals ≠ Edge state / learned actuators |
| Dead AEC theater on access path | Delete |
| Celebrating abort↓ while ≪OCC | Wall/TPS vs OCC is the bar |
| Unfenced that is **more expensive than OCC** | **Held from v3** — canary/Edge tax on “optimistic” path is forbidden |
| Fence-first / residual Bind theater on cold ℓ | v2 failure mode — held deleted |
| **Sparse Detect / Detect-as-optional** | **New ban (v4)** — Detect always on; learning without continuous Detect is theater |
| **Late-only Resolve (validate-end storms as first Avoid)** | **New ban (v4)** — Avoid/Resolve must be **timely** when prediction says essential |
| SuffixRepair-as-default Resolve cost class | Held deleted; prefer R1 or OCC reincarnation |
| Unfenced-more-expensive-than-OCC | Alias of Unfenced≤OCC cost class — explicit |

---

## 1. Protocol identity

**Name:** Learned OCC–PCC Hybrid SpecFence (architecture v4).

SpecFence is an **OCC-cost-class parallel executor** with a **first-class learning PCC overlay** on Regions:

1. **Baseline path = OCC** — preset-order MVCC, validate, reincarnate. No Edge SM tax, no canary probe class, no residual-Bind theater when no essential anti-dep is predicted.  
2. **Detect always** — fine grain `(ℓ, t, k, depth)` + typed edges; Detect ≠ Fence, but Detect is **never skipped**. Feeds learning and timely actuators.  
3. **Learning (first-class)** — trains Detect features → Avoid/Resolve actuators: predicted essential anti-deps, hot serial sets, version-install readiness, ordered-admission heat, quiet decay. Not “ROI admit or silence.”  
4. **Avoid = fine PCC verbs when predicted essential** — WaitFor(w) / Bind(version) / Region serial-lane / ordered admission — **timely**, at access / scheduler tick, not after abort storms.  
5. **Else OCC proceed** — no predicted essential ⇒ Unfenced ≡ OCC read.  
6. **Timely Resolve** — mid-flight / early abort / RebindOnly when value-stable; do **not** wait for end-of-tx validate-only as the first conflict response on predicted essentials. Miss path = OCC reincarnation (cheap).  
7. **PCC is fine-grained Region Fence** — Spec=Region; Fence barriers scoped to predicted-essential Regions / edges, not tx-wide locks and not whole-block Fence-first.

Family: early-visible MVCC + **continuous fine Detect** + **learned timely PCC Avoid/Resolve** + work-conserving schedule + OCC-cheap miss path.

**Not:** Fence-first hybrid (v2), CostGate ROI-only Fence (v3), SoftWait meta-CC, AEC Await, Storm morph π, “Unfenced canary” expensive path, Detect-optional learning theater.

### 1.1 How v4 differs from v3 CostGate and from v2 Fence-first

| Axis | v2 Fence-first | v3 CostGate | **v4 Learned OCC–PCC Hybrid** |
|------|----------------|-------------|-------------------------------|
| Default path | Fence/Edge/canary on hot path | OCC baseline | **OCC baseline** (same cost class) |
| Detect | Feeds Edge SM → verb | Feeds ROI; Fence iff admit | **Always on**; feeds learning + actuators |
| Fence / PCC | Default insurance | **Sparse** — only if ROI proves | **Continuous overlay** — fire when learning predicts essential |
| Avoid timing | Often late / park-heavy | Often **never** (deny) | **Timely** PCC before abort storms |
| Resolve | R1-first + SuffixRepair dominant | Reincarnation default; R1 rare | **Timely** R1 / early cut + **OCC reincarnation on miss** |
| Learning role | Structure weights; cost class stuck | ROI admit only | **First-class** trains Detect/Avoid/Resolve |
| Failure vs OCC | Meta + R2 + park ≫ OCC | Admit∅ ≈ OCC, but **no hybrid win** on fan_out | Hybrid: PCC where predicted; OCC elsewhere |
| Critique | Too expensive | **不够** — ROI Fence too sparse / late | Target: hybrid effect |

**User critique absorbed:** default OCC is correct; what was missing is **fine-grained + learning-driven conflict Detect / Avoid / timely Resolve** on that baseline so the system behaves like **OCC+PCC mixed**, not “OCC always” or “Fence when ROI.”

---

## 2. System model

### 2.1 Execution

- Block = ordered txs `0..n-1`. Correct commit order = preset order.  
- Workers = P cores (lab: **8**). Useful parallelism ≤ `min(P, independent ready width)`.  
- Each tx incarnation executes EVM; storage/account touches go through **path-selected** intercepts that **always Detect**, then either **PCC Avoid verb** or **OCC proceed**.

### 2.2 Objects

| Object | Meaning |
|--------|---------|
| **Location ℓ** | conflict object |
| **Access** | `(t, k, depth)` |
| **Edge** | typed conflict observation (Detect) — may become Fence under learning |
| **Region (Spec)** | contended dependency unit (hot ℓ, RAW chain, multi-writer star) |
| **PredictedEssential** | learning posterior: this edge/Region will abort if Unfenced |
| **Fence (PCC verb)** | Bind / WaitFor / serial-lane / ordered-admission on predicted-essential Regions |
| **Baseline (Unfenced)** | OCC-cost optimistic path when ¬PredictedEssential |
| **Miss path** | OCC reincarnation when prediction wrong |

### 2.3 Cost model (law)

```
wall = useful_EVM + wait_idle + abort_recovery + protocol_meta

OCC_wall ≈ useful_EVM + reincarnation_recovery     # protocol_meta≈0, wait_idle≈0

SF_v2   ≈ useful_EVM + wait_idle + SuffixRepair + Edge/Bind/canary_meta
SF_v3   ≈ OCC_wall   + Σ_{ROI-admitted} (Fence_tax − abort_savings)
          # problem: admit sparse/late ⇒ abort_savings unrealized; hybrid effect missing

SF_v4   ≈ useful_EVM
          + Σ_{predicted essential} (timely_PCC_tax)          # Avoid before storm
          + Σ_{prediction miss} (OCC_reincarnation)           # cheap miss
          + Detect_meta_cheap                                 # always-on, must stay ≪ OCC gap
```

**Invariants:**

1. If learning predicts **no** essential Regions, `SF_wall ≡ OCC_wall` (within noise) — Detect meta must be noise-class.  
2. If learning predicts essentials **correctly and timely**, PCC tax ≪ avoided abort cascades.  
3. If learning **misses**, Resolve = OCC reincarnation — **never** invent a costlier repair class.  
4. **Ban:** Unfenced path cost class > OCC.

### 2.4 Success metric

**Primary:** median SF/OCC TPS and wall @8 on all-blocks corrected set.  
**Bar today:** 0.326 / 3.05×.  
**v4 falsifier targets:** see §11. Quiet cohort ≥1 must not regress.  
**Hybrid signature:** fan_out blocks show **timely PCC on star/chain** + **OCC-width on wave independents**; META_COLD / quiet show **SF≡OCC**.

---

## 3. Fine Detect (always on, cheap)

Three Detect layers (grain honesty unchanged):

1. **L_record** — ℓ  
2. **L_access** — `(t, k, depth)`  
3. **L_edge** — typed edge + publish state  

**Laws:**

- **Detect ≠ optional.** Every storage/account touch emits Observe/Publish features for learning.  
- **Detect ≠ Fence.** Recording an edge does not install WaitFor/Bind until Avoid fires.  
- **Detect must be cheap.** Feature write + EMA update; no SoftWait Soft, no canary probe class, no full Edge SM on ¬PredictedEssential.  
- Detect feeds: PredictedEssential posterior, hot serial sets, version-install readiness, ordered-admission heat, quiet/flip decay.

**Critical v4 split vs v3:**

| Stage | v3 CostGate | v4 Hybrid |
|-------|-------------|-----------|
| Detect | for ROI; Fence only if admit | **always**; continuous learning features |
| Avoid | only inside AdmittedFenceSet | **when PredictedEssential** — timely PCC |
| Residual Bind | only if admitted | only if PredictedEssential ∧ Done∅Data pattern |
| Canary Unfenced | deleted | **deleted** — Detect + OCC discover; no canary tax |
| “Admit∅ ⇒ silence” | yes | **no** — Detect still runs; Avoid stays off until prediction |

---

## 4. Avoid (PCC verbs when predicted essential; else OCC proceed)

### 4.1 Decision

```
on access (t, ℓ, k, depth):
  Detect.record(...)                          # always
  if PredictedEssential(ℓ, t, k, depth):
    verb := PCC_Avoid:
      Published Data     → Bind(version)           # version-install
      Unpublished anti-dep → WaitFor(w) | serial-lane(pred) | ordered_admit
  else:
    BaselineOCC read                           # literally OCC; no Edge tax
```

**PCC Avoid verbs (fine Region Fence):**

| Verb | When | Effect |
|------|------|--------|
| **Bind(version)** | Producer Data published / value-install ready | Read certified version; avoid stale reincarnation |
| **WaitFor(w)** | Essential anti-dep; producer not ready; single waiter preferred | Barrier until w publishes |
| **Region serial-lane** | Hot star/chain; avoid fleet-wide park | One logical lane; other workers on independents |
| **Ordered admission** | Ready-set must respect predicted RAW order | Admit consumers after producers without SoftWait Soft |

**Laws:**

- Known / **predicted** essential ⇒ Bind or WaitFor / serial-lane — never optimistic hang on that edge.  
- ¬PredictedEssential ⇒ **OCC proceed** — never invent UnfencedCold / canary tax.  
- Hang-freedom = serial-lane progress or Bind race or steal from independents — **not** SoftWait Soft.  
- Prefer **serial-lane + ordered admission** over multi-worker WaitFor park (6196166 lesson).

### 4.2 Hot Region serialization vs cold parallel

| Partition | Mechanism | Worker policy |
|-----------|-----------|---------------|
| **Predicted-essential Region** | PCC Fence (serial lane / Bind / ordered admit) | 1 logical lane on Region; other workers **never WaitFor-park fleet** |
| **¬PredictedEssential / independent** | Baseline OCC | Full P-way parallel |

This is the **hybrid schedule face**: PCC on the learned critical Regions; OCC width on the rest.

---

## 5. Timely Resolve (don’t wait for end-of-tx validate-only)

| Rank | Name | When | Cost class |
|-----:|------|------|------------|
| **A0** | **Timely Avoid cut** | Mid-access Detect says essential now | Avoid before body waste |
| **R1** | RebindOnly | PredictedEssential ∧ value-stable / FF match | Near-zero body |
| **E1** | Early abort + reincarnate | PredictedEssential ∧ value will change / dirty known early | OCC-identical body restart — **sooner** than validate-end |
| **B0** | Baseline reincarnation | Prediction miss / non-essential abort | **OCC-identical** |
| ~~R2~~ | ~~SuffixRepair~~ | **Deleted as default** | Only if lab proves < reincarnation; else remove |

**v2/v3 lessons absorbed:**

- v2: `identity_preserved` / `journal_ff` abundance did not yield R1 on `true_suffix` value changes — SuffixRepair dominated and lost to OCC.  
- v3: reincarnation-default was correct for cost class, but **waiting until validate** on predicted essentials wasted the PCC side of the hybrid.  
- v4: **timely** means Avoid/early-cut/R1 fire when learning + Detect say so; miss still B0.

Incarnation carry / residual maps may remain **inside PredictedEssential Regions** only; on baseline path, OCC reincarnation rediscovers — do not add cold-Unfenced tax.

---

## 6. Learning (first-class — trains Detect / Avoid / Resolve actuators)

Learning is **not** an optional ROI gate. It is the control plane that makes the PCC overlay fire **in time** and stay **off** when OCC is enough.

### 6.1 What trains what

| Signal / feature | Trains | Actuator effect |
|------------------|--------|-----------------|
| abort_density / fan / RAW depth per ℓ | Detect posterior | PredictedEssential(ℓ,…) |
| first-wave Observe→abort latency | Avoid timing | Fire WaitFor/Bind **earlier** next access / next block |
| Fence_tax_ns vs reincarnation_ns EMA | Avoid aggressiveness | Soften/strengthen PCC; never SoftWait Soft |
| pack_top hot ℓ / star cover | Region serial-lane set | Ordered admission + serial lane |
| writer_done / Done∅Data patterns | Avoid Bind residual | Only under PredictedEssential |
| value_stable / true_suffix fail rates | Resolve rank | R1 vs E1 vs B0 |
| park_idle / BlockingOther | Schedule Avoid | Prefer serial-lane over fleet WaitFor |
| quiet / flip priors | Inter-block decay | Protect quiet; do not sticky-plant H into quiet neighbors |

### 6.2 First-wave + inter-block

**First-wave (intra-block):**

1. Warm Detect from InterBlockPrior (hot ℓ, RAW templates, quiet bias).  
2. As early writers publish, Detect updates PredictedEssential **online** — Avoid may engage mid-block without waiting for full abort storms.  
3. Resolve actuators update EMA on each abort (R1 hit rate, early-cut savings).

**Inter-block:**

- Warm-start PredictedEssential priors + hot serial sets with flip/quiet decay.  
- Never plant PCC Fence sets that flip quiet→fan_out without abort evidence.  
- Priors are performance-only; commit path remains seq≡par TCB.

### 6.3 Explicitly delete (do not “wire harder”)

| Item | Fate |
|------|------|
| Canary probe / canary_reopen as path | **Delete** — Detect + OCC discover |
| AEC choose_resolve / αβγδ Await | **Delete** |
| SoftWait meta / engagement Storm π | **Delete** |
| r1_first_bias chasing true_suffix value changes | **Delete** — use E1/B0 reincarnation |
| SuffixRepair-as-default ladder | **Delete** |
| PreferAdmit heat as primary park fix | **Delete** if serial-lane lands |
| Morph heuristic as Fence actuator | **Delete**; prior only |
| CostGate **default-deny silence** as the only Fence story | **Replace** — continuous learned PCC overlay |
| Learning that only moves counters not wall class | **Delete** |

---

## 7. End-to-end control loop (schedule)

```
begin_block:
  seed Detect priors + PredictedEssential candidates from InterBlockPrior
  (quiet → PredictedEssential≈∅ bias; hot star/chain → warm PCC readiness)
  # empty prediction ⇒ pure OCC cost class (Detect still on, Avoid off)

per access (t, ℓ, k, depth):
  Detect: record edge features (ALWAYS, cheap)
  update PredictedEssential online (first-wave learning)
  if PredictedEssential(...):
    Avoid := Bind | WaitFor | serial-lane | ordered_admit   # timely PCC
  else:
    BaselineOCC read                                       # no Edge SM, no canary

per early dirty / mid-flight known conflict:
  timely Resolve: R1 if value_stable else E1 early reincarnate
  # do NOT defer first response to validate-end on predicted essentials

per validation abort (miss or residual):
  R1 if value_stable ∧ was PredictedEssential
  else: B0 reincarnate (OCC-identical)

scheduler tick:
  fill P from independent ready (wave-first)               # OCC width
  progress PredictedEssential Region serial lanes          # PCC without fleet park
  ordered admission for predicted RAW consumers

end_block:
  update learning EMA (PredictedEssential, Avoid tax vs abort savings,
                       Resolve R1/E1/B0 rates, park_idle)
  pack_top hot Regions; quiet/flip decay
  emit falsifiers (SF/OCC, soft, await, pcc_fire_count, detect_ns,
                   meta_ns, park_idle, miss_reincarnation)
```

Single live question: **for this edge/Region, does learning predict essential conflict now?**  
- Yes → timely fine PCC Avoid/Resolve.  
- No → OCC proceed.  
- Miss → OCC reincarnation (cheap).

---

## 8. EVM / pevm map

| EVM / pevm | Learned OCC–PCC Hybrid |
|------------|------------------------|
| SLOAD / BALANCE / … | **Detect always**; PCC Avoid intercept **iff** PredictedEssential |
| MvMemory publish | Bind when PredictedEssential ∧ Data ready; else OCC read |
| Tx Ready / Executing / Done | Serial lane / ordered admit for predicted Regions; OCC scheduler otherwise |
| Validation fail | R1 / E1 / B0; no SuffixRepair default |
| Early dirty / ESTIMATE-class | Timely E1 when predicted; else OCC path |
| Call depth / k | EdgeKey grain for Detect + learning |
| Journal / FF | R1 door inside PredictedEssential only |
| OCC baseline runner | **Same code path** as Unfenced baseline |
| `edge.rs` SM | Behind PredictedEssential (not ROI-only admit∅ silence) |
| `rem.rs` SuffixRepair | Remove from default lean path |
| `learner.rs` | **First-class** PredictedEssential + Avoid/Resolve actuators; strip AEC/AdaptiveParams π |
| `scheduler.rs` | Wave-first + Region serial lanes + ordered admission |
| `vm.rs::maybe_wait_specfence` | No-op when ¬PredictedEssential |

---

## 9. Correctness

1. **Preset order** commit serialization.  
2. **Baseline OCC safety** unchanged when PredictedEssential≈∅.  
3. **Fence soundness** only claimed on PredictedEssential Regions/edges — wrong prediction ⇒ performance only (abort/reincarnate), not wrong commit.  
4. **Hang-freedom:** serial-lane progress or Bind race or independent steal; no SoftWait Soft.  
5. **Independence:** ¬PredictedEssential ℓ never WaitFor-park.  
6. **No new speculation:** Unfenced baseline = OCC; Fence is barrier, not guess.  
7. **Detect honesty:** missing Detect is a protocol bug; false PredictedEssential is a performance bug.  
8. **Bans:** soft=0, await=0, no bn hardcodes, Unfenced ≰ OCC cost class, Detect≠optional, Resolve≠validate-end-only on predicted essentials.

---

## 10. Explicit principles (held)

1. **Detect ≠ optional** — always-on, cheap, fine grain.  
2. **Learning is first-class** — trains Detect posteriors and Avoid/Resolve actuators; not a side metrics bus.  
3. **PCC is fine-grained Region Fence** — Spec=Region; verbs Bind / WaitFor / serial-lane / ordered admission.  
4. **OCC reincarnation remains the cheap failure path** when prediction misses.  
5. **OCC cost class default** when ¬PredictedEssential.  
6. **Timely Avoid/Resolve** — hybrid effect requires PCC **before** abort storms, not ROI-sparse Fence after.

---

## 11. Falsifiers from 99-block distribution

| Falsifier | Expect after v4 land | Today (post-v2) |
|-----------|----------------------|-----------------|
| soft / await | **0** | 0 |
| writer_done / u_aa / hot_after_fence | **0** | 0 on digests |
| **¬PredictedEssential blocks: SF/OCC ≈ 1** | META_COLD + quiet + low-rewind | META_COLD still 0.19–0.35 (canary/meta tax) |
| median SF/OCC @8 | **≥ 0.7** then → **≥ 1.0** | **0.326** |
| fan_out median | **≥ 0.6** with **timely PCC on star** | 0.298 |
| worst N3 (19807137) | **≫ 0.08** via timely Bind/serial on star + OCC reincarnate on value change (no R2×meta) | 0.076 |
| park_idle on fan_out | **≪ 0.25** via serial-lane not fleet WaitFor | 6196166 ≈1.19 |
| quiet cohort | **stay ≥1** | ~1.10 |
| protocol_meta_ns on ¬PredictedEssential | **≈ OCC** (+ Detect noise only) | Edge/canary tax everywhere |
| Detect coverage | **100%** of storage/account touches | — |
| PCC fire on predicted essentials | **before** abort-storm peak (first-wave) | late / sparse (v3 design) |
| SuffixRepair default count | **≈ 0** on lean path | rewind dominant |
| Fail-mode mass | R2+WAIT_PARK+META shrink; QUIET grows; **HYBRID_PCC** appears on fan_out | R2=36, WAIT=14, META=14 |

Distribution must stay **fully classified** after land (every block in a mode).

---

## 12. Single-iteration land list (no P0/P1/P2)

One coherent cut — **all required together**:

1. **Unify Unfenced baseline with OCC path** — same read/validate/reincarnate; remove canary path + Edge SM from ¬PredictedEssential ℓ (`vm.rs`, `edge.rs` gate).  
2. **Always-on cheap Detect** — every access records L_record/L_access/L_edge features for learning (`vm.rs` / plant).  
3. **First-class learning → PredictedEssential** — intra first-wave + InterBlockPrior; trains Avoid/Resolve actuators (`learner.rs` reshape; strip AEC/AdaptiveParams π).  
4. **Timely PCC Avoid overlay** — Bind / WaitFor / Region serial-lane / ordered admission when PredictedEssential; else OCC proceed (`edge.rs`, `scheduler.rs`).  
5. **Hot Region serial lane + ordered admission** — replace fleet WaitFor-park for predicted stars/chains (`scheduler.rs`).  
6. **Timely Resolve** — mid-flight / early E1 + R1 value-stable; delete SuffixRepair-as-default (`pevm.rs`, `rem.rs`).  
7. **Miss path = OCC reincarnation** — prediction miss never invents costlier repair.  
8. **Strip dead theater** — AEC/AdaptiveParams π, SoftWait, Storm edge, PreferAdmit-as-primary, canary_reopen path, CostGate-only-silence story.  
9. **Quiet / ¬PredictedEssential protection** — never seed PCC Fence on quiet priors; Detect still on.  
10. **Falsifier suite** — all-blocks SF/OCC + soft/await + detect coverage + pcc_fire timing + meta_ns + park_idle + mode census.  
11. **Docs** — this SoT + v3→v4 errata + evidence; retire CostGate-as-face; v2/v3 historical.

No staged P0/P1/P2. Partial land (PCC overlay without OCC-identical baseline, or Detect without timely Avoid, or learning without actuators) is a **non-land**.

---

## 13. Worked examples

### 13.1 Quiet / META_COLD (`15199017`, `14029313`)

- Detect sees light aborts; today canary/cold Unfenced tax.  
- Learning: PredictedEssential≈∅.  
- Avoid: off. Path: pure OCC ⇒ SF/OCC → ~1.  
- Detect still records (cheap) so next fan_out neighbor can warm correctly.  
- Learning: Fence would not have paid; quiet prior reinforced.

### 13.2 Fan_out star (`14689597` / 19807137-class)

- L1 wave ≫ P; one hot star.  
- First-wave Detect: early writers publish → PredictedEssential(star) rises **before** abort storm peak.  
- Avoid: **timely** Bind / serial-lane for star readers; wave independents OCC-parallel.  
- Resolve: value change → E1/B0 reincarnation (not R2×cold meta).  
- Expect: cut park_idle and meta; hybrid — PCC on star, OCC on wave.

### 13.3 Long chain (`19606599`, `19469097`)

- Learning may PredictedEssential along RAW depth if abort density predicts cascade.  
- Avoid: serial lane / ordered admission along chain; off-chain OCC.  
- If prediction weak: OCC proceed + B0 — still ≡ OCC cost class (v3 admit∅ outcome), but Detect stays ready for mid-block promotion.

### 13.4 Park worst (`6196166`)

- Today: WaitFor parks burn > wall.  
- v4: PredictedEssential ⇒ **serial lane** without multi-park; ¬PredictedEssential ⇒ OCC.  
- **Never** 8-way BlockingOther Soft/Wait theater.

### 13.5 Prediction miss (any block)

- PredictedEssential false positive: paid small PCC tax → still correct; learning down-weights.  
- False negative: OCC abort → B0 reincarnation (cheap miss path) → learning up-weights PredictedEssential for next access/block.  
- Hybrid stays safe: miss never invents SuffixRepair/canary cost class.

---

## 14. What v2 / v3 got right (absorb) vs wrong (replace)

| Absorb | Replace |
|--------|---------|
| Spec=Region vocab; Soft/Await bans | v2 Fence-first default |
| Avoid leak closures (writer_done, u_aa) | v2 canary / Edge tax on “Unfenced” |
| Detect fine grain | v2 SuffixRepair-as-default Resolve |
| Quiet seed never plants H | v2 R1-first chasing true_suffix value changes |
| Structural collapse without bn hardcodes | v2 PreferAdmit as park antidote |
| All-blocks falsifier discipline | Learning that doesn’t change cost class |
| v3 OCC cost-class baseline | **v3 ROI-only / sparse Fence (不够)** |
| v3 ban Unfenced > OCC | **v3 Detect→silence when admit∅** |
| v3 delete SuffixRepair default | **v3 late/absent timely PCC Avoid** |
| v3 hot Region serial-lane idea | CostGate-as-face product identity |

---

## 15. Pause

**No coding in this task.** User confirms before any Learned OCC–PCC Hybrid land. Implementation map to be written only after confirm.

---

## Appendix A — Evidence pointers

- Corrected summary: `lab/results/arch-v2-all-blocks-sf-occ-sweep-corrected-summary.json`  
- Deep evidence: `lab/notes/specfence-post-v2-all-blocks-deep-evidence.md`  
- Prior v3 SoT (historical / CostGate): `lab/notes/specfence-complete-architecture-v3.md`  
- Prior v2 SoT (historical / Fence-first): `lab/notes/specfence-complete-architecture-v2.md`  
- v3→v4 errata: `lab/notes/specfence-v3-to-v4-errata.md`
