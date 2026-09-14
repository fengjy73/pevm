# SpecFence complete architecture v9.3 — pevm-unified spine (AUTHORITATIVE correction SoT)

**Date:** 2026-09-14 (Asia/Shanghai, UTC+8)  
**Status:** **AUTHORITATIVE design SoT for plant ownership / spine unity** — design only; **DO NOT implement Rust yet**  
**Tip at write:** `bb67ff7`  
**USER CORRECTION (absorbed):** 「我的意思是specfence应该和pevm集成在一起吧，不然你肯定会比OCC慢啊」  
→ SpecFence must be **integrated into pevm itself**, not a bolted-on second computer beside OCC. Dual path (empty-PE → OCC computer / PE-on → SpecFence computer) is **inherently slower** than bare OCC. OCC is the **default Spec cost class inside one pevm spine**, not a rival mode you retreat into.

**Absorbed:**  
- `lab/notes/specfence-complete-architecture-v9.1-cc-pc-bayes.md` — **bars + call-flow** (still authoritative for those)  
- `lab/notes/specfence-complete-architecture-v9.2-module-structure.md` — **layer names** `pc/` `cc/` `bayes/` `fuse/` (kept); **ownership re-homed into pevm spine**  
- `lab/notes/specfence-v9-whole-plant-callflow-audit.md` — hybrid OCC↔SF called a top smell (§3 #3, live `specfence_plant_is_occ`)  
- Live tip: `pevm.rs` worker `specfence_plant_is_occ` → `next_occ_task` vs `next_sf_task` bifurcation; `vm` gate early-Ok; validate fork OCC stage vs `validate_specfence`

**Supersedes:**  
- v9.1 §§ that treat “quiet_occ_mode / empty∧cold → byte OCC **computer**” or “known-star keep **SF computer**” as dual rival schedulers (§2.1 quiet branch, §5.3 `quiet_fence_off ⇒ next_occ_task whole computer`, §6 `computer.rs` as alternate next_sf-only)  
- v9.2 §§ that leave `pevm` as a thin **seam** calling either OCC or SF computers (`§2.1` “only `pc::computer::next_*`”, `§6` pevm owns hybrid as folklore)  
- Land-brief framing that cut **9** = “Quiet OCC identity” via `pevm` **hybrid**  

**Does not supersede:** v9.1 product bars (§1), call order Bayes→admit→decide→Fence→Validate/Repair, Soft=0, WaitFor/Resolve redesign (M1–M5), morph recipes, falsifiers. Module **names** still v9.2; they are **owned by the pevm spine**, not a parallel crate computer.

```
v9.1 owns:  BARS + WHAT (Bayes→admit→decide→Fence→Validate) + Soft=0
v9.2 owns:  LAYER NAMES (pc/ cc/ bayes/ fuse/ research/) + DELETE/MERGE inventory
v9.3 owns:  WHERE THE SPINE LIVES — one pevm parallel executor; SpecFence = pevm's CC⊗PC⊗Bayes plant
            BAN dual OCC/SF computers as product architecture
```

---

## 0. Essence (ONE paragraph)

**SpecFence v9.3** is **pevm's concurrency plant**, not a sibling executor bolted beside Block-STM OCC. There is **one** parallel executor spine — `pevm.rs` worker loop + `scheduler.rs` ready/steal + `vm.rs` execute host + `mv_memory` — for every `ConcurrencyMode` path that matters. SpecFence is that spine running **PC ⊗ CC ⊗ Bayes**: Bayes seeds structure, PC admits/schedules Stages (ProducerStage, PinHold, Validate, Repair), CC decides Mode(a) and Fence verbs, Validate/Repair consume certs. **Quiet/cold** stays on the **same** spine with Mode(a)=**Spec** everywhere, Bayes.cold, zero meta overhead — wall **cost-class ≡ OCC**, not a retreat into `next_occ_task` / `validate_occ_stage` / gate-bypass as a second computer. **Banned as product architecture:** hybrid `specfence_plant_is_occ` → full OCC scheduler/validate fork when PE empty or `quiet_fence_off`. Dual-computer always loses wall to bare OCC (meta tax + schedule bifurcation + validate fork + mode re-check per task). v9.2's `pc/` `cc/` `bayes/` `fuse/` modules **fold into** pevm ownership (integration, not parallel crate). Bars and call order remain v9.1; Soft=0 forever.

---

## 0.1 USER CORRECTION — why dual-computer is structurally slower

Live tip (`bb67ff7`) does this every worker iteration:

```
task := if Occ ∨ specfence_plant_is_occ(empty PE ∨ quiet_fence_off)
           then next_occ_task(scheduler)     # Computer A: Block-STM indices only
           else next_sf_task(...)            # Computer B: wave + ready + ProducerStage
execute/validate similarly forked
```

That is **two computers** sharing a process. Even when Computer B is “off,” SpecFence still pays:

| Tax | Why bare OCC wins |
|-----|-------------------|
| **Meta** | Learner quiet_fence_off / has_any_predicted / plant_is_occ checks; SpecFenceCtx tables allocated; hotset/prior/bayes observe ticks even on cold |
| **Schedule bifurcation** | Branch + alternate `next_*` symbol; SF path cannot share OCC's minimal ready walk; switching mid-block (quiet lifts after abort) invalidates scheduler assumptions |
| **Validate fork** | `validate_occ_stage` vs `validate_specfence` vs rem path — different abort/estimate/cert protocols; cannot amortize one validate kernel |
| **Retreat framing** | Product treats OCC as the safe mode you fall back into → SF never becomes the cheap default Spec class; cold path cannot prove ≡ OCC cost because it is a different codepath |

**Consequence:** any bolted-on SpecFence that “becomes OCC when quiet” will **always** lose wall clock to a process that only ever ran OCC. Integration is not aesthetics — it is the only way quiet/cold can be cost-class ≡ OCC while hot RAW uses the same spine with Fence/admit.

---

## 0.2 Hard bans (v9.3 — spine unity)

All v9.1 / v9.2 bans **held**, plus:

| Ban | Hold |
|-----|------|
| Hybrid `plant_is_occ` → full OCC scheduler / validate / execute retreat as **product architecture** | **NEW (authoritative)** |
| Dual rival computers: `next_occ_task` vs `next_sf_task` as alternate plant spines for SpecFence mode | **NEW** |
| Shipping “quiet ≡ OCC” by **switching computers** instead of Mode(a)=Spec + Bayes.cold + zero meta on **one** spine | **NEW** |
| SpecFence as a parallel crate / second executor beside pevm Block-STM | **NEW** |
| Land that preserves bifurcation “temporarily for quiet” while claiming v9.1 bars | **NEW** — dual path is a falsifier of cost-class ≡ OCC |
| Treating `ConcurrencyMode::Occ` as the SpecFence cold path (Occ remains a **separate mode** for non-SpecFence callers; SpecFence cold ≠ flip to Occ computer) | **NEW** |

**Allowed (not dual-computer):**  
- `ConcurrencyMode::Occ` as a **distinct product mode** (no SpecFence tables, no Bayes) — pure Block-STM.  
- Inside `ConcurrencyMode::SpecFence`: **one** scheduler entry, **one** validate entry, **one** execute host; cold = Spec cost class (no Fence meta).  
- Internal fast paths that are **inlinable no-ops** on the same spine (e.g. `if Bayes.cold { /* Mode(a)=Spec; skip admit tables */ }`) — must compile to near-OCC work, not call a second `next_occ_task` plant.

---

## 1. One pevm spine (product law)

### 1.1 Ownership

```
┌──────────────────────────────────────────────────────────────────────────┐
│ pevm parallel executor spine (ONE)                                       │
│                                                                          │
│  pevm.rs        begin_block / worker loop / try_execute / end_block      │
│  scheduler.rs   ready / steal / indices / admit law (PC Stages live here)│
│  vm.rs          EVM host; calls CC decide + Fence act (no rival plant)   │
│  mv_memory.rs   STM + tip identity / snap for Validate                   │
│                                                                          │
│  crates/pevm/src/specfence/{pc,cc,bayes,fuse,research}/                  │
│       = CC⊗PC⊗Bayes **modules owned by this spine**                      │
│       ≠ second computer; ≠ alternate next_task plant                     │
└──────────────────────────────────────────────────────────────────────────┘
```

**SpecFence = pevm's CC⊗PC⊗Bayes plant.** Module dirs from v9.2 remain the filesystem law for policy/state; **schedule authority, worker loop, and validate stage dispatch live in pevm/scheduler/vm**, calling into those modules — never the reverse “specfence computer owns next_task, pevm only switches.”

### 1.2 ConcurrencyMode paths that matter

| Mode | Spine | SpecFence plant? |
|------|-------|------------------|
| `Occ` | pevm Block-STM only | **No** — no SpecFenceCtx, no Bayes, no ready_edge. Pure OCC computer (legitimate **separate mode**). |
| `SpecFence` | **Same** pevm worker + scheduler + vm | **Yes** — PC⊗CC⊗Bayes always on; cold = Spec cost class |
| Other / rem research | out of product SoT or behind `research/` | not dual SpecFence↔OCC hybrid |

**Inside SpecFence mode there is no `plant_is_occ` retreat.** Cold is a **Bayes/Mode(a) cost class**, not a mode flip.

### 1.3 Quiet / cold = Spec cost class ≡ OCC (same spine)

```
begin_block (SpecFence mode always):
  Bayes.seed(...)
  if empty PE ∧ no ReadyEdge ∧ Bayes.cold:
      cost_class := Spec          # Mode(a)=Spec everywhere
      meta := 0                   # no Fence, no admit refuse, no cert, no PinHold
      # SAME scheduler.next / steal law; ReadyEdge table empty ⇒ admit all
      # SAME validate entry; RS_fence noop when no strips
  else:
      cost_class := Fenced        # admit / decide / Fence / R1 live
```

**Equivalence claim:** cold SpecFence wall ≈ bare `Occ` mode wall on quiet blocks — proven by **same schedule/validate symbols** with meta elided, not by calling `next_occ_task`.  
**Falsifier:** cold path still branches to `next_occ_task` / `validate_occ_stage` / `specfence_access_is_occ` early-Ok that skips a different protocol.

---

## 2. Call-flow (kept from v9.1 — re-homed on one spine)

Order **unchanged**: **Bayes → PC.admit → Execute(only if admitted) → CC.decide ← Bayes → Fence → Validate/Repair**.

```
pevm begin_block
  Bayes.seed → pc.admit_seed (ReadyEdges + ProducerStage)
  # NO: if cold then arm_occ_computer()

pevm worker (ONE loop for SpecFence mode)
  task := scheduler.next_task_unified(...)   # PC steal law always
           # cold: degenerates to Block-STM index walk (empty ready extras)
           # hot:  ProducerStage ∪ edge-satisfied Execute ∪ Validate ∪ PinHold ∪ Repair

  Task::Execution → vm.execute
    CC.decide ← Bayes   # cold: Mode(a)=Spec unconditionally (zero query cost)
    Fence acts only if PE-on ∧ ¬cold

  Task::Validation → validate_unified
    RS_spec bool; RS_fence tip/snap when strips; else OCC-bool subset on SAME entry
    # NO: if plant_is_occ then validate_occ_stage else validate_specfence
```

PinHold, ScheduleRefuse, AbortingThrow last, R1 live, Soft=0 — **all v9.1 held**.

---

## 3. Why dual-computer always loses wall to OCC (explicit)

1. **Two schedule implementations** cannot both be as thin as one; the SF computer carries ReadyEdge/ProducerStage/wave even when unused, or pays a branch to avoid them — bare OCC pays neither.  
2. **Per-task re-evaluation** of `plant_is_occ` (tip does this on every schedule and every execute) is pure meta; integrated cold is a block-level cost_class latch, not a per-access mode oracle.  
3. **Validate protocol fork** doubles abort/estimate/cert reasoning; integrated validate is one function with optional Fence RS.  
4. **Mid-block mode flip** (quiet_fence_off lifts after first abort heat) forces workers across the bifurcation — scheduler/wave/ready state was never warmed on the OCC side → first RAW wave already lost (M4), then SF computer starts cold. Integrated spine never “starts over” as a different computer.  
5. **Product psychology:** treating OCC as retreat prevents SpecFence cold from being engineered to ≡ OCC; the dual path becomes a permanent crutch and a permanent tax.

**Therefore:** dual OCC/SF computers are **banned** as architecture; quiet performance is an **integration** problem, not a hybrid-switch problem.

---

## 4. How v9.2 modules fold **into** pevm spine (integration map)

Module **names and DELETE/MERGE inventory** stay v9.2. **Ownership** changes: layers are pevm-owned policy/state, not a rival `computer.rs` plant.

| v9.2 layer / module | Folds into (ownership) | Duty on unified spine |
|---------------------|------------------------|------------------------|
| `pc/computer.rs` | **`scheduler.rs` + thin helpers** | Steal / ready law / PinHold / ProducerStage priority — **one** `next_task` entry for SpecFence mode; delete product use of `next_occ_task` vs `next_sf_task` bifurcation |
| `pc/ready_edge.rs`, `producer_stage.rs`, `lane.rs`, `wave.rs` | Called from **scheduler / pevm begin_block** | Admit seed + refuse; not a second scheduler |
| `cc/decide.rs`, `fence_act.rs`, `access_*`, `certificate`, `validate`, `repair` | Called from **`vm.rs` / pevm validate arm** | Single decide; single validate entry; Fence acts leave `vm` body |
| `bayes/*` | **`pevm` begin_block / end_block** + ports to admit/decide/validate | Query ports; quiet_cold ⇒ cost_class Spec; **delete** Boolean π; **delete** `quiet_fence_off` as computer switch |
| `fuse/*` | pevm metrics / process | Honesty; falsify dual-computer regression |
| `research/*` | gated | Unchanged quarantine |
| `executor.rs` `specfence_plant_is_occ` / `next_occ_task` / `validate_occ_stage` as SpecFence cold | **DELETE from SpecFence product path** | `Occ` **mode** may keep pure OCC helpers; SpecFence must not call them as retreat |

### 4.1 Seam rewrite (vs v9.2 §2.1)

| Seam | v9.2 (superseded for unity) | **v9.3** |
|------|-----------------------------|----------|
| `pevm.rs` | begin_block seed; worker picks `next_occ` **or** `next_sf` | begin_block seed + **cost_class**; worker **always** unified next/validate for SpecFence mode |
| `scheduler.rs` | PC steal; hybrid still possible via pevm | **Sole** schedule authority for SpecFence; empty extras ≡ OCC walk |
| `vm.rs` | call cc::decide; gate must not `plant_is_occ` early-Ok as computer | Same; cold = Mode(a)=Spec, not skip-to-OCC-protocol |
| `mv_memory.rs` | tip snap | unchanged |

### 4.2 Structural land order amendment

v9.2 S0–S2 (museum DELETE → MERGE → layer dirs) **still required**, but **prepend**:

| Phase | Work | Done when |
|------:|------|-----------|
| **S−1** | **Unify pevm spine** — design+land plan: one next_task, one validate entry, one execute host for SpecFence mode; ban `plant_is_occ` retreat; define cold Spec cost class | Bifurcation absent from product architecture SoT; Occ mode remains separate pure path |
| **S0** | Museum DELETE / dual π quarantine | (v9.2) |
| **S1** | MERGE kernel→certificate; fence_act out of vm | (v9.2) |
| **S2** | Create pc/cc/bayes/fuse dirs **as pevm-owned modules** | (v9.2 names; v9.3 ownership) |
| **S3** | v9.1 call-order cuts on the **unified** spine | Bars path |

**Ban:** S0–S2 museum cleanup that **preserves** `next_occ_task`/`next_sf_task` hybrid as the quiet story — that re-encodes dual-computer into the layered tree.

---

## 5. Bars + Soft (unchanged from v9.1)

Product bars **B1–B12** and falsifiers from v9.1 §1 — **held in full**. Soft=**0** forever.  
Additional falsifier under v9.3:

- SpecFence mode still branches to a full OCC scheduler/validate **computer** for quiet/cold (dual-computer regression).  
- Cold SpecFence wall systematically worse than `ConcurrencyMode::Occ` on quiet cohort for **meta/bifurcation** reasons (not RAW).

---

## 6. Relation to prior SoTs

| Doc | Still authoritative for | Superseded by v9.3 |
|-----|-------------------------|--------------------|
| v9.1 | Bars, call order, WaitFor/Resolve, morphs, Soft=0 | Dual-computer / quiet→OCC computer / SF computer rivalry wording |
| v9.2 | Layer names, DELETE/MERGE/MOVE inventory, S0–S2 museum order | pevm-as-thin-seam; computer.rs as alternate next_sf plant; hybrid left as folklore |
| Call-flow audit | Evidence of hybrid smell @ tip | — (smell ratified as ban) |
| Land brief | Cuts 0–9 | §0 gains **S−1**; cut 9 = Spec cost class on unified spine, not hybrid OCC identity |

---

## 7. Implementation posture

- **Design only; DO NOT implement Rust under this note until authorized.**  
- Land must include **S−1 spine unity** before or as opening of structural work — museum folders without unity = dual-computer with prettier paths.  
- Partial land that keeps `specfence_plant_is_occ` → `next_occ_task` as the quiet product path = **non-land**.  
- `ConcurrencyMode::Occ` pure path may remain for non-SpecFence callers; it is **not** SpecFence's cold path.

---

## 8. Success checklist (additions to v9.1 §9)

1–11 from v9.1 §9 **held**.  
12. SpecFence mode: **one** schedule entry, **one** validate entry, **one** execute host — no `plant_is_occ` full OCC retreat.  
13. Quiet/cold: Mode(a)=Spec, Bayes.cold, zero Fence meta; wall cost-class approaches bare Occ mode **without** switching computers.  
14. v9.2 modules exist as **pevm-owned** layers; `computer.rs` is not a rival plant.  
15. Dual-computer bifurcation absent from hot path and from architecture SoT.

---

## 9. Essence restated

OCC is not SpecFence's rival mode or safe retreat — it is the **default Spec cost class** of a **single pevm parallel executor** when Bayes is cold and Fence meta is zero. SpecFence is pevm running PC⊗CC⊗Bayes on that spine when RAW structure appears. Dual `next_occ_task` / `next_sf_task` computers, `specfence_plant_is_occ` validate/execute forks, and quiet-as-mode-switch are **banned**: they guarantee wall loss to bare OCC via meta + bifurcation + validate fork. **v9.3 supersedes v9.1/v9.2 unified-spine / hybrid wording; bars stay v9.1; module names stay v9.2 under pevm ownership.** Soft=0.
