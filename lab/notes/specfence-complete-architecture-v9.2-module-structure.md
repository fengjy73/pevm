# SpecFence complete architecture v9.2 — module / land structure (AUTHORITATIVE for layout)

**Date:** 2026-09-14 (Asia/Shanghai, UTC+8)  
**Status:** **AUTHORITATIVE for module tree, ownership, DELETE/MERGE/MOVE/KEEP, structural land order** — design only; **DO NOT implement Rust yet**  
**Tip at write:** `bb67ff7`  
**Bars + call-flow SoT (unchanged):** `lab/notes/specfence-complete-architecture-v9.1-cc-pc-bayes.md`  
**Structure audit:** `lab/notes/specfence-v9.1-code-structure-audit.md`  
**Call-flow audit:** `lab/notes/specfence-v9-whole-plant-callflow-audit.md`  
**Land brief:** `lab/notes/specfence-v9-land-brief.md`

> **Unified pevm spine:** superseded for **plant ownership / dual OCC↔SF computer ban / quiet as Spec cost class** by  
> `lab/notes/specfence-complete-architecture-v9.3-pevm-unified.md`.  
> **Module names** (`pc/` `cc/` `bayes/` `fuse/` `research/`) and DELETE/MERGE inventory **still AUTHORITATIVE here**,  
> but layers are **owned by the pevm spine** (integration into `pevm.rs` / `scheduler.rs` / `vm.rs`), not a parallel  
> crate computer. Seam text that leaves pevm as a thin hybrid switch (`next_occ` vs `next_sf`) is **void**.  
> Bars + call-flow remain v9.1; spine unity is v9.3.

> **Structure law (v9.4):** folder-layer SoC (`pc/`/`cc/`/`bayes/` as product requirement) is **DEMOTED**.  
> Authoritative structure correction: `lab/notes/specfence-complete-architecture-v9.4-file-srp.md` — **file single responsibility**;  
> PC/CC/Bayes = analysis lenses only, not folder-split goal; tree below = **optional packaging** only.  
> Bars + call-flow remain v9.1; pevm spine unity remains v9.3. **No land now.**


**Supersedes only:** v9.1 §§5–6 (delete/merge list + flat module map) and any land-map that keeps a flat 31-file `specfence/` as the plant shape.  
**Does not supersede:** v9.1 product bars (§1), call-flow spine (§2), WaitFor/Resolve redesign (§4), morph recipes, falsifiers.

```
v9.1 owns:  WHAT the plant does (Bayes→admit→decide→Fence→Validate/Repair) + BARS
v9.2 owns:  WHERE code lives (pc/ cc/ bayes/ fuse/ research/) + structural land ORDER
```

---

## 0. Essence (ONE paragraph)

**v9.2** is the **module plant** for the v9.1 call spine: four live layers — **`pc/`** (admit, Stages, ready/steal, ProducerStage, PinHold, WavePark), **`cc/`** (decide←Bayes, Fence acts, certificate+kernel, validate/repair), **`bayes/`** (query ports, learner-as-feeder, prior/hotset/PE), **`fuse/`** (process honesty + metrics) — plus **`research/`** quarantine for SoftWait/Bind-snap/finegrain/edge-π/AEC museums. Flat 31-file SpecFence with ~67% museum LOC and Fence acts buried in `vm.rs` is **banned** as a land shape. **Delete/merge/quarantine before** PinHold/R1/admit feature patches. Call-order and bars remain v9.1; this note only makes the filesystem enforce ownership so dual π and pevm/vm split-brain cannot silently return.

---

## 1. Hard structure bans

| Ban | Hold |
|-----|------|
| Flat `specfence/*.rs` as long-term plant (31 peers, no layers) | **NEW** |
| Shipping feature patches (WaitFor/Bind/R1) before museum DELETE/MERGE | **NEW** |
| Leaving Fence acts (`pcc_wait_for_writer`, Bind, SerialLane, gate) permanently in `vm.rs` | **NEW** |
| `computer.rs` thin wrapper while `pevm` owns OCC↔SF hybrid as "PC" | **NEW** |
| Exporting `choose_edge_action` / AEC `choose_action` / `bayes.should_wait_hard` on SpecFenceCtx hot surface | **NEW** (v9.1 held) |
| Compiling `boundary` / SoftWait Soft / finegrain on default hot path without `research/` gate | **NEW** |
| Second Mode(a) decide anywhere outside `cc/decide` | **NEW** |
| Second validate authority that always falls to OCC kernel while strips exist | **NEW** (v9.1 held) |

---

## 2. Target module tree

```
crates/pevm/src/specfence/
  mod.rs                      # THIN: ConcurrencyMode, SpecFenceCtx, layer pubs; docs → v9.1+v9.2
  pc/
    mod.rs
    computer.rs               # ready/steal/pipeline; PinHold Stage; Validate priority; ProducerStage first
    ready_edge.rs             # ReadyEdgeTable + begin_block / abort admit_seed API
    producer_stage.rs         # reserve / promote / runnable invariant
    lane.rs                   # exclusive SerialLane; ban Ready→Spec canary
    wave.rs                   # WavePark (+ steal-after-park) EXTRACTED from rem.rs
  cc/
    mod.rs
    decide.rs                 # Mode(a) ← Bayes.queries ONLY (from access_policy.rs)
    access_vis.rs             # unfinished=!done; refresh OK; NO first ReadyEdge insert
    access_log.rs             # PE-on ordinal.note
    certificate.rs            # strips + survival; ABSORB kernel may_resolve / rem_legal
    fence_act.rs              # PinWithoutThrow / ScheduleRefuse / Bind rare / SerialLane / AbortingThrow LAST
                              #   (MOVED out of vm::pcc_* / specfence_access_gate body)
    validate.rs               # RS_spec bool + RS_fence tip_id/snap; selective R1 (from executor validate_*)
    repair.rs                 # R1a / R1b / selective / B0 — wired, not token
  bayes/
    mod.rs
    state.rs                  # BayesMap query ports: P_RAW, PE, EV[*], liveness, P(covers_all), quiet_cold
                              #   DELETE Boolean decide / should_wait_hard / RegionMode Wait π
    learner.rs                # feeds Bayes + PE priors; quiet_fence_off demoted; OR-bool Fire DELETE
    prior.rs                  # InterPrior / WŜ → Bayes.seed + admit_seed
    hotset.rs                 # track / fanout features → Bayes + admit_seed (not mid-tx first edge)
    pe.rs                     # PredictedEssential surface (trim from sketch.rs live subset)
  fuse/
    mod.rs
    process.rs                # Fence verb + WaitFor shape (pin|Aborting|refuse) + Bind-after-Done
    metrics.rs                # product falsifiers; trim dead-π era counters
    decision_field.rs         # observe-only feats (not π)
  research/                   # QUARANTINE — feature `specfence_research` or cfg; not default hot path
    mod.rs
    boundary.rs               # Bind-snap / inspect / absolute jump
    rem_softwait.rs           # SoftWait Soft + research SuffixRepair / PartialRetry plant
    finegrain.rs
    edge_pi.rs                # choose_edge_action + Detect museum (helpers may MERGE live Detect → pe/access_log)
    resolve_aec.rs            # choose_action / PolicyCtx
    engagement_extra.rs       # inspect flags / abort-rate ladders unused by π
    dag_soft.rs               # SoftWait Soft graph paths
    heat.rs                   # PCC account heat
    region.rs                 # PCC RegionTable Wait stub
```

### 2.1 Seam files (outside tree — shrink SpecFence surface)

| Seam | v9.2 duty |
|------|-----------|
| `pevm.rs` | begin_block: `bayes.seed` → `pc.admit_seed`; worker: **only** `pc::computer::next_*`; Blocking path must not default Aborting for PinHold |
| `scheduler.rs` | Implement PC steal law; refuse PE-blocked Execute; admit_spine for ProducerStage |
| `vm.rs` | EVM host: call `cc::decide` + `cc::fence_act`; storage read unchanged; **no** inline WaitFor Aborting policy |
| `mv_memory.rs` | Tip identity / value snap APIs for `cc/validate` (non-incarnation-strict first) |

---

## 3. Ownership of the call spine

Must match v9.1 §2:

| Spine step | Layer | Primary module(s) |
|------------|-------|-------------------|
| begin_block seed | **bayes/** → **pc/** | `bayes/state` + `prior`/`hotset`/`pe` → `pc/ready_edge` admit_seed + `producer_stage` |
| PC admit / schedule | **pc/** | `computer`, `ready_edge`, `producer_stage`, `wave`, `lane` |
| Execute only if admitted | **pc/** gate; host in pevm/vm | `ready_edge.may_execute` + computer Stages |
| CC decide | **cc/** ← **bayes/** | `cc/decide` ← `bayes/state` queries |
| Fence act | **cc/** → **pc/** | `cc/fence_act` → PinHold Stage / refuse / rare Bind / AbortingThrow last |
| Validate / Repair | **cc/** ← **bayes/**; Stage in **pc/** | `cc/validate` + `repair`; tip snap via mv_memory |
| end_block update | **bayes/** + **fuse/** | decay; pack_top; falsify bind_tax / WaitFor-tax |

**Single decide:** `cc/decide` only.  
**Single cert SoT:** `cc/certificate` (kernel merged).  
**Single validate entry for SpecFence:** `cc/validate` (may call OCC bool for RS_spec subset — not “always OCC kernel while strips”).

---

## 4. DELETE / MERGE / MOVE / KEEP

### 4.1 DELETE

| Target | Why |
|--------|-----|
| `mode.rs` | 3-LOC reexport |
| `bayes::{decide, decide_account, should_wait_hard}` Boolean π APIs | Not SpecFence π; replace with query ports |
| Hot-path reexports of `choose_edge_action`, `choose_action`, `PolicyCtx` on Ctx | Dual π |
| SoftWait Soft arming as Avoid | Soft=0 forever |
| `SpecFenceCtx::{choose_resolve, should_wait_account, should_wait_location}` production surface | Museum APIs |
| `mod.rs` Iter6–30 SoftWait novel as SoT | Move text to lab archive note |

### 4.2 MERGE

| From | Into |
|------|------|
| `kernel.rs` | `cc/certificate.rs` |
| LiveLearner OR-bool Fire (`ev_win`, …) | `bayes/state` EV query wrappers (temp adapters OK during land) |
| Mid-tx first ReadyEdge insert in `access_vis` | `pc/ready_edge` admit_seed (begin_block + abort) |
| `executor::{validate_specfence, validate_occ_kernel, …}` SpecFence branch | `cc/validate.rs` |
| Detect helpers still needed from `edge.rs` | `bayes/pe` or `cc/access_log` (not π) |
| Live sketch PE subset | `bayes/pe.rs` |

### 4.3 MOVE

| From | To |
|------|----|
| `vm::{specfence_access_gate body, pcc_wait_for_writer, pcc_bind_published, pcc_serial_lane}` policy | `cc/fence_act.rs` (+ thin vm call) |
| `rem::WaveParkTable` (+ park steal) | `pc/wave.rs` |
| `boundary`, SoftWait rem, finegrain, edge π, resolve AEC, heat, region Wait | `research/` |
| engagement inspect / dead ladders | `research/engagement_extra` (keep lean-execute flag in bayes/fuse if needed) |

### 4.4 KEEP (correct direction)

| Keep | Why |
|------|-----|
| Soft=0 | held |
| unfinished=!done | held |
| ReadyEdge known-consumer refuse (not suffix-global) | deadlock ban |
| ProducerStage reserve/promote | must run before satellite Execute |
| empty-PE cold ≡ OCC | held |
| PE-on ordinal HashMap | held |
| fan_out no `[1,6,10,20]` template spray | held |
| Bind tip_is_conflict ∧ bind_tax_losing direction | timing wrong, signal OK |
| ESTIMATE must not mark PE | held |

---

## 5. Structural land order (before feature patches)

Ship as **one coherent plant**, but **internally** order work so structure cannot regress:

| Phase | Work | Done when |
|------:|------|-----------|
| **S0** | Quarantine / DELETE museums + dual π exports; delete `mode.rs`; strip `mod.rs` Iter novel; stop exporting dead decide | Hot path has **one** decide symbol; research behind gate |
| **S1** | MERGE kernel→certificate; carve `pc/wave` from rem; MOVE fence policy sketch into `cc/fence_act` (even if still Aborting-shaped temporarily) | Single cert SoT; WavePark not under SoftWait file |
| **S2** | Create `pc/` `cc/` `bayes/` `fuse/` dirs; move live files; thin `mod.rs` | Path ownership matches §2 tree |
| **S3** | **Then** v9.1 call-order cuts: Bayes ports → admit_seed → PinWithoutThrow → decide←Bayes → R1 live → cert survival → SerialLane exclusive → telemetry falsifiers → quiet OCC identity | Bars path unblocked |

**Ban:** Land PinHold/R1/admit_seed **into** the flat museum tree without S0–S2 — that is patch salad with folders later.

Feature cut IDs in land brief (0–9) remain; **S0–S2 are prerequisites** to cut 0+ (dual π delete is both S0 and cut 0).

---

## 6. File role matrix (post-land)

| Layer | Owns | Must not own |
|-------|------|--------------|
| **pc/** | Ready set, Stages, steal, admit_seed apply, PinHold, ProducerStage | Mode(a) EV; Beta posteriors; EVM interpreter |
| **cc/** | decide, Fence verb act, strips, validate/repair grain | Scheduler indices; inter-block prior storage |
| **bayes/** | Posteriors, EV queries, PE, quiet_cold, end_block decay | Direct `Err(Blocking)`; ready deque pop |
| **fuse/** | Honesty telemetry / process snaps | π decisions |
| **research/** | Snap/jump/SoftWait/AEC/edge-π/finegrain | Default production plant |
| **pevm/scheduler/vm/mv_memory** | Host / STM / EVM / memory | SpecFence π bodies (call layers instead) |

---

## 7. Relation to v9.1 module map

v9.1 §6 listed a **flat** rewrite checklist (`computer.rs`, `bayes.rs`, …). v9.2 **keeps every responsibility** but **places** them under `pc/` `cc/` `bayes/` `fuse/` `research/` and adds explicit **S0–S2 structural phases**. Implementers treat v9.1 §6 as intent and **this tree as filesystem law**.

---

## 8. Implementation posture

- **Design only; DO NOT implement Rust under this note until authorized.**  
- Partial land = non-land (v9.1 held).  
- Structural S0–S2 before or as the opening of the single PR — never “features first, folders later.”  
- Success checklist = v9.1 §9 **plus**: default crate tree matches §2; dual π absent; `vm` has no WaitFor Aborting policy body; museum LOC not on default hot path.

---

## 9. Essence restated

v9.1 fixed **who calls whom** and **how high the bar is**. v9.2 fixes **where the code is allowed to live** so PC is a real layer (not pevm folklore), CC Fence is not a `vm.rs` god cluster, Bayes is ports not a Beta museum, and ~13k LOC of SoftWait/snap/AEC/edge-π cannot keep impersonating the plant. **Delete and layer first; then wire the spine.**
