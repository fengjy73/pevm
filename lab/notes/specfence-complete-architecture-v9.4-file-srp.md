# SpecFence complete architecture v9.4 — file single-responsibility (AUTHORITATIVE structure correction)

**Date:** 2026-09-14 (Asia/Shanghai, UTC+8)  
**Status:** **AUTHORITATIVE design SoT for structure law** — **DESIGN ONLY**; **DO NOT land**; **DO NOT implement Rust**; no immediate refactor suggested  
**Tip at write:** `1ee6dda` (docs tip; equivalent content lineage `bb67ff7`+docs)  
**USER CORRECTION (absorbed):** 「不对，不是要把PC cc bayes职责分离，是要把文件本身职责分离」  
**Steering (absorbed):** PC/CC/Bayes triple = **analysis / optimization lens only** — **NOT** a folder-split goal; **no land now**; file-SRP is what to apply **when we eventually clean code**; unified pevm spine (v9.3) remains **conceptually correct**.

**Absorbed / kept:**  
- `lab/notes/specfence-complete-architecture-v9.1-cc-pc-bayes.md` — **bars + call-flow** (AUTHORITATIVE)  
- `lab/notes/specfence-complete-architecture-v9.3-pevm-unified.md` — **one pevm spine**; dual OCC/SF computers **banned** (AUTHORITATIVE)  
- Audits: `specfence-v9.1-code-structure-audit.md`, `specfence-v9-whole-plant-callflow-audit.md` (evidence of god files / dual π / museums)

**Supersedes (structure law only):**  
- v9.2 claim that **SoC success = create `pc/` `cc/` `bayes/` `fuse/` ownership directories** (folder-layer SoC as product requirement)  
- Land-brief **S2 “create pc/cc/bayes dirs” as success**  
- Any reading that “separate PC vs CC vs Bayes into separate ownership directories” is the SE goal

**Does not supersede:** v9.1 bars + Bayes→admit→decide→Fence→Validate call order; Soft=0; WaitFor/Resolve redesign; morphs; falsifiers; **v9.3 unified pevm spine** (PC⊗CC⊗Bayes remain **fused mechanisms** on one spine).

```
v9.1 owns:  BARS + WHAT (Bayes→admit→decide→Fence→Validate) + Soft=0
v9.2 owns:  (DEMOTED) optional packaging names only — NOT structure law
v9.3 owns:  WHERE THE SPINE LIVES — one pevm parallel executor; SpecFence = fused plant
v9.4 owns:  FILE SRP — one file ≈ one job; triple = thinking lens; NO land now
```

---

## 0. Essence (ONE paragraph)

**v9.4** corrects the structure goal: **separation of concerns = file single responsibility** (one file ≈ one job), **not** carving the plant into PC / CC / Bayes **ownership directories**. PC⊗CC⊗Bayes stay **fused mechanisms** on the **one pevm spine** (v9.3): they are **analysis and optimization lenses** for reasoning about admit/schedule, Mode(a)/Fence/validate, and posteriors/EV — **thinking tools**, not three siloed plants or a mandatory folder split. Live tip smells are **god files and dual π museums** (`vm.rs` EVM+Fence, `rem.rs` SoftWait+wave, `learner.rs` megaclass, dual π, `boundary`/`finegrain` museums) — fix those by **KEEP / SPLIT / DELETE / MERGE** of **files**, when cleanup is eventually authorized. v9.2’s `pc/cc/bayes/fuse` tree is **optional packaging**, not the product requirement; if used later, it must not become three siloed plants. **v9.1 bars + call-flow and v9.3 unified spine are kept.** This note is **design-only SoT**: **no land now**, no Rust, no “implement S0 tomorrow.”

---

## 0.1 USER CORRECTION — what was wrong about “SoC”

| Misread (void) | Correct (v9.4) |
|----------------|----------------|
| SoC = put PC in `pc/`, CC in `cc/`, Bayes in `bayes/` as separate ownership plants | SoC = **each file has one job**; split gods; delete dual π; quarantine museums |
| Triple-peer frame ⇒ three directory trees as SE success | Triple = **lens** to optimize **fused** mechanisms on one spine |
| Land S2 = “create the three dirs” | Eventual cleanup success = **file-SRP inventory done**; dirs optional |
| “Separate PC vs CC vs Bayes responsibilities” as ownership split | Keep PC⊗CC⊗Bayes **fused**; separate **file** responsibilities |

**Explicit ban as SE goal:** “separate PC vs CC vs Bayes into separate ownership directories.”

---

## 0.2 Hard bans (v9.4 — structure)

All v9.1 / v9.3 bans **held**, plus:

| Ban | Hold |
|-----|------|
| Treating **folder-layer SoC** (`pc/`/`cc/`/`bayes/` as three plants) as the product structure requirement | **NEW (authoritative)** |
| Land / redesign that **silos** PC, CC, Bayes into three non-communicating ownership trees | **NEW** |
| Claiming v9.2 S2 “dirs exist” as structure success without file-SRP (gods still gods) | **NEW** |
| **Immediate land / Rust refactor** under this note | **NEW** — design only; **no land now** |
| Replacing v9.3 spine unity with “three computers named after the triple” | **NEW** |

**Allowed:**  
- Using PC / CC / Bayes vocabulary in design, audits, and mechanism cuts as **lenses**.  
- Optional later packaging under `pc/`/`cc/`/`bayes/`/`fuse/`/`research/` **if** it remains pevm-owned helpers and does **not** become three siloed plants.  
- Eventual file-SRP cleanup (SPLIT/DELETE/MERGE) **when explicitly authorized** — not implied by this SoT.

---

## 1. Triple frame = analysis / optimization lens (NOT folder goal)

```
PC lens:     admit, Stages, ready/steal, ProducerStage, PinHold, wall decomposition
CC lens:     Detect/Avoid/Resolve, Mode(a), Fence verbs, certs, Validate/Repair
Bayes lens:  P_RAW, PE, EV[shapes], liveness, quiet_cold, queried at ports

Plant:       ONE pevm spine running fused PC⊗CC⊗Bayes mechanisms (v9.3)
Folders:     optional labels — never the success criterion
```

**Use the triple to ask:** “Is admit seeded before Execute?” “Does decide query Bayes?” “Is Fence PinWithoutThrow not Aborting?”  
**Do not use the triple to demand:** “Must we create three ownership directories?”

Call order remains v9.1: **Bayes → PC.admit → Execute(only if admitted) → CC.decide ← Bayes → Fence → Validate/Repair** — as **fused call-flow on one spine**, not as three packages calling across walls.

---

## 2. Violations today = file SRP failures (not “missing folders”)

Evidence tip (`bb67ff7` / docs `1ee6dda`); see structure audit.

| File (smell) | Jobs crammed together | File-SRP violation |
|--------------|----------------------|--------------------|
| **`vm.rs` god** | EVM host **+** SpecFence gate **+** WaitFor/Bind/SerialLane Fence policy | EVM interpreter host ≠ Fence-act policy body |
| **`rem.rs` god** | WavePark (needed) **+** SoftWait Soft **+** SuffixRepair research plant | Wave scheduling ≠ SoftWait museum |
| **`learner.rs` megaclass** | PE feed **+** quiet_fence_off computer switch **+** OR-bool Fire EV **+** morph/tax | Feeder ≠ decide π ≠ cost_class switch |
| **Dual π files** | `access_policy::decide` (live) **vs** `edge::choose_edge_action` / `resolve::choose_action` / `bayes::{decide,should_wait_hard}` | One Mode(a) decide; museums impersonate π |
| **`boundary.rs` / `finegrain.rs` museums** | Research Bind-snap/jump / lab collectors co-located as peer modules | Research ≠ default plant surface |
| **`mod.rs` + Ctx bag** | Live wiring **+** Iter archaeology **+** dead reexports | Thin facade ≠ museum novel + dual-π export |
| **`certificate` ∥ `kernel`** | Two “may resolve / strip” authorities | One cert SoT |
| **`pevm.rs` hybrid** | Worker owns OCC↔SF bifurcation as second computer | Spine unity (v9.3) — mechanism smell, also a file duty mess |

**Folder absence is not the primary smell.** God files and dual π would still be wrong inside pretty `pc/`/`cc/`/`bayes/` trees.

---

## 3. Concrete file-SRP target list (KEEP / SPLIT / DELETE / MERGE)

**Posture:** inventory for **eventual** cleanup when authorized. **Not** a land checklist to execute now.

### 3.1 KEEP (direction correct; one job already or nearly)

| Keep | One job |
|------|---------|
| Soft=0 forever | product bar |
| `ready_edge.rs` refuse for **known** consumers (not suffix-global) | admit refuse law |
| `producer_stage.rs` reserve/promote API shape | ProducerStage lifecycle |
| `access_log.rs` PE-on ordinal | true-k note |
| `lane.rs` SerialLane table (policy body must leave `vm`) | exclusive lane state |
| unfinished=!done (`access_vis` compose) | visibility compose |
| empty∧cold ≡ Spec cost class on **same** spine (v9.3) | quiet identity concept |
| fan_out no `[1,6,10,20]` template spray | PE honesty |
| `ConcurrencyMode::Occ` as **separate mode** (not SpecFence cold retreat) | pure OCC callers |

### 3.2 SPLIT (god → one job per file)

| From | Split into (conceptual duties) | Notes |
|------|--------------------------------|-------|
| **`vm.rs`** | (a) EVM / storage host thin calls; (b) Fence-act / gate **policy** extracted to a dedicated fence-act module (name free; not “must live under `cc/`”) | Extract Fence; do not leave Aborting WaitFor body in interpreter file |
| **`rem.rs`** | (a) **wave-only** (WavePark + steal-after-park); (b) SoftWait Soft + research SuffixRepair → quarantine | **rem → wave only** on product surface |
| **`learner.rs`** | (a) **feeder** (priors / PE / features into Bayes ports); (b) **decide** must not live here — Mode(a) stays one decide symbol; (c) quiet as **Bayes.cold / cost_class**, not computer switch | Split feeder vs decide; kill OR-bool π ownership |
| **`sketch.rs`** | live PE subset vs template/museum mass | Trim; templates not fan_out spray |
| **`mod.rs` / SpecFenceCtx** | thin pubs + Ctx fields needed on hot path vs archaeology / dead APIs | Strip novel; stop exporting dual π |

### 3.3 DELETE

| Target | Why |
|--------|-----|
| `mode.rs` | 3-LOC reexport |
| Hot-path dual π: `edge::choose_edge_action`, `resolve::choose_action`, `bayes::{decide,should_wait_hard}` as SpecFence π | One decide only |
| `SpecFenceCtx` production exports of museum choose/should_wait_* | Dual π surface |
| SoftWait Soft as Avoid | Soft=0 |
| Product use of `specfence_plant_is_occ` → `next_occ_task` / `validate_occ_stage` retreat | v9.3 spine unity |
| Treating “create `pc/`/`cc/`/`bayes/` dirs” as mandatory DELETE of flat layout **for its own sake** | v9.4 — dirs optional |

### 3.4 MERGE

| From | Into (duty) |
|------|-------------|
| `kernel.rs` | single **certificate** SoT |
| LiveLearner OR-bool Fire used as π | Bayes **query ports** / one decide consumer (not a second decide file) |
| Mid-tx first ReadyEdge insert in `access_vis` | begin_block / abort **admit_seed** owner |
| SpecFence validate fork salad | **one** validate entry on pevm spine (v9.3) |

### 3.5 QUARANTINE (research — not default hot path)

| Target | Why |
|--------|-----|
| `boundary.rs` | Bind-snap / inspect / jump museum |
| `finegrain.rs` | lab collectors |
| SoftWait Soft arms inside `rem` (after wave extract) | Soft=0 |
| `edge` / `resolve` π bodies (after delete from hot surface) | dead_π museums |
| `heat` / `region` Wait stubs unused by SpecFence π | PCC/research |
| engagement inspect ladders unused by π | research |

**Success of eventual cleanup:** gods split, dual π gone, museums gated, one decide, one cert, wave-only rem, learner feeder≠decide, vm thin host — **whether or not** `pc/cc/bayes` folders exist.

---

## 4. Banner: v9.2 folder-layer SoC demoted

| Version | Structure claim | v9.4 status |
|---------|-----------------|-------------|
| **v9.2** | Filesystem law = `pc/` `cc/` `bayes/` `fuse/` `research/` ownership layers | **DEMOTED** to **optional packaging**; not product requirement; must not silo the triple |
| **v9.4** | Structure law = **file SRP** (KEEP/SPLIT/DELETE/MERGE/QUARANTINE) | **AUTHORITATIVE** |
| **v9.1** | Bars + call-flow | **KEPT** |
| **v9.3** | One pevm spine; fused PC⊗CC⊗Bayes; ban dual computers | **KEPT** |

v9.2 DELETE/MERGE/MOVE **inventory ideas** remain useful as **hints** for file-SRP work; the **folder tree as SoC** does not.

If packaging dirs are used later:

```
OK:   thin pevm-owned helper modules grouped for humans
BAD:  three siloed plants / separate next_task authorities / “CC crate vs PC crate”
```

---

## 5. Relation to land brief (conceptual order only — **no land now**)

When land is **eventually** authorized (not now), conceptual order:

| Phase | Intent | Success ≠ |
|-------|--------|-----------|
| **S−1** | Unify pevm spine (v9.3) — one next / validate / execute for SpecFence mode | “hybrid quiet still OK” |
| **S0** | **File-SRP:** split gods (`vm` Fence extract, `rem`→wave only, learner feeder vs decide); **delete dual π**; **quarantine museums** | “created `pc/cc/bayes` dirs” |
| **Then** | Mechanism cuts (admit_seed, PinWithoutThrow, decide←Bayes, R1 live, …) on unified spine | Patch salad into gods |

**S2 “create pc/cc/bayes dirs” is not a success criterion.** Optional packaging may happen incidental to file moves; it is **not** the SE goal.

**This SoT does not authorize S−1 / S0 / cuts.** Posture: **design only; no land now; no Rust.**

---

## 6. Implementation posture

- **DESIGN ONLY.**  
- **DO NOT land.**  
- **DO NOT implement Rust** under this note.  
- **Do not** treat this document as a mandate to start S0 file splits tomorrow.  
- PC/CC/Bayes remain **lenses** for optimizing fused mechanisms.  
- Unified pevm spine (v9.3) remains **conceptually correct**.  
- File-SRP is the structure law for **eventual** cleanup when the user explicitly authorizes land.

---

## 7. Falsifiers (design / future land)

- SoC “done” because `pc/` `cc/` `bayes/` folders exist while `vm`/`rem`/`learner` remain gods  
- Triple used to justify three ownership silos or three schedule authorities  
- Dual π still on hot path after a claimed cleanup  
- Museums (`boundary`/`finegrain`/SoftWait Soft) still default-hot after claimed quarantine  
- Quiet/cold still a second OCC computer (v9.3 regression)  
- “Land now” implied from this note alone  

---

## 8. Essence restated

Wrong SoC: three folders named PC, CC, Bayes. Right SoC: **one job per file**, museums out, dual π dead, gods split — while PC⊗CC⊗Bayes stay **fused** on **one pevm spine** and the triple stays a **thinking lens** for mechanism optimization. v9.2 folder-layer law is demoted; **file-SRP is structure law**; v9.1 bars/call-flow and v9.3 spine **kept**. **No land now.** Soft=0.
