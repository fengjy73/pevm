# SpecFence v9.1 code-structure / module-architecture audit (tip `bb67ff7`)

**Date:** 2026-09-14 (Asia/Shanghai, UTC+8)  
**Tip:** `bb67ff7` Soft=0  
**Mandate:** inventory + structural smells + measured contradictions; design clean module tree for v9.1 plant.  
**Design only — no Rust edits.**  
**Companion (call-flow):** `lab/notes/specfence-v9-whole-plant-callflow-audit.md`  
**Bars / call spine SoT:** `lab/notes/specfence-complete-architecture-v9.1-cc-pc-bayes.md`  
**Module land SoT (supersedes §5–§6 land/module of v9.1):** `lab/notes/specfence-complete-architecture-v9.2-module-structure.md`  
**Land brief:** `lab/notes/specfence-v9-land-brief.md`

---

## 0. Scale at tip

| Bucket | Files | LOC |
|--------|------:|----:|
| `crates/pevm/src/specfence/*.rs` (flat, 31 mods) | 31 | **19 684** |
| Museum / `#![allow(dead_code)]` / research-heavy (edge+resolve+bayes+mode+engagement+heat+region+kernel+sketch+dag+rem+boundary+finegrain) | 13 | **13 133** (~67%) |
| Live-critical candidates (computer…decision_field+mod, excl. museums above) | 17 | **6 551** (~33%) |
| Seams: `vm.rs` + `pevm.rs` + `scheduler.rs` + `mv_memory.rs` | 4 | **6 796** |
| SpecFence-named refs in `vm.rs` alone | — | **~463** match lines; gate/WaitFor/Bind/SerialLane cluster **~L411–~L900** |

**Verdict:** SpecFence is a **flat god cratelet** where museum mass (~13k) outnumbers the live spine (~6.5k), and the real PC loop + Fence acts live in **pevm/vm** seams — not in `computer.rs` (45 LOC).

---

## 1. File inventory — `crates/pevm/src/specfence/`

Legend: **live** = on SpecFence hot path @ tip; **dead_π** = `#![allow(dead_code)]` or Boolean π unused by gate; **museum** = research/OFF default; **dup_π** = second/third decide vs `access_policy::decide`; **PCC** = SpecFence stub / PCC-only.

| File | LOC | Role @ tip | Status | Dup π / notes |
|------|----:|------------|--------|---------------|
| `mod.rs` | 659 | Module SoT header (still **v8** + Iter6–30 SoftWait archaeology); `SpecFenceCtx` god bag; reexports all museums; dead `choose_resolve` / `should_wait_*` | **live wiring + doc museum** | Exports `choose_edge_action`, `choose_action` |
| `computer.rs` | 45 | `next_sf_task`: ProducerStage then `scheduler.next_task_with_wave_ready` | **live thin** | SoT header still v8; no PinHold / Validate Stage law |
| `ready_edge.rs` | 211 | Consumer←producer bits; `may_execute` refuse | **live incomplete** | No begin_block consumer fan-out seed |
| `producer_stage.rs` | 83 | Reserve / promote / next_reserved | **live weak** | Not authoritative before satellite Execute |
| `access_policy.rs` | 401 | **Sole live Mode(a) decide** (`decide` ← LiveLearner OR-bools) | **live π** | Never queries Bayes |
| `access_vis.rs` | 68 | unfinished=!done compose | **live** | Mid-tx first ReadyEdge insert (too late) |
| `access_log.rs` | 104 | PE-on ordinal `k` | **live** | Keep |
| `certificate.rs` | 187 | Fence strips; `begin_execute` wipe (M5) | **live broken** | Split-brain vs `kernel` |
| `kernel.rs` | 132 | Tx-bool `note_fence` / `may_resolve` / rem_legal | **live parallel SoT** | Merge into certificate |
| `repair.rs` | 56 | `covers_all` → R1 else B0 | **live token** | No selective fenced RAW / tip snap / R1b |
| `lane.rs` | 85 | SerialLane table | **live weak** | Ready→Spec canary in vm |
| `executor.rs` | 341 | `validate_specfence` / `validate_occ_*` / `plant_is_occ` | **live wrong shape** | Dual validate → mostly OCC kernel |
| `learner.rs` | 1758 | PE, quiet_fence_off, OR-bool EV, morph, bind_tax | **live π owner** | Owns what Bayes should; `#![allow(dead_code)]` |
| `bayes.rs` | 531 | BetaMap; `decide` / `should_wait_hard` | **dead_π museum** | Observe-only on hot path; **dup_π** |
| `hotset.rs` | 322 | Track / fanout cache | **live feature** | Feeds vis mid-tx; not admit_seed |
| `prior.rs` | 205 | Inter RwPrior / WŜ | **live feature** | Mild bayes bump; no ReadyEdge seed |
| `sketch.rs` | 900 | HotSketch / residual bind / PE templates | **live+museum mix** | Large; template PE banned on fan_out but code mass remains |
| `process.rs` | 493 | ExecProcess / Fence reason snaps | **live telemetry** | WaitFor pin vs Aborting not first-class |
| `metrics.rs` | 1167 | Counters / hist | **live bloat** | Still names `choose_edge_action` eras |
| `decision_field.rs` | 366 | Feature×verb research snaps | **observe** | Docs cite dead `choose_edge_action` |
| `edge.rs` | 959 | Detect + **`choose_edge_action`** | **dead_π dup** | `#![allow(dead_code)]`; tests only |
| `resolve.rs` | 845 | AEC **`choose_action`** / PolicyCtx | **dead_π dup** | Lab `SpecFenceCtx::choose_resolve` only |
| `mode.rs` | 3 | Reexport `access_policy::*` | **DELETE** | Zero value |
| `engagement.rs` | 308 | Lean execute + Quiet/Storm label; research_inspect | **demote** | Morph label ≠ Avoid peer |
| `heat.rs` | 64 | Account EWMA | **PCC** | SpecFence SoftWait seed banned |
| `region.rs` | 103 | RegionTable Wait bits | **PCC stub** | SpecFence `should_wait` always false |
| `dag.rs` | 463 | FenceGraph SoftWait / hard_wait | **mix** | Soft=0 SoftWait dead; hard_wait used |
| `rem.rs` | 3297 | WavePark + SoftWait + SuffixRepair + research plant | **god museum** | Keep WavePark; SoftWait Soft quarantine |
| `boundary.rs` | 3548 | Bind-snap / inspect / jump | **research museum** | Production OFF |
| `finegrain.rs` | 1980 | Lab collectors | **museum** | Opt-in |

### 1.1 Seams (outside `specfence/`)

| File | LOC | SpecFence role | Smell |
|------|----:|----------------|-------|
| `vm.rs` | 3240 | `specfence_access_gate`, `pcc_wait_for_writer`, Bind, SerialLane, `access_vis`, cert note, ready release | **God seam** — CC Fence acts buried in EVM host |
| `pevm.rs` | 1995 | begin_block tables; worker hybrid OCC↔SF; Blocking→Aborting+steal; validate dispatch | **Split-brain PC** — owns loop computer.rs claims |
| `scheduler.rs` | 878 | `may_execute` + wave ready + ProducerStage promote | Partial PC; refuse only *known* consumers |
| `mv_memory.rs` | 683 | `prior_read_value_stable` (incarnation-strict), selective invalidate | Tip snap identity wrong for R1 (M3) |

---

## 2. Structural smells

### 2.1 God files / god bags

| Smell | Evidence |
|-------|----------|
| Flat 31-file cratelet | No `pc/` `cc/` `bayes/` `fuse/` layers; everything peer-exported from `mod.rs` |
| `mod.rs` archaeology | ~150 lines Iter6–30 SoftWait novel as module docs; cites **v8** SoT |
| `SpecFenceCtx` god bag | Every table pointer + dead `choose_resolve` / region Wait APIs |
| `vm.rs` SpecFence gate | ~500 LOC cluster: decide act + WaitFor Aborting + Bind-after-Done + SerialLane canary + mid-tx edge insert |
| `rem.rs` 3.3k | WavePark (PC-needed) fused with SoftWait Soft + research SuffixRepair plant |
| `boundary.rs` 3.5k | Research Bind-snap/jump co-located with production crate |
| `learner.rs` 1.8k | PE + quiet law + OR-bool Fire + morph + tax — Bayes peer demoted |
| `metrics.rs` 1.2k | Era counters for dead π verbs |

### 2.2 Split-brain (computer thin vs pevm worker)

```
SoT / computer.rs header:  "SpecFenceComputer owns ready/steal/pipeline"
tip reality:
  computer::next_sf_task     ≈ 45 LOC thin wrapper
  pevm worker               owns quiet_fence_off → OCC computer,
                            Blocking→Aborting+steal_without_park,
                            validate branch, begin_block (no admit_seed)
  scheduler                 owns may_execute + wave steal
```

PC is **not** a module — it is a **behavior scattered** across pevm+scheduler+thin computer.

### 2.3 Dual / triple validate & resolve

| Path | When | Outcome @ tip |
|------|------|---------------|
| `validate_specfence` | SpecFence + has path | Often falls to `validate_occ_kernel` |
| `validate_occ_kernel` / `validate_occ_stage` | no cert / fail / Occ | B0 + PE train + late ReadyEdge |
| `specfence_r1_validate` + `repair_grain` | tests / token | R1a≈4 total; R1b unused |
| `resolve::choose_action` | museum | Not wired to validate |
| `rem` SuffixRepair | research / legacy | Not default R1b |

### 2.4 Learner vs Bayes vs prior vs HotSet ownership mess

| Concern | Who owns @ tip | Who *should* (v9.1) |
|---------|----------------|---------------------|
| Quiet first wave | `LiveLearner::quiet_fence_off` → OCC computer | Bayes.quiet_cold ∧ no edges |
| PE(ℓ,k) | learner + sketch templates | Bayes PE posterior / ports |
| Fire/Wait EV | learner OR-bools in `decide` | Bayes EV[Pin\|Aborting\|Bind\|B0] |
| Conflict posterior | BayesMap observe-only | Queried at admit/decide/validate |
| Star consumers | HotSet track mid-tx / abort | begin_block ReadyEdge admit_seed |
| Morph | engagement label + learner | Bayes morph prior only |

### 2.5 Certificate / repair / resolve / kernel overlap

```
certificate strips  ──┐
kernel note_fence   ──┼── three "may resolve?" authorities
repair covers_all   ──┤
resolve AEC museum  ──┘
rem SuffixRepair research
```

No single strip-survival + tip-snap + selective R1 owner.

### 2.6 Process / metrics bloat without product falsifiers

Missing first-class: WaitFor **shape** (PinWithoutThrow | AbortingThrow | ScheduleRefuse), Bind-after-Done share, R1 win rate, useful_EVM fraction — while retaining dead-π era histograms.

### 2.7 Duplicate π museums (measured)

| Symbol | Hot-path callers @ tip | Status |
|--------|------------------------|--------|
| `access_policy::decide` | `vm::specfence_access_gate` | **LIVE** |
| `edge::choose_edge_action` | none outside `edge.rs` tests + reexport | **dead_π** |
| `resolve::choose_action` | `SpecFenceCtx::choose_resolve` only (lab) | **dead_π** |
| `bayes::{decide,should_wait_hard}` | bayes tests only | **dead_π** |
| `mode.rs` | unused reexport | **DELETE** |

---

## 3. Measured contradictions (structure × call-flow)

| # | Structure claim / layout | Live measurement @ `bb67ff7` |
|---|--------------------------|------------------------------|
| C1 | `computer.rs` = PC SoT | 45 LOC; pevm hybrid owns OCC↔SF |
| C2 | Bayes peer module exists (`bayes.rs` 531) | Zero queries from `decide`; BetaMap museum |
| C3 | Triple decide surfaces exported | Only OR-bool `decide` fires; edge/AEC/bayes Boolean dead |
| C4 | `certificate` + `kernel` both "Fence cert" | Dual wipe/authority; M5 wipe on `begin_execute(inc==0)` |
| C5 | `repair.rs` R1 grain | Token; validate falls OCC B0; R1a=4 / R1b=0 |
| C6 | ReadyEdge + ProducerStage modules | Edges filled mid-tx/`access_vis`; Bind-after-Done 442/473 |
| C7 | SoftWait in rem/dag as Avoid | Soft=**0** held — Soft paths are dead weight in 3.7k+ LOC |
| C8 | `mod.rs` documents choose_edge_action spine | Contradicts live `access_policy::decide` |
| C9 | Museum LOC 13.1k vs live 6.5k | Land cost dominated by delete/quarantine, not new features |
| C10 | Call-flow audit top-10 | Same root as structure: no admit-first PC module, Fence acts in vm, Bayes demoted |

(Call-flow numbers: nonempty median **0.728**; **14689597** ≈**0.362**; WaitFor 2260 > Bind 1895; Soft=0 — see whole-plant audit.)

---

## 4. Top 10 structural problems

1. **No layer tree** — flat `specfence/` with 31 peers; PC/CC/Bayes/fuse not enforceable by path.  
2. **Museum mass ~67%** — boundary+rem+finegrain+edge+resolve+… drown the live spine; compile/review tax.  
3. **PC split-brain** — `computer.rs` thin; `pevm` worker owns hybrid loop, Blocking Aborting, begin_block.  
4. **CC Fence acts in `vm.rs`** — gate/WaitFor/Bind/SerialLane god seam (~L411–900) instead of `cc/fence_act`.  
5. **Triple π** — live `decide` + dead `choose_edge_action` + AEC `choose_action` + bayes Boolean; exports still advertise museums.  
6. **Learning ownership mess** — learner owns quiet/OR-bool Fire; Bayes/hotset/prior/sketch/engagement incoherent.  
7. **Cert∥kernel∥repair∥resolve∥rem** — five Resolve authorities; R1 never product-live.  
8. **Dual validate** — `validate_specfence` theater → `validate_occ_kernel`; tip snap incarnation-strict in mv_memory.  
9. **`mod.rs` / SpecFenceCtx god bag** — v8 Iter archaeology + every table + dead APIs as public surface.  
10. **Telemetry bloat without falsifiers** — process/metrics large; product bars (R1 win, Bind-after-Done, WaitFor shape, useful_EVM) not structural.

---

## 5. Proposed clean module tree (target — detail in v9.2)

```
crates/pevm/src/specfence/
  mod.rs                 # thin: mode enum, Ctx wiring, layer reexports; SoT → v9.1+v9.2
  pc/                    # admit + schedule Stages
    mod.rs
    computer.rs          # ready/steal/pipeline + PinHold + Validate priority
    ready_edge.rs        # begin_block admit_seed API
    producer_stage.rs
    lane.rs
    wave.rs              # WavePark extracted from rem
  cc/                    # decide + Fence act + Validate/Repair
    mod.rs
    decide.rs            # today's access_policy; ← Bayes only
    access_vis.rs        # no first ReadyEdge insert
    access_log.rs
    certificate.rs       # + merged kernel
    fence_act.rs         # MOVE from vm: Pin/Refuse/Bind/Serial
    validate.rs          # split RS; tip snap; not always OCC kernel
    repair.rs            # R1a/R1b/selective/B0 wired
  bayes/                 # posteriors + query ports
    mod.rs
    state.rs             # BayesMap; DELETE Boolean decide/should_wait_hard
    learner.rs           # feeds Bayes; OR-bool Fire DELETE
    prior.rs
    hotset.rs
    pe.rs                # PE essentials from sketch (trim)
  fuse/                  # shared telemetry / process honesty
    mod.rs
    process.rs           # WaitFor shape; Bind-after-Done; R1 win
    metrics.rs           # trim dead-π eras
    decision_field.rs    # observe-only
  research/              # quarantine; feature-gated or not on hot path
    boundary.rs
    rem_softwait.rs      # SoftWait Soft + research SuffixRepair plant
    finegrain.rs
    edge_pi.rs           # choose_edge_action museum
    resolve_aec.rs       # choose_action museum
    engagement_extra.rs
    dag_soft.rs
    heat.rs              # PCC
    region.rs            # PCC Wait stub
```

**Seams (stay outside tree, shrink):**  
`pevm.rs` — begin_block calls `bayes→pc.admit_seed`; worker calls `pc::computer` only (no quiet→OCC when edges).  
`scheduler.rs` — steal law matches PC Stages.  
`vm.rs` — EVM host calls `cc::fence_act` / `cc::decide`; no Aborting default.  
`mv_memory.rs` — tip identity / value snap for R1.

### DELETE / MERGE / MOVE / KEEP (summary)

| Action | Targets |
|--------|---------|
| **DELETE** | `mode.rs`; Boolean `bayes::{decide,should_wait_hard}`; hot-path export of `choose_edge_action` / `choose_action`; SoftWait Soft Avoid paths |
| **MERGE** | `kernel` → `certificate`; learner OR-bool Fire → Bayes EV adapters; mid-tx edge insert → admit_seed; dual validate → one `cc/validate` |
| **MOVE** | vm Fence acts → `cc/fence_act`; WavePark → `pc/wave`; museums → `research/`; Iter archaeology → lab archive |
| **KEEP** | Soft=0; unfinished=!done; ReadyEdge known-consumer refuse; ProducerStage; empty-PE cold ≡ OCC; PE-on ordinal; fan_out no template spray |

**Call-order (must match v9.1):**  
`Bayes.seed → PC.admit → Execute(admitted) → CC.decide(Bayes) → Fence(Refuse|PinHold) → Validate/Repair(R1)`.

---

## 6. Essence (structure)

Tip SpecFence is a **flat museum warehouse** (~13k dead/research LOC) glued to a **live OR-bool learner+vm gate** and a **pevm-owned OCC-first worker**, while `computer.rs` / `bayes.rs` / `repair.rs` are **named peers that do not own their SoT responsibilities**. The structural rewrite is not “add folders for aesthetics” — it is **quarantine museums, give PC/CC/Bayes real directories with single decide/validate owners, pull Fence acts out of `vm.rs`, and land delete/merge before any feature patch** so the v9.1 call spine cannot be re-broken by the next WaitFor/Bind/R1 salad.

---

## 7. Deliverable map

| Artifact | Path |
|----------|------|
| This structure audit | `lab/notes/specfence-v9.1-code-structure-audit.md` |
| Module / land SoT | `lab/notes/specfence-complete-architecture-v9.2-module-structure.md` |
| Bars + call-flow SoT (unchanged authority) | `lab/notes/specfence-complete-architecture-v9.1-cc-pc-bayes.md` (+ banner → v9.2 for modules) |
| Land brief (structural order) | `lab/notes/specfence-v9-land-brief.md` |
| Call-flow audit | `lab/notes/specfence-v9-whole-plant-callflow-audit.md` |
