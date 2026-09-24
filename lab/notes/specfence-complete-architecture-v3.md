# SpecFence complete architecture v3 — CostGate (standalone SoT)

**Date:** 2026-09-13 (Asia/Shanghai, UTC+8)  
**Status:** AUTHORITATIVE design SoT — **analysis + design only**; pause for user confirm before coding.  
**Branch / HEAD at write:** `cursor/specfence-complete-cc-63b0` @ `b63c75d`  
**Evidence base:** post-v2 all-blocks SF/OCC@8 across **99** ethereum snapshots (corrected n=98, median SF/OCC **0.326**); focus subgrain 597/599/097; tip process digests  
**Companion evidence:** `lab/notes/specfence-post-v2-all-blocks-deep-evidence.md`  
**Vocab (frozen):** **Spec = Region** (not “speculate”). **Fence** = Bind / WaitFor / serial-lane barriers on Regions. **Unfenced** = optimistic — in v3, **literally OCC-cost** when chosen. Product name SpecFence stays.

This document is a **brand-new complete** architecture. It is **not** a patch on v2. It replaces v2’s “Fence-first hybrid with R1-first Resolve” with **OCC-default / Fence-on-ROI (CostGate)**.

---

## Essence test (ONE paragraph)

**The one cost-class mistake SpecFence keeps making:** it treats Fence/Region machinery as default insurance and pays `protocol_meta + wait_idle + SuffixRepair_body` for conflict information that OCC already discovers via **cheap reincarnation** — so even after Avoid leaks are closed and R1 doors exist, SF wall stays ~3× OCC on the fan_out majority. **v3 eliminates it structurally** by making the Unfenced path **identical in cost class to OCC** (same MVCC reincarnation, no Edge/canary tax), and by admitting Fence **only** when a Region ROI gate proves strict makespan reduction; otherwise Fence machinery is not on the hot path at all.

---

## 0. Hard bans (non-negotiable)

| Ban | Why |
|-----|-----|
| SoftWait Soft storms | Wake≪reabort; soft=0 everywhere |
| EV Await doors / AdaptiveParams-as-θ | Makespan EV is a **gate feature**, not an Await verb |
| tip-identity Bind gate | Plant hygiene ≠ π |
| OCC-retry / Block-STM reincarnation as **control plane for contended Regions** | Discovery **is** the default path; not a fallback labeled “OCC mode flip” |
| Morph Storm/Quiet as edge actuator | Morphology feeds ROI prior only |
| 597 / bn hardcodes | Full-set median matches focus |
| Gate salad / OR-bool π | Signals ≠ Edge state |
| Dead AEC theater on access path | Delete |
| Celebrating abort↓ while ≪OCC | Wall/TPS vs OCC is the bar |
| Unfenced that is **more expensive than OCC** | **New ban** — canary/Edge tax on “optimistic” path is forbidden |
| Fence without ROI proof | **New ban** — Fence is optimization, not default |
| SoftWait / Await / tip-identity / OCC-retry-π / Storm / 597 hardcodes | Held |

---

## 1. Protocol identity

**Name:** CostGate SpecFence (architecture v3).

SpecFence is an **OCC-default parallel executor** with an optional **Region Fence overlay** admitted only under CostGate:

1. **Baseline path = OCC** — preset-order MVCC, validate, reincarnate. No Edge state machine tax, no canary probe class, no residual-Bind theater on this path.  
2. **Detect** still records conflicts (for learning + ROI), but Detect ≠ Fence.  
3. **CostGate** estimates, per Region, whether Fence reduces makespan vs baseline OCC.  
4. **Fence verbs** (Bind / WaitFor / serial-lane) apply **only** to Regions that pass CostGate.  
5. **Resolve on baseline** = OCC reincarnation (R4-shaped, cheap). **Resolve on Fenced Region** = R1 RebindOnly when value-stable; else reincarnation — **SuffixRepair is not a third cost class** unless it is proven cheaper than reincarnation for that tx (rare; default delete).  
6. **Learning** writes **Region ROI priors + hot serial sets** only if they change cost class; else delete.

Family: early-visible MVCC + optional piece barriers + work-conserving schedule + cost-class learning.

**Not:** Fence-first hybrid, SoftWait meta-CC, AEC Await, Storm morph π, “Unfenced canary” as a separate expensive path.

---

## 2. System model

### 2.1 Execution

- Block = ordered txs `0..n-1`. Correct commit order = preset order.  
- Workers = P cores (lab: **8**). Useful parallelism ≤ `min(P, independent ready width)`.  
- Each tx incarnation executes EVM; storage/account touches go through **path-selected** intercepts.

### 2.2 Objects

| Object | Meaning |
|--------|---------|
| **Location ℓ** | conflict object |
| **Access** | `(t, k, depth)` |
| **Edge** | typed conflict observation (Detect) — **not automatically a Fence** |
| **Region** | contended dependency unit (hot ℓ, RAW chain, multi-writer star) |
| **CostGate** | admits Fence for a Region iff predicted makespan↓ |
| **Fence** | Bind / WaitFor / serial-lane on admitted Regions only |
| **Baseline (Unfenced)** | OCC-cost optimistic path |

### 2.3 Cost model (law)

```
wall = useful_EVM + wait_idle + abort_recovery + protocol_meta

OCC_wall ≈ useful_EVM + reincarnation_recovery     # protocol_meta≈0, wait_idle≈0
SF_v2   ≈ useful_EVM + wait_idle + SuffixRepair + Edge/Bind/canary_meta
SF_v3   ≈ OCC_wall   + Σ_{Regions admitted} (Fence_tax − abort_savings)
```

**Invariant:** if no Region is admitted, `SF_wall ≡ OCC_wall` (within noise).  
**Admit law:** admit Region R only if `E[Fence_tax(R)] < E[abort_savings(R)]`.

### 2.4 Success metric

**Primary:** median SF/OCC TPS and wall @8 on all-blocks corrected set.  
**Bar today:** 0.326 / 3.05×.  
**v3 falsifier targets:** see §11. Quiet cohort ≥1 must not regress.

---

## 3. Conflict / Region model (Detect ≠ Fence)

Three Detect layers (unchanged grain honesty):

1. **L_record** — ℓ  
2. **L_access** — `(t, k, depth)`  
3. **L_edge** — typed edge + publish state  

**Region** aggregates hot ℓ, predicted writers, RAW chains — SoT for **whether to Fence**, not a pile of bools.

**Critical v3 split:**

| Stage | v2 | v3 |
|-------|----|----|
| Detect | feeds Edge SM → verb | feeds ROI + optional Fence set |
| Avoid broadcast | forces Fence | updates Region heat; Fence only if CostGate admits |
| Residual Bind | default on Done∅Data | only inside admitted Regions |
| Canary Unfenced | first-wave probe tax | **deleted** as a path; baseline OCC discovers |

---

## 4. Fence ontology (admitted Regions only)

Same verbs as v2, **scoped**:

```
if ℓ ∉ AdmittedFenceSet:
  → BaselineOCC          # literally OCC read/validate/reincarnate

else:  # admitted Region
  EdgeView →
    Published Data     → Bind(version)
    Unpublished essential anti-dep → WaitFor(w) | serial-lane(pred)
    else               → BaselineOCC (do not invent UnfencedCold tax)
```

**Laws:**

- Known essential **inside admitted Region** ⇒ Bind or WaitFor — never optimistic hang.  
- Hang-freedom = admit + steal **or** serial-lane progress — not SoftWait Soft.  
- Outside admitted set: **no** Bind residual theater, **no** canary, **no** Edge OR-salad.  
- Reasons are metrics-only.

### 4.1 Hot Region serialization vs cold parallel

Partition:

| Partition | Mechanism | Worker policy |
|-----------|-----------|---------------|
| **Hot Region** (admitted star/chain) | **Serial lane** for producer→consumer order | 1 logical lane; other workers **never WaitFor-park** on this Region |
| **Cold / independent** | Baseline OCC | Full P-way parallel |

This replaces “8 workers park on WaitFor while steal thrash” (6196166 / 597). PreferAdmit becomes unnecessary if the hot Region never multi-parks the fleet.

---

## 5. CostGate (scheduler-first ready graph)

### 5.1 Inputs (cheap features)

Per Region R (and block prior):

- `fan` / reader count / RAW depth (Detect + InterBlockPrior)  
- Historical `abort_density`, `reincarnation_ns`, `Fence_tax_ns` EMA  
- L1-shaped priors when available: wave_width, chain_length (offline falsifier; online approx from Detect)  
- `P` and current ready width

### 5.2 Decision

```
admit(R) ⇔ E[OCC_abort_cascade_ns(R)] − E[Fence_makespan_ns(R)] > margin
```

- **Default deny** (OCC baseline).  
- Admit only hot stars / long RAW chains where Wait/serial **strictly** beats abort storms.  
- Quiet / META_COLD / wide low-rewind blocks: **admit ∅** → SF≡OCC.

### 5.3 Scheduler-first ready graph

1. Build ready set from preset-order + completed deps (OCC scheduler).  
2. If admitted Region blocks a consumer, **enqueue on Region serial lane** (not park-all).  
3. Always fill P from independent ready txs first (**wave-first**).  
4. Steal = pull from ready/independent, never SoftWait Soft wake storms.

---

## 6. Resolve (cost-class aligned)

| Rank | Name | When | Cost class |
|-----:|------|------|------------|
| **B0** | Baseline reincarnation | Any abort on non-admitted path | **OCC-identical** |
| **R1** | RebindOnly | Admitted Region ∧ value-stable / FF match | Near-zero body |
| **B0′** | Reincarnation | Admitted Region ∧ value changed | **OCC-identical** — prefer over SuffixRepair |
| ~~R2~~ | ~~SuffixRepair~~ | **Deleted as default** | Only if lab proves < reincarnation for piece-repair; else remove |

**v2 lesson:** `identity_preserved` / `journal_ff` abundance did not yield R1 on `true_suffix` value changes — R2 stayed dominant (rewind:rebind 10–200×). Paying SuffixRepair **and** Fence meta is strictly worse than OCC reincarnation alone. v3 stops inventing a repair cost class that loses to OCC.

Incarnation carry (residual maps) may remain **inside admitted Regions** only; on baseline path, OCC reincarnation already rediscovers reads — do not add cold-Unfenced tax.

---

## 7. Learning (live only if cost class changes; else delete)

### 7.1 Keep / reshape

| Signal | Sink | Cost-class effect |
|--------|------|-------------------|
| abort_density / fan / RAW depth per ℓ | Region ROI prior | Changes admit(R) |
| Fence_tax_ns vs reincarnation_ns EMA | CostGate margin | Changes admit(R) |
| pack_top hot ℓ | AdmittedFenceSet seed | Only if admit fires |
| quiet prior | admit∅ bias | Protects quiet cohort |

### 7.2 Explicitly delete (do not “wire harder”)

| Item | Fate |
|------|------|
| Canary probe / canary_reopen as path | **Delete** — OCC discovers |
| AEC choose_resolve / αβγδ Await | **Delete** |
| SoftWait meta / engagement Storm π | **Delete** |
| r1_first_bias chasing true_suffix value changes | **Delete** — use B0 reincarnation |
| SuffixRepair-as-default ladder | **Delete** |
| PreferAdmit heat as primary park fix | **Delete** if serial-lane lands; else temporary only |
| Morph heuristic as Fence actuator | **Delete**; ROI prior only |
| Learning that only moves counters not wall class | **Delete** |

### 7.3 Inter-block

Warm-start **ROI priors + hot serial sets** with flip/quiet decay. Never plant admitted Fence sets that flip quiet→fan_out without abort evidence.

---

## 8. End-to-end control loop

```
begin_block:
  seed ROI priors + candidate hot Regions from InterBlockPrior (quiet → admit∅)
  AdmittedFenceSet := { R | CostGate(R) }
  # if AdmittedFenceSet empty → pure OCC path (SF≡OCC)

per access (t, ℓ, k, depth):
  Detect: record edge features for ROI (always cheap)
  if ℓ ∈ AdmittedFenceSet:
    verb := Bind | WaitFor | serial-lane   # Fence ontology
  else:
    BaselineOCC read                     # no Edge SM, no canary

per validation abort:
  if value_stable ∧ admitted: R1 RebindOnly
  else: reincarnate (OCC-identical)      # no SuffixRepair default

scheduler tick:
  fill P from independent ready (wave-first)
  progress admitted Region serial lanes without fleet-wide park

end_block:
  update ROI EMA (Fence_tax vs reincarnation, abort_density)
  pack_top only for Regions that were admitted or should have been
  emit falsifiers (SF/OCC, soft, await, admit_count, meta_ns, park_idle)
```

Single live question: **does this Region pay for itself?** If no, OCC.

---

## 9. EVM / pevm map

| EVM / pevm | CostGate SpecFence |
|------------|--------------------|
| SLOAD / BALANCE / … | Detect always; Fence intercept **iff** admitted |
| MvMemory publish | Bind only in admitted Regions; else OCC read |
| Tx Ready / Executing / Done | Serial lane for admitted; OCC scheduler otherwise |
| Validation fail | R1 or reincarnation; no SuffixRepair default |
| Call depth / k | EdgeKey grain for Detect/ROI |
| Journal / FF | R1 door inside admitted only |
| OCC baseline runner | **Same code path** as Unfenced baseline — not a mode flip theater |
| `edge.rs` SM | Gated behind AdmittedFenceSet |
| `rem.rs` SuffixRepair | Remove from default lean path |
| `learner.rs` | ROI EMA only; strip AEC/AdaptiveParams π |
| `scheduler.rs` | Wave-first + Region serial lanes |
| `vm.rs::maybe_wait_specfence` | No-op outside admitted set |

---

## 10. Correctness

1. **Preset order** commit serialization.  
2. **Baseline OCC safety** unchanged when admit∅.  
3. **Fence soundness** only claimed inside AdmittedFenceSet.  
4. **Hang-freedom:** serial-lane progress or Bind race; no SoftWait Soft.  
5. **Independence:** non-admitted ℓ never WaitFor-park.  
6. **No new speculation:** Unfenced baseline = OCC; Fence is barrier, not guess.  
7. **Bans:** soft=0, await=0, no bn hardcodes, Unfenced ≰ OCC cost class.

---

## 11. Falsifiers from 99-block distribution

| Falsifier | Expect after v3 land | Today (post-v2) |
|-----------|----------------------|-----------------|
| soft / await | **0** | 0 |
| writer_done / u_aa / hot_after_fence | **0** | 0 on digests |
| **admit∅ blocks: SF/OCC ≈ 1** (noise) | META_COLD + quiet + low-rewind | META_COLD still 0.19–0.35 |
| median SF/OCC @8 | **≥ 0.7** then → **≥ 1.0** | **0.326** |
| fan_out median | **≥ 0.6** | 0.298 |
| worst N3 (19807137) | **≫ 0.08** (toward OCC by denying futile Fence or serializing hot only) | 0.076 |
| park_idle on fan_out | **≪ 0.25** without SoftWait | 6196166 ≈1.19 |
| quiet cohort | **stay ≥1** | ~1.10 |
| protocol_meta_ns on admit∅ | **≈ OCC** | Edge/canary tax everywhere |
| SuffixRepair default count | **≈ 0** on lean path | rewind dominant |
| Fail-mode mass | R2+WAIT_PARK+META shrink; QUIET grows | R2=36, WAIT=14, META=14 |

Distribution must stay **fully classified** after land (every block in a mode).

---

## 12. Single-iteration land list (no P0/P1/P2)

One coherent cut — **all required together**:

1. **Unify Unfenced baseline with OCC path** — same read/validate/reincarnate; remove canary path + Edge SM from non-admitted ℓ (`vm.rs`, `edge.rs` gate).  
2. **Implement CostGate + AdmittedFenceSet** — default deny; ROI features from Detect + InterBlockPrior (`learner.rs` reshape).  
3. **Hot Region serial lane** — replace fleet WaitFor-park for admitted stars/chains (`scheduler.rs`).  
4. **Delete SuffixRepair-as-default** — reincarnation unless R1 value-stable (`pevm.rs`, `rem.rs`).  
5. **Strip dead theater** — AEC/AdaptiveParams π, SoftWait, Storm edge, PreferAdmit-as-primary, canary_reopen path.  
6. **Quiet / admit∅ protection** — never seed admitted sets on quiet priors.  
7. **Falsifier suite** — all-blocks SF/OCC + soft/await + meta_ns + park_idle + mode census.  
8. **Docs** — this SoT + evidence; retire v2 as historical.

No staged P0/P1/P2. Partial land (Fence gate without OCC-identical baseline, or baseline without deleting SuffixRepair) is a **non-land**.

---

## 13. Worked examples

### 13.1 Quiet / META_COLD (`15199017`, `14029313`)

- Detect sees light aborts, huge canary/cold Unfenced today.  
- CostGate: **admit∅**.  
- Path: pure OCC ⇒ SF/OCC → ~1.  
- Learning: record that Fence would not have paid.

### 13.2 Fan_out star (`14689597` / 19807137-class)

- L1 wave ≫ P; one hot star.  
- CostGate: admit **star Region only**.  
- Serial lane for star readers; wave independents OCC-parallel on other workers.  
- Aborts on star with value change → reincarnation (not R2×cold).  
- Expect: cut park_idle and meta; approach OCC with small Fence tax on star only.

### 13.3 Long chain (`19606599`, `19469097`)

- CostGate may admit chain Region if RAW depth × abort density predicts savings.  
- Serial lane along chain; off-chain OCC.  
- If ROI negative (common today at SF/OCC~0.29): admit∅ and match OCC.

### 13.4 Park worst (`6196166`)

- Today: WaitFor parks burn > wall.  
- v3: either admit∅ (OCC) or serial lane without multi-park — **never** 8-way BlockingOther.

---

## 14. What v2 got right (absorb) vs wrong (replace)

| Absorb | Replace |
|--------|---------|
| Spec=Region vocab; Soft/Await bans | Fence-first default |
| Avoid leak closures (writer_done, u_aa) | Canary / Edge tax on “Unfenced” |
| Detect fine grain | SuffixRepair-as-default Resolve |
| Quiet seed never plants H | R1-first chasing true_suffix value changes |
| Structural collapse without bn hardcodes | PreferAdmit as park antidote |
| All-blocks falsifier discipline | Learning that doesn’t change cost class |

---

## 15. Pause

**No coding in this task.** User confirms before any CostGate land. Implementation map to be written only after confirm.

---

## Appendix A — Evidence pointers

- Corrected summary: `lab/results/arch-v2-all-blocks-sf-occ-sweep-corrected-summary.json`  
- Deep evidence: `lab/notes/specfence-post-v2-all-blocks-deep-evidence.md`  
- Prior v2 SoT (historical): `lab/notes/specfence-complete-architecture-v2.md`
