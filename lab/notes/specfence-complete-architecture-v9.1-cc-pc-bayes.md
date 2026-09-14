# SpecFence complete architecture v9.1 — PC⊗CC⊗Bayes call-flow rewrite (AUTHORITATIVE SoT)

**Date:** 2026-09-14 (Asia/Shanghai, UTC+8)  
**Status:** **AUTHORITATIVE design SoT — design only; DO NOT implement Rust yet**  
**Tip at write:** `bb67ff7`  
**Supersedes:** `lab/notes/specfence-complete-architecture-v9-cc-pc-bayes.md` (triple-peer frame **kept**; honesty bars raised; **live call order rewritten**; delete/merge/rewire made explicit)  
**Whole-plant audit:** `lab/notes/specfence-v9-whole-plant-callflow-audit.md`  
**Land brief:** `lab/notes/specfence-v9-land-brief.md`  
**Absorbed:** WaitFor Aborting (M1); Spec-sibling covers_all (M2); incarnation-strict R1a (M3); quiet first wave (M4); cert wipe (M5); BIND_AFTER_PRODUCER_DONE 442/473; tip Soft=0 honesty median **0.728** / fan **0.362**.

> **Module / land layout:** superseded for **filesystem tree + DELETE/MERGE/MOVE structural order** by  
> `lab/notes/specfence-complete-architecture-v9.2-module-structure.md`.  
> **Unified pevm spine / dual-computer ban:** superseded by  
> `lab/notes/specfence-complete-architecture-v9.3-pevm-unified.md` (**AUTHORITATIVE correction**).  
> Any v9.1 wording that treats quiet/cold as a retreat into an OCC **computer** (`next_occ_task` /  
> `plant_is_occ` hybrid / rival SF computer) is **void**; OCC is the default **Spec cost class** on  
> **one** pevm spine. **Bars (§1), call order Bayes→admit→decide→Fence→Validate, Soft=0, WaitFor/Resolve,  
> morphs, falsifiers — still AUTHORITATIVE here.**  
> Structure audit: `lab/notes/specfence-v9.1-code-structure-audit.md`.

> **Structure law (v9.4):** file-SRP supersedes v9.2 folder-layer SoC — `lab/notes/specfence-complete-architecture-v9.4-file-srp.md`.  
> Bars + call-flow here **kept**; spine unity v9.3 **kept**. Triple = lens, not folder goal. **No land now.**



**π fields KEPT:** Spec=Region; Mode(a) verbs; Soft=**0**; exclude set; Spec=Region meaning.  
**Honesty now (tip):** nonempty median **0.728**; **14689597 N=3 ≈0.362**; WaitFor↑∧abort≈OCC; R1≈0; star Bind-after-Done dominant. **No celebration.**

---

## 0. Essence (ONE paragraph)

**SpecFence v9.1** is a **co-equal triple-frame** plant whose **call order is the product**: **Bayes seeds shared structure → PC admits (ReadyEdges + ProducerStages) → CC decides Mode(a) only on admitted Executes → Fence is ScheduleRefuse or PinWithoutThrow (AbortingThrow last) → Validate/Repair consumes certs with tip identity/snap and must convert R1 when fenced RAW covers**. PC owns Stages, ready/steal/pipeline, ProducerStage, PinHold, and `wall = useful_EVM + idle + repair + meta`. CC owns Detect/Avoid/Resolve, Mode(a), Fence, certs, R1 — co-owning ready membership. Bayes owns posteriors over RAW edges, PE(ℓ,k,morph), EV[Fence shapes vs B0], producer-liveness, P(covers_all) and **must be queried** at admit/decide/validate — never an OR-bool museum. Empty PE ∧ no ReadyEdge ∧ Bayes.cold ⇒ byte-identical OCC. Soft=0 forever. **Product bars:** nonempty median **≥0.95** (stretch ≥0.98), quiet p10 **≥0.90**, mixed median **≥0.92**, **14689597 ≥0.90 @8 N≥3** (stretch ≥0.95–1.0), R1 win rate **≥50%** on cert-bearing fan_out fails, useful_EVM fraction gated, Soft=0 — WaitFor↑∧abort≈OCC, Bind-after-Done dominant, always-B0 despite strips, or Bayes-unused are failures. v9 named the peers; **v9.1 rewires who calls whom** so the plant cannot “land” WaitFor volume without abort cut and R1.

---

## 0.1 Why v9 text was not enough (plant still v8-shaped)

```
v9 design said:   PC ⊗ CC ⊗ Bayes peers; schedule-first; PinWithoutThrow; R1 live; Bayes ports
tip plant does:   quiet_fence_off → OCC computer first wave
                  mid-tx decide OR-bool → WaitFor Aborting | Bind-after-Done
                  validate → covers_all∧incarnation-strict → almost always B0
                  Bayes BetaMap observe-only; dual π museums (edge/resolve/bayes.decide)

v9.1:             RAISE BARS + REWRITE CALL ORDER + DELETE/MERGE DUAL π
                  Bayes → PC.admit → Execute(only if admitted) → CC.decide(Bayes) →
                  Fence(ScheduleRefuse|PinHold) → Validate/Repair(R1 live)
```

---

## 0.2 Hard bans (v9 held + v9.1)

All v9 bans (§0.3 of v9) **held**, plus:

| Ban | Hold |
|-----|------|
| Shipping median≥0.744 as “success” while fan_out≪0.9 | **NEW** |
| Land that raises WaitFor/Bind counts without abort↓ **and** R1 win rate | **NEW** |
| Keeping `choose_edge_action` / AEC `choose_action` / `bayes.should_wait_hard` as live SpecFence π | **NEW** |
| `quiet_fence_off` forcing OCC computer when InterPrior/HotSet/Bayes mark hot RAW | **NEW** (quiet lone-abort protection **kept** for cold quiet) |
| Patch salad (WaitFor-only / Bind-only / R1-only) without admit-order rewrite | **NEW** |

---

## 1. Raised success bars (PRODUCT — all required)

| # | Metric | Bar |
|---|--------|-----|
| B1 | Nonempty all-blocks median SF/OCC TPS @8 Soft=0 | **≥ 0.95** (stretch **≥ 0.98**) |
| B2 | Quiet + quiet_ish median | **≥ 0.98** |
| B3 | Quiet p10 | **≥ 0.90** |
| B4 | Mixed cohort median | **≥ 0.92** |
| B5 | **14689597** SF/OCC @8 **N≥3** | **≥ 0.90** stable; stretch **≥ 0.95–1.0** |
| B6 | **19807137** @8 N≥3 | **≥ 0.70** then ratchet; abort_SF ≤ abort_OCC |
| B7 | Soft / exclude / await Soft storms | **0** |
| B8 | R1 win rate on fan_out when PE+certs present | **≥ 50%** of cert-bearing validate fails → R1a∨R1b |
| B9 | Star Bind-after-Done share (14689597 consumers) | **< 10%** |
| B10 | WaitFor↑ ∧ abort≈OCC as steady state | **forbidden** |
| B11 | useful_EVM / (P × wall) | quiet **≥ 0.70**; fan_out @8 **≥ 0.55** |
| B12 | Abort SF/OCC median (nonempty) | **≤ 1.0**; fan_out **≤ 0.9** |

**Falsifiers (stop / revert class):** Soft>0; median claim without JSON; Bind↑∧abort↓; WaitFor↑∧abort≈OCC; Bind-after-Done ≥10% on star; R1 win rate token (>0 but ≪50%); template PE `[1,6,10,20]` on fan_out; Aborting WaitFor default on high depth_frac; Bayes posterior unused at ports; PC-primary / CC-only / Bayes-museum partial land; celebrating B1 while B5 fails.

*Legacy v8/v9 bars (median>0.744 / fan≥0.85 / quiet p10≥0.85) are **explicitly retired** as success criteria — too small for product intent.*

---

## 2. Authoritative call-flow (PC ⊗ CC ⊗ Bayes)

### 2.1 Diagram — who calls whom

```
┌─────────────────────────────────────────────────────────────────────────────┐
│ begin_block                                                                 │
│   Bayes.seed(InterPrior, HotSet, WŜ, morph, effect-raw offline priors)      │
│        │                                                                    │
│        ▼                                                                    │
│   PC.admit_seed  ←── Bayes.P_RAW · EV_admit                                 │
│        insert ReadyEdge(t←w) for known/predicted consumers                  │
│        ProducerStage.reserve(w) for every edge producer                     │
│        if empty∧cold → quiet_occ_mode (byte OCC)                            │
│        else → SF computer ON even if intra PE empty (known stars)           │
└───────────────────────────────────┬─────────────────────────────────────────┘
                                    │
┌───────────────────────────────────▼─────────────────────────────────────────┐
│ PC.schedule (steal law)                                                     │
│   ready = ProducerStage(w)                                                  │
│         ∪ Execute(t) where ∀edges Done∨lane ∧ ¬schedule_hold                │
│         ∪ Validate(Executed)                                                │
│         ∪ Repair(grain)                                                     │
│         ∪ PinHold(t)          # mid-tx park WITHOUT Aborting                │
│   steal: useful_EVM → Validate pipeline → ProducerStage → PinHold wake      │
│          → Repair → lane; NEVER steal PE-blocked Execute; NEVER default     │
│          steal PinHold into FullRetry                                       │
└───────────────────────────────────┬─────────────────────────────────────────┘
                                    │
          ┌─────────────────────────┴─────────────────────────┐
          ▼                                                   ▼
┌──────────────────────────┐                    ┌─────────────────────────────┐
│ Execute(t)  [PC]         │                    │ Validate(t)  [PC←CC⊗Bayes]  │
│ only if admitted         │                    │ RS_spec: OCC bool           │
│ for access a:            │                    │ RS_fence: tip_id / snap     │
│   k := ordinal.note PE-on│                    │ fail → Repair(grain)        │
│   vis := access_vis      │                    │   R1a/R1b/selective/B0      │
│   q := Bayes.queries(...)│                    │ Bayes.update(outcome)       │
│   verb := CC.decide(q)   │                    └─────────────────────────────┘
│   act:                   │
│     ScheduleRefuse — bug │  (should not be running)
│     PinWithoutThrow      │──► PinHold Stage (keep rem/PC/call stack)
│     SerialLane exclusive │
│     Bind rare            │
│     Spec OCC             │
│     AbortingThrow LAST   │──► only EV[Aborting]>EV[hold] ∧ low depth_frac
│   cert on success; process.record(shape); Bayes.update
│   publish → release edges → enqueue Validate
└──────────────────────────┘
```

### 2.2 When Bayes is queried (mandatory)

| Moment | Query | Writes / returns |
|--------|-------|------------------|
| begin_block | `P_RAW`, HotSet, WŜ, morph, `quiet_cold` | ReadyEdges + PE priors + quiet_occ_mode |
| PC admit (each candidate Execute) | `EV[schedule_refuse]` vs Spec canary | keep t out of ready / allow one canary |
| CC decide (each PE-on access) | `PE(ℓ,k_true)`, `EV[PinHold|Aborting|Bind|B0]`, `P(producer_live)`, `depth_frac` | Mode(a) + WaitFor **shape** |
| Validate / Repair | `P(covers_all∣strips,invalid)`, tip snap prior | R1 vs B0 grain |
| end_block | decay; pack_top; bind_tax / WaitFor-tax falsify | next-block priors |

**Ban:** LiveLearner OR-bool (`ev_win = !quiet_off && (intra \|\| …)`) as sole Fire π. OR-bools may summarize Bayes EV temporarily during land, but ports must exist and be consumed.

### 2.3 When PC admits

```
may_admit_Execute(t):
  ∀ ReadyEdge(t←w): w Done ∨ lane_grant(t)
  ∧ ProducerStage invariant: if any edge, ProducerStage(w) runnable∨Done
  ∧ ¬Bayes.schedule_hold(t)
  ∧ ¬quiet_occ_mode ∨ edge(t)   # known-star edges override quiet computer
```

**First-wave law:** If InterPrior/HotSet/Bayes mark star RAW (e.g. 14689597 producer 38, ℓ star, k≈6), **seed consumer edges before any satellite Execute** — do not wait for mid-tx `access_vis` or post-abort train.

### 2.4 When CC fences

Only on **admitted** Execute, PE-on access:

```
prefer:  (already refused at admit)     # never see doomed mid-tx
then:    WaitFor PinWithoutThrow        # depth_frac high / EV[hold]
then:    SerialLane exclusive
then:    Bind rare (tip==conflict ∧ EV ∧ !tax)
never:   Bind-after-Done as dominant Avoid
last:    AbortingThrow                  # EV says throw; low depth_frac
```

### 2.5 When Repair runs

```
Validate fail:
  fenced_raw := invalid ∩ certified ∧ RAW-class (Bayes/CC)
  if invalid ⊆ cert ∧ tip_identity_or_snap → R1a
  else if fenced_raw covers RAW fail set → R1 on fenced; Spec residual selective B0/ESTIMATE
  else if Spec-only → B0 + train true-k + edge + Bayes
  R1b SuffixRepair when EV[rewind] > EV[rebind] > EV[B0]
Ban: always validate_occ_kernel while strips exist
Ban: incarnation-strict as sole R1a gate
```

---

## 3. Frame objects (unchanged roles; call-order fixed)

| Object | Frame | Notes |
|--------|-------|-------|
| Stage / ready / steal / pipeline / ProducerStage / PinHold | **PC** | PinHold is first-class Stage |
| ReadyEdge | **PC ⊗ CC ⊗ Bayes** | seeded at begin_block + abort; not mid-tx-only |
| Mode(a) / Fence / cert / lane | **CC** ← Bayes queries | — |
| BayesState posteriors / EV / liveness / covers | **Bayes** | shared structure ports |
| Repair grain | **PC** ← CC⊗Bayes | R1 live |

Makespan law, unfinished=!done, Soft=0, sticky-cert ban, template PE ban — **kept** from v9 §§1.2–1.3 (with new falsifiers §1 above).

---

## 4. WaitFor / Resolve redesign (absorb M1–M5) — kept from v9, call-order enforced

| Killer | v9.1 enforcement point |
|--------|------------------------|
| M1 Aborting WaitFor | **CC act + PC PinHold**; pevm Blocking arm must not default Aborting+FullRetry for PinWithoutThrow |
| M2 Spec siblings | **Validate** fenced RAW prefix selective R1 |
| M3 incarnation-strict | **Validate** tip identity / value snap first-class |
| M4 quiet first wave | **Bayes→PC.admit_seed** before Execute; quiet_cold only when truly cold |
| M5 cert wipe | **certificate.begin_execute** strip survival across PinHold / same-tx resume; begin_block clear only |

---

## 5. Delete / merge / rewire list (explicit)

### 5.1 Delete or quarantine (not SpecFence live π)

| Module / symbol | Action |
|-----------------|--------|
| `edge.rs` `choose_edge_action` as π | **Delete** from hot path; keep Detect classify helpers or merge into sketch/access_log |
| `resolve.rs` `choose_action` / AEC `PolicyCtx` as SpecFence π | **Quarantine** `research_`; remove from `SpecFenceCtx::choose_resolve` production surface |
| `bayes.rs` `decide` / `should_wait_hard` Boolean π | **Delete** Boolean APIs; replace with query ports |
| `mode.rs` thin reexport | **Delete** (use `access_policy` directly) |
| SoftWait Soft arming paths in `rem`/`dag` as Avoid | **Dead code quarantine**; Soft=0 forever |
| `boundary.rs` Bind-snap / absolute jump / inspect as plant path | **Quarantine** research; not v9.1 land critical path |
| `mod.rs` Iter6–30 SoftWait archaeology as module SoT | **Move** to lab archive; header → v9.1 |

### 5.2 Merge

| From | Into |
|------|------|
| `kernel.rs` tx-global `note_fence` / `may_resolve` | `certificate.rs` strip-only resolve authority |
| LiveLearner OR-bool Fire (`ev_win`, …) | Bayes EV query wrappers (temporary adapters OK) |
| Mid-tx HotSet/WŜ edge insert in `access_vis` | begin_block + abort **admit_seed** (vis may refresh, must not be first insert) |
| Dual validate museums in `try_validate` | `validate_specfence` tip snap / identity first-class |

### 5.3 Rewire (call-order — the land)

| Current | v9.1 |
|---------|------|
| `quiet_fence_off` ⇒ `next_occ_task` whole computer | quiet_cold ∧ no edges only; **known-star edges keep SF computer** |
| Edge insert in `access_vis` / post-abort | **Bayes→admit_seed** first; abort strengthens |
| `decide` without Bayes | `decide ← Bayes.queries` |
| `pcc_wait_for_writer` → Blocking → Aborting | PinWithoutThrow → PinHold Stage; AbortingThrow last |
| Done→Bind fallthrough dominant | schedule-refuse earlier; Bind rare only |
| `validate_specfence` → mostly `validate_occ_kernel` | split RS; R1 live; tip snap |
| `begin_execute(inc==0)` wipe after WaitFor | strip survival |
| `computer.rs` thin next_sf only | ready law includes PinHold + Validate priority |

### 5.4 Keep (correct direction @ tip)

| Keep | Why |
|------|-----|
| Soft=0 | held |
| unfinished=!done (S2) | held |
| Bind tip_is_conflict_producer ∧ bind_tax_losing | directionally right; timing wrong |
| ReadyEdge known-consumer refuse (not suffix-global) | deadlock ban held |
| ProducerStage reserve/promote | needed; must run **before** satellite Execute |
| empty-PE ordinal HashMap = 0 | held |
| fan_out no `[1,6,10,20]` template spray | held in learner |
| ESTIMATE observe must not mark PE | held |

---

## 6. Module map (target)

```
specfence/
  computer.rs       # ready/steal/pipeline + ProducerStage + PinHold  [REWIRE]
  ready_edge.rs     # edges; begin_block seed API                     [REWIRE]
  producer_stage.rs # reserve/progress                                [KEEP+]
  access_policy.rs  # decide ← Bayes.queries; WaitFor shapes          [REWIRE]
  access_vis.rs     # unfinished=!done; liveness; no first edge insert[REWIRE]
  access_log.rs     # ordinal.note PE-on                              [KEEP]
  certificate.rs    # strips + survival; absorb kernel may_resolve    [MERGE]
  repair.rs         # R1a/R1b/selective/B0 — wired, not token         [REWIRE]
  lane.rs           # exclusive; ban Ready+Spec                       [REWIRE]
  bayes.rs          # FIRST-CLASS query ports                         [REWIRE]
  learner.rs        # feeds Bayes + edges; OR-bool demoted            [REWIRE]
  executor.rs       # validate split; tip snap; not always OCC kernel [REWIRE]
  process.rs        # Fence verb + WaitFor shape (pin|Aborting|refuse)[REWIRE]
  hotset.rs/prior.rs# begin_block → Bayes + ReadyEdge                 [REWIRE]
  edge.rs           # Detect helpers only OR delete π                 [DELETE π]
  resolve.rs        # research quarantine                             [QUAR]
  kernel.rs         # merge into certificate                          [MERGE]
  mode.rs           # delete                                          [DELETE]
  boundary/rem …    # research SoftWait/snap quarantine               [QUAR]
pevm.rs / scheduler.rs / vm.rs   # wire admit→decide→PinHold→R1       [REWIRE]
```

---

## 7. Morph recipes (bars raised)

| Morph | Structure | Target |
|-------|-----------|--------|
| **fan_out** (14689597) | admit_seed all star consumers←38 @ k_true≈6; ProducerStage(38); refuse/PinHold ≫ Bind-after-Done; R1 on certified | **≥0.90 @8 N≥3** (stretch ≥0.95–1.0); Bind-after-Done **<10%**; R1 win ≥50% |
| **spine** (19807137) | OrderedAdmit + off-spine steal; Aborting WaitFor mass without head progress = fail | **≥0.70** then ratchet; abort_SF≤OCC |
| **quiet** | OCC identity | median ≥0.98; p10 ≥0.90 |
| **mixed** | edges per class; minimal Fence | median ≥0.92 |

---

## 8. Implementation posture

- **Design only; DO NOT implement Rust under this note until authorized.**  
- No P0/P1/P2 — one coherent **call-order** land (Bayes→admit→decide→PinHold/Refuse→R1).  
- Partial land (WaitFor volume without admit rewrite; R1 without tip snap; Bayes bump without port consume) = **non-land**.  
- Land brief: `lab/notes/specfence-v9-land-brief.md`.  
- Audit: `lab/notes/specfence-v9-whole-plant-callflow-audit.md`.

---

## 9. Success checklist (when eventually implemented)

1. Soft=0, exclude=0.  
2. Nonempty median **≥0.95** with JSON (stretch 0.98).  
3. Quiet median ≥0.98 **and** quiet p10 ≥0.90; mixed ≥0.92.  
4. **14689597 ≥0.90 @8 N≥3** (stretch ≥0.95); Bind-after-Done <10%.  
5. R1 win rate ≥50% on cert-bearing fan_out validate fails; R1b used when EV wins.  
6. WaitFor↑ must not coexist with abort≈OCC; AbortingThrow rare.  
7. useful_EVM fraction meets B11.  
8. Bayes queries consumed at admit/decide/validate — unused = fail.  
9. Dual π (`choose_edge_action` / AEC / `should_wait_hard`) absent from hot path.  
10. Empty-PE cold path ≡ OCC; known-star priors still seed edges.  
11. **PC ⊗ CC ⊗ Bayes co-equal** with **admit-before-Execute** call order.

---

## 10. Essence restated

Fusion = three peers **and** a single call spine: Bayes writes ReadyEdges/PE/EV/covers; PC admits and schedules ProducerStages, PinHolds, Validates, Repairs; CC decides Fence verbs only on admitted work and Resolve must convert certs into R1 with tip snap. Fence is timely (refuse/pin), not Aborting theater or Bind-after-Done; quiet is OCC; Soft=0; product bars demand median near OCC and fan_out ≥0.90 with live R1 — not a 0.744 honesty participation trophy. **v9.1 supersedes v9 by raising bars and making the call-flow rewrite the SoT.**
