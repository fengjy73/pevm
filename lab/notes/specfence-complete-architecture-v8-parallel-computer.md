# SpecFence complete architecture v8 — parallel computer (AUTHORITATIVE SoT)

**Date:** 2026-09-14 (Asia/Shanghai, UTC+8)  
**Status:** **AUTHORITATIVE design SoT — ready for immediate full-batch implement** (user: 这些都得优化)  
**Tip at write:** `3376ac4`  
**Diagnosis:** `lab/notes/specfence-v6-postland-all-blocks-diagnosis.md`  
**Switch+Bind audit:** `lab/notes/specfence-v6-switch-and-bind-tax-audit.md`  
**v7 (absorbed, superseded as SoT):** `lab/notes/specfence-complete-architecture-v7-essence.md`  
**Earlier PC SoT (primary frame restored):** `lab/notes/specfence-parallel-compute-architecture.md`  
**Land brief:** `lab/notes/specfence-v8-land-brief.md`  
**Supersedes as plant SoT:** v6 essence, v7 essence (prescriptions kept), CC-only redesigns.  
**π fields KEPT:** Spec=Region; Mode(a) verbs; Soft=**0**; exclude set; Spec=Region meaning.  
**Honesty bar:** nonempty median **> 0.744**; quiet median ≈1.0 with p10 **≥0.85**; named fan_out **14689597 ≥0.85 @8 N≥3**; Soft=0.  
**Honesty now (tip `3376ac4`):** digest median **0.795**; remasure N=1 median **1.021** (caveat); **14689597 N=3 = 0.336**; R1a=R1b=**0**. **No celebration.**

---

## 0. Essence (ONE paragraph)

**SpecFence v8** is a **preset-order parallel EVM computer**: a task graph of `Execute` / `Validate` / `Repair` stages, a first-class **ready-set**, **work-stealing**, an **execute∥validate pipeline**, and **ProducerStages** that keep writers runnable. Wall obeys  
`wall = useful_EVM + idle + repair + meta`.  
Concurrency control (Mode(a) Fence verbs: Spec \| Bind \| WaitFor \| SerialLane \| OrderedAdmit) is a **layer that annotates ReadyEdges and pins access grain** — it does **not** own the schedule loop, the validate walk, or the rem journal. Default Spec ≡ OCC. Empty PE ∧ no ReadyEdge ⇒ **byte-identical OCC path**. Nonempty PE ⇒ Mode(a) at access **and** edge-admit at schedule, with **ProducerStage-safe refuse** (the v6 ready-refuse deadlock is designed out). Bind is **rare** (tip == conflicting producer ∧ EV win); WaitFor/lane are **primary**. Learning is a **closed loop that writes computer structure** (ReadyEdges, PE posteriors, R1 coverage) — not OR-bool gates. Soft=0 forever. Success = median >0.744 **and** 14689597 ≥0.85 @8 N≥3 **and** quiet p10 ≥0.85 **and** Soft=0 — Bind↑ without abort↓ is failure.

---

## 0.1 Primary frame vs CC layer (non-negotiable)

```
┌──────────────────────────────────────────────────────────────────┐
│ SpecFenceComputer v8  (THE COMPUTER)                             │
│                                                                  │
│  ready = ProducerStages ∪ PE/edge-satisfied Executes             │
│        ∪ Validates ∪ Repairs                                     │
│  steal = independent Stages only (useful_EVM first)              │
│  pipeline: Execute(t) publish → Validate(t) on another core      │
│                                                                  │
│  ┌──────────┐   ┌──────────┐   ┌──────────┐                      │
│  │ Execute  │──►│ Validate │──►│ Repair   │   Stages             │
│  └────┬─────┘   └────┬─────┘   └────┬─────┘                      │
│       │              │              │                            │
│       ▼              ▼              ▼                            │
│  ┌─────────────────────────────────────────┐                     │
│  │ CC LAYER (annotates edges / Mode(a))    │  NOT the computer   │
│  │  ReadyEdge(t←w) gates ready membership  │                     │
│  │  Mode(a) at access: Spec|WaitFor|lane|  │                     │
│  │             Bind(rare)|OrderedAdmit     │                     │
│  │  cert strips → R1 coverage at fail-a    │                     │
│  └─────────────────────────────────────────┘                     │
│       │                                                          │
│       ▼                                                          │
│  shared revm + MvMemory                                          │
└──────────────────────────────────────────────────────────────────┘
```

| Question | Owner |
|----------|-------|
| Which Stage runs on which core now? | **Computer** (ready / steal / pipeline) |
| May Execute(t) enter ready? | **Computer** + ReadyEdge + ProducerStage invariant |
| For this access \(a\), Spec or Fence verb? | **CC layer** (Mode(a)) |
| Miss → B0 or R1 at fail-\(a\)? | **Repair stage** consuming cert coverage |

**Ban:** treating SpecFence as “CC-only Mode(a) redesign” while schedule stays Block-STM indices + wave graft. That is v6 fusion theater.

---

## 0.2 Why v6/v7 still need a PC-primary cut

```
v6 won:   empty-PE OCC retreat; Soft=0; median digest 0.795 > 0.744
v6 lost:  Bind tax 14689597 (0.336 N=3); ready-refuse abandoned (deadlock);
          ordinal dead → templates; always validate_occ_kernel → R1=0;
          HotSet/WŜ → posterior bump only; WaitFor ≪ Bind

v7 prescribed: ProducerStage-safe ReadyEdges; WaitFor/lane primary;
               true-k; RS_fence→R1; Soft=0; no template PE; Bind rare
               — BUT framed as PC⊗CC essence, easy to implement as CC-first

v8:       PRIMARY FRAME = parallel computer (earlier PC SoT restored).
          ALL v7 prescriptions absorbed as structure of that computer.
          Ready for immediate full-batch implement.
```

**Regression vs product intent = Fence meta without first-wave Avoid + R1 + ProducerStage-safe schedule.**

---

## 0.3 Hard bans (held + v7 + v8)

| Ban | Hold |
|-----|------|
| SoftWait Soft storms | **yes** |
| Tx sticky Wait / ForcePrefix-as-π / canary live / H-OR / `inc` Avoid / morph actuator / writer_validated Bind gate | **yes** |
| Incarnation Occ\|Pcc fork as mode SoT | **yes** |
| `note_fence` without successful Fence verb | **yes** |
| SerialLane = prefer_admit + Spec continue | **yes** |
| `unfinished` counting done writers | **yes** |
| AccessOrdinal HashMap on empty-PE quiet path | **yes** |
| Tx-global certificate from one Bind covering sibling Spec misses | **yes** |
| Celebrating median while fan_out≪OCC or Bind↑∧abort↓ | **yes** |
| P0/P1/P2 staging | **yes** — one coherent computer cut |
| **Bind-on-any-published-Data as default Fence** | **yes** |
| **Ready-edge observe without admit (or refuse without ProducerStage)** | **yes** |
| **Template PE spray `[1,6,10,20]` on fan_out when ordinal absent** | **yes** |
| **SpecFence validate always OCC B0 while certs exist** | **yes** |
| **Gate decide with `dominant_k` alone when live ordinal available** | **yes** |
| **CC-only redesign that leaves schedule as OCC indices + wave graft** | **NEW v8** |
| **Re-enable v6 “defer consumer only” refuse without ProducerStage** | **yes** |

---

## 0.4 What changes vs tip plant (`3376ac4`)

| Item | v6 live @ tip | **v8** |
|------|---------------|--------|
| Primary frame | hybrid OCC↔Mode(a) stitch | **parallel computer** (Stages, ready, steal, pipeline) |
| Empty-PE OCC | hybrid `!has_any_predicted` | **kept** — byte-identical OCC |
| ReadyEdge | observe; refuse off (deadlock) | **ProducerStage + ConsumerEdge**; refuse only if producer runnable/reserved |
| First wave | abort-then-PE | **seed/HotSet/WŜ → edges before doomed Execute**; else one Spec canary then edge |
| Bind | Data∧unfinished=0∧park_ok (common) | **rare**: tip identity == conflicting producer ∧ EV[Fence]<EV[B0]; prefer WaitFor/lane |
| WaitFor | starved vs Bind | **primary pin** when unfinished=1 ∧ executing |
| SerialLane | grant; Ready→occ_unfenced | **exclusive Execute permit**; unfinished head never Spec-continues |
| Ordinal | `note` dead; templates | **live true-\(k\) when PE-on**; templates **forbidden** on fan_out |
| Validate | always OCC B0 | **RS_spec OCC bool; RS_fence covers_all → R1a/R1b at fail-a** |
| HotSet/WŜ | posterior bump | **ReadyEdge insert + PE posterior** (not Wait OR-door) |
| Learning | PE class + vis | **closed loop → edges, PE, R1 coverage** (computer structure) |
| Repair | B0≡aborts | **R1 on certified fail-a; Spec-only → B0 + train true-k** |
| Telemetry | Bind invisible to process | **every successful verb records process + decision_fields** |
| CC role | owns switch / half the story | **annotates edges**; computer owns schedule |

---

## 1. System model

### 1.1 Objects

| Object | Meaning | SoT? |
|--------|---------|------|
| **Block** | txs `0..n-1`; commit = preset order | yes |
| **Access-event \(a\)** | \(a=(t,k,\mathrm{depth},\ell,\mathrm{mode})\) | **yes — primary grain** |
| **Stage** | Execute(t) \| Validate(t) \| Repair(grain) | **yes — computer** |
| **ProducerStage** | runnable Stage for writer \(w\) (Execute/Repair) reserved on index | **yes — deadlock ban** |
| **ReadyEdge** | `(consumer_a \| consumer_t) ← producer_t` on PE/RAW class | **yes — schedule** |
| **EdgeVisibility \(e_{\mathrm{vis}}\)** | writer?, published_Data?, unfinished_**!done**, executing? | **yes** |
| **Gate** | PE\((\ell,k_{\mathrm{true}},\mathrm{morph})\) ∨ independence_certified | **yes** |
| **Mode(a)** | Spec \| Bind \| WaitFor \| SerialLane \| OrderedAdmit | **yes — CC layer verb** |
| **Certificate strip** | rem/CallEntry/first_k **for Fenced prefix only** | Repair coverage |
| **SerialLane token** | mutex on PE access-class | Fence progress |
| **Incarnation** | bookkeeping | not Avoid key |

### 1.2 Makespan law (primary)

```
wall = useful_EVM + idle + repair + meta

SF_v8 ≈ useful_EVM
      + Σ_{a:Spec} (OCC_read_meta ≈ 0 if empty PE; else light ordinal if PE-on)
      + Σ_{a:Fence} (timely_Fence_tax_on_a)          # WaitFor/lane ≫ Bind
      + Σ_Validate (bool Spec-RS; origin check Fenced-RS only)
      + Σ_Repair (R1a/R1b on certified fail-a; else B0)
      + idle(ready_edges, steal, ProducerStage progress, pipeline)

idle_frac ≈ 1 - useful_EVM / (P × wall)
```

**Invariants:**

1. Empty PE ∧ no ReadyEdge ⇒ SF_wall ≡ OCC_wall (± one mode flag).  
2. Correct timely Fence/edge ⇒ Fence tax ≪ avoided B0 **and** sibling Spec keep OCC width.  
3. Miss on Spec-only → B0; miss on Fenced prefix → R1 at fail-\(a\).  
4. One Fenced access must not sticky-cert the Spec remainder of the tx.  
5. Soft = 0.  
6. `unfinished` never counts done writers.  
7. **Bind_count↑ ∧ abort_count↓ falsifier** — if Bind rises and aborts do not fall vs OCC, Bind is tax (14689597).  
8. **Refuse(consumer) ⇒ ProducerStage(w) is runnable or already Done** — no v6 deadlock.  
9. Steal never takes PE-blocked Execute “to look busy”; pipeline Validate of Executed is first-class.

### 1.3 Success metrics

**Primary:** nonempty all-blocks median SF/OCC TPS @8 Soft=0.  
**Bars (ALL required):**  
- nonempty median **> 0.744**  
- quiet median ≈1.0 **and** quiet p10 **≥0.85**  
- **14689597 ≥0.85 @8 N≥3**  
- Soft=0; exclude=0  

**Falsifiers:** Soft>0; `note_fence` without verb; prefer_admit without lane progress; unfinished includes done; AccessOrdinal HashMap on empty-PE; B0≡aborts on fan_out with PE+certs present; Bind↑∧abort↓; template PE on fan_out; median claim without JSON; schedule refuse without producer runnable; CC-only land that leaves ready refuse off.

---

## 2. The computer — Stages, ready-set, steal, pipeline

### 2.1 Task graph

```
Execute(t, inc)  ──publish WS/RS──►  Validate(t, inc)
                      │
                      │ fail ∧ Spec-only RS
                      └──► Repair = B0 reincarnate ──► Execute(t, inc+1)

                      │ fail ∧ ⊆ RS_fence ∧ covers_all
                      └──► Repair = R1a RebindThis / R1b rewind_to fail-a

                      │ fail mixed
                      └──► selective R1 on fenced; B0 residual on Spec
```

Admission edges for later `Execute(t')`: ReadyEdge / SerialLane / OrderedAdmit / Unfenced OCC speculation. **Not** “tx waits” — admission is per access class / edge, then the incarnation is ready or parked on that grain.

### 2.2 Ready set (fixes v6 deadlock)

```
ready =
  { ProducerStage(w) | w has Execute/Repair work }          # ALWAYS progress path
∪ { Execute(t) | status=Ready
               ∧ ∀ ReadyEdge(t←w): w Done ∨ lane_grant(t)
               ∧ admission_ok(t) }
∪ { Validate(t) | status=Executed }                         # pipeline partner
∪ { Repair(g)   | validate_failed ∧ repair_plan(g) }
```

**PE refuse Execute(t):** if ReadyEdge(t←w) and not Done(w) and not lane head → **t not ready**.  
**Deadlock ban:** never refuse t unless ProducerStage(w) is in ready or running or Done. If collaborative index cannot see w, **promote w** (admit_spine / explicit ProducerStage push) — do not spin.  
**Ban:** re-enable v6 `defer consumer only` with `let _ = ready` and producer off index.

### 2.3 Work-stealing

```
steal priority:
  1. local Execute of independent ready (wave-first)     # useful_EVM
  2. Validate of any Executed (pipeline)                 # hide validate latency
  3. ProducerStage progress on PE class                  # unlock refused consumers
  4. Repair B0 / R1 of aborted
  5. serial-lane progress on PE class
  never: SoftWait Soft wake storms
  never: steal PE-blocked Execute "to look busy"
```

Hang-freedom = ProducerStage progress **or** serial-lane **or** Bind race **or** steal from independents.

### 2.4 Execute∥validate pipeline

- After `Execute(t)` publishes, `Validate(t)` is a **different Stage** and **may run on another core immediately**.  
- Spec-RS validate is OCC bool walk — cheap; pipeline hides it.  
- Fenced-RS may R1 without full re-execute when `covers_all`.  
- Do **not** force same worker execute→validate→repair as one OCC `try_validate` museum.

### 2.5 Publish → edge release

On producer publish Data for \(\ell\): wake WaitFor; release ReadyEdges; grant next SerialLane waiter **one** at a time. No fleet SoftWait.

### 2.6 First-wave Avoid (schedule-first)

```
before Execute(t) on fan_out / HotSet star:
  if InterPrior ∨ HotSet ∨ WŜ predicts RAW(t, ℓ, w) with EV win:
       insert ReadyEdge(t←w); ensure ProducerStage(w) runnable
       # t waits in ready-set — NOT mid-read Bind theater
  else:
       allow one Spec canary incarnation; on abort train true-k + edge
```

ESTIMATE observe may insert edges **without** marking PE classes that open Bind spray (v6 ban on ESTIMATE→PE kept).

---

## 3. CC layer — Mode(a) annotates edges (Bind demoted)

### 3.1 `access_vis` (kept from v6 S2)

```
unfinished := sketch.unfinished_writers_before(ℓ,t) filtered by !is_done
           ∪ MV writers_before with !is_done
# FORBIDDEN: pushing last_writer_before when is_done(writer)
published_data := last_data_before.is_some()
writer_executing := writer.is_some_and(is_executing)
tip_is_conflict_producer := writer == predicted_RAW_producer(ℓ,t)
```

### 3.2 `decide` (event-driven, Bind-rare)

```
empty PE ∨ ¬PE(ℓ,k_true)     → Spec
PE ∧ independence_certified ∧ unfinished=0 ∧ ¬intra → Spec (FM9)
PE ∧ unfinished==1 ∧ executing ∧ !quiet_off ∧ (intra ∨ EV_win)
                              → WaitFor(w)           # PRIMARY pin
PE ∧ unfinished>1 ∧ EV_win ∧ executing
                              → SerialLane(earliest !done)
PE ∧ unfinished==0 ∧ published_data ∧ tip_is_conflict_producer ∧ EV_win
                              → Bind                 # RARE; cert after Data
PE ∧ prior_only ∧ ¬intra ∧ EV_fence ≥ EV_B0 → Spec (roi_skip)
else                          → Spec (roi_skip)
```

**EV_win / prior_pe_fire_wins:** fan_out ∧ ¬quiet_off ∧ ¬park_storm ∧ (Data∨executing) ∧ **predicted abort cascade cost > Fence tax**.  
**HotSet/WŜ:** update PE posterior + **ReadyEdge prior** only — never SerialLane OR-door.  
**Forbidden:** Bind when tip is merely "some Data" (v6 Bind tax on 14689597).

### 3.3 Certificate discipline

| Verb success | Cert? |
|--------------|------:|
| Bind after Data confirmed **and** tip_is_conflict_producer | **yes** — strip for that \(a\) |
| WaitFor armed (park) | **yes** |
| SerialLane exclusive grant + **progress** | **yes** when reader Fenced; **no** for admit-only |
| Bind decide then tip mismatch / Data miss | **no cert**; stay Spec |
| Spec | **never** |

`may_resolve` := **`covers_all(fail locations)`** on certificate strips — never sticky tx bool from first Bind.

### 3.4 SerialLane / OrderedAdmit

**SerialLane(ℓ, k_class):**
1. Token held by earliest unfinished producer (or admitted head).  
2. Consumers **blocked in ready-set** until token holder Done/Data **or** WaitFor park of the single executing head.  
3. **Forbidden:** `admit_spine` + `occ_unfenced` while unfinished head live.  
4. After Data: next consumer may Bind (rare) or take token.

**OrderedAdmit (spine morph):** same along longest_rw_chain; steal only off-spine Executes.

---

## 4. Validate + Repair (certs finally consume)

```
Validate(t):
  RS_spec   := locations Mode=Spec
  RS_fence  := locations with certificate strip covering them
  ok_spec   := OCC bool validate(RS_spec)
  ok_fence  := origin/version check(RS_fence)
  if ok_spec ∧ ok_fence → commit progress
  else → Repair(grain):
      if fail ⊆ RS_fence ∧ covers_all → R1a rebind / R1b rewind_to fail-a
      else if fail ⊆ RS_spec only → B0 + PE(true k from ordinal)
      else → selective: R1 on fenced fail; Spec fail locations ESTIMATE + B0 residual
```

**Ban:** SpecFence branch that always calls `validate_occ_kernel` while strips exist.  
**OccKernel quiet path:** when empty PE ∧ no edges, Validate ≡ OCC bool + B0 only (no Vec museum).

---

## 5. Quiet / empty-PE fast path (kept)

```
if !learner.has_any_predicted() ∧ no ReadyEdge:
    # identical to OCC helpers — no detect museum, no HashMap ordinal, no vis
    return occ_read / next_occ_task / validate_occ_kernel
```

When PE becomes nonempty mid-block: enable ordinal+decide **for subsequent PE ℓ only** (not whole-tx sticky).  
`quiet_fence_off` remains: lone abort must not template-spray PE (2179522 protection).

---

## 6. Learning — closed loop as **computer structure**

Learning does **not** OR-bool Fire verbs. It writes structures the computer consumes at three ports: **schedule admit**, **decide**, **validate/repair**.

### 6.1 Ports (mandatory consume)

| Signal | Must consume at |
|--------|-----------------|
| PE true-\(k\) (ordinal.note when PE-on) | decide gate \(k\); ReadyEdge class; SerialLane class; PE train |
| HotSet / WŜ | **ReadyEdge insert + PE posterior** (not Wait OR) |
| abort RAW | PE + ReadyEdge(consumer←producer) + morph EV update |
| unfinished !done / Data / executing | decide + ready |
| independence | Unfence false PE |
| certificate strip | **Validate/Repair covers_all** |
| morph fan_out/spine/quiet | ready policy + lane width + quiet fast path + template ban |
| DecisionField / effect-raw offline | EV priors (not live OR-bool) |

### 6.2 Ordinal law

```
empty PE:          ordinal HashMap ops = 0
PE-on Execute:     k := access_log.note(ℓ)   # THIS access
decide/PE train:   use k_true; dominant_k only as prior mean, never sole gate
fan_out abort:     FORBIDDEN templates [1,6,10,20]; train at observed fail k only
quiet lone abort:  quiet_fence_off — no template spray
```

### 6.3 Operational loop

```
epoch observe:
  on Spec abort: record (ℓ, k_true, consumer, producer); insert ReadyEdge; update PE
  on finalize:   HotSet/WŜ → edge priors for next block / later txs
  on Fence verb: strip + process.record (telemetry must see Bind/WaitFor)

epoch consume:
  schedule: ReadyEdge + ProducerStage          # computer structure
  decide:   k_true + vis + EV (WaitFor/lane ≫ Bind)
  validate: covers_all → R1 else B0

epoch falsify:
  Bind↑ ∧ abort↓ → disable Bind for that class (roi_skip)
  refuse without ProducerStage → bug
  cert without R1 path → bug
```

### 6.4 v6 learned-unused → v8 must consume

HotSet sizes on 14689597 (~20–28), cert strips after Bind, ReadyEdge consumer bits, effect-raw first-cross k≈6 — **all become edges/R1/true-k**, not museum counters.

---

## 7. Morph playbooks (block info → TPS)

| Morph | Structure | Fence / edge | Repair |
|-------|-----------|--------------|--------|
| **fan_out** | refuse satellites until ProducerStage Done; steal independents | WaitFor/SerialLane at **true k** on star; Bind rare | R1 satellites; B0 rare |
| **spine** | OrderedAdmit along chain; steal off-spine | spine ℓ PE; single WaitFor head | R1a tip |
| **quiet** | OCC-identical | none | B0 only if any |
| **mixed** | edges per class | minimal Fence surface | selective R1 |
| **park-prone** | no ESTIMATE fleet; lane tokens | PE only | — |

**14689597 recipe:** edge every consumer of producer 0 (and other stars) at first-cross ℓ with k_true≈6; ProducerStage(0) always runnable; satellites WaitFor/lane; validate R1 on certified reads; Bind only if tip==0's Data for that ℓ. **Target ≥0.85 @8 N≥3.**

**19807137 recipe:** OrderedAdmit + off-spine steal; WaitFor mass must not park without head progress; B0 cut via R1 on spine tip.

---

## 8. Control loop (live target)

```
begin_block:
  seed PE + ReadyEdges from InterBlockPrior / HotSet / WŜ if EV says win
  if empty PE ∧ no edges: quiet_occ_mode=true
  clear cert strips; clear lane tokens; clear ProducerStage table

schedule:                                    # THE COMPUTER
  pop ready Stage (ProducerStage ∪ PE-satisfied Execute ∪ Validate ∪ Repair)
  steal independent only; pipeline Validate of Executed

Execute(t):
  for access a:
    if quiet_occ_mode: Spec OCC; continue
    if PE-on ∧ location_predicted(ℓ):
      k := ordinal.note(ℓ)                 # true-k
      vis := access_vis_corrected(ℓ)
      verb := decide(... tip_is_conflict_producer ...)
      act(verb); cert only on success; process.record(verb)
    else Spec OCC
  publish; release edges; enqueue Validate(t)   # pipeline

Validate / Repair: as §4
```

---

## 9. Module map (target)

```
specfence/
  computer.rs       # ready/steal/pipeline with ProducerStage + PE refuse (no deadlock)
  ready_edge.rs     # edges + producer runnable invariant
  producer_stage.rs # NEW — reserve/progress writers under refuse
  mode.rs / access_policy.rs  # decide Bind-rare; WaitFor/lane primary
  access_vis.rs     # unfinished=!done; tip_is_conflict_producer
  access_log.rs     # ordinal.note when PE-on only
  certificate.rs    # covers_all for R1
  repair.rs         # R1a/R1b/B0 by grain — SpecFence must call
  lane.rs           # exclusive tokens; ban Ready+Spec
  learner.rs        # HotSet/WŜ/abort → edges+PE+EV; no fan_out templates
  executor.rs       # validate split RS_spec/RS_fence; not always validate_occ_kernel
  process.rs        # record every Fence verb
  hotset.rs / prior.rs  # feed ReadyEdge priors
kernel.rs           # debug mirror; not SoT
pevm.rs / scheduler.rs  # wire computer next_task; OCC path zero SF ticks when quiet
vm.rs               # access gate Mode(a); empty-PE → occ_read
```

---

## 10. Explicit design-out of v6 killers

### 10.1 Ready-refuse deadlock

| v6 | v8 |
|----|-----|
| `note_consumer` → refuse Execute(t) until `note_producer_done(w)` | same refuse **only if** ProducerStage(w) reserved |
| producer off collaborative index → workers spin | promote ProducerStage(w) into ready; never refuse orphan |
| plant abandoned refuse (`let _ = ready`) | refuse **on** with ProducerStage invariant tested |

### 10.2 Bind tax 14689597

| v6 | v8 |
|----|-----|
| Bind when Data ∧ unfinished=0 ∧ park_ok | Bind only tip_is_conflict_producer ∧ EV_win |
| WaitFor≪Bind | WaitFor/lane **primary** |
| templates `[1,6,10,20]` + dominant_k gate | live true-\(k\); templates **forbidden** on fan_out |
| validate always B0; cert unused | RS_fence → R1; Bind↑∧abort↓ falsifier disables Bind class |
| first wave Spec → abort → PE | HotSet/WŜ/prior → ReadyEdge **before** doomed Execute |

---

## 11. Implementation posture

- **Status: design SoT ready for immediate full-batch implement.**  
- No P0/P1/P2 — one coherent computer cut.  
- No SoftWait Soft, canary, ForcePrefix π, AEC OR-bool π, mark_pcc resurrection.  
- Do not re-enable v6 “defer consumer only” refuse without ProducerStage.  
- Land brief: `lab/notes/specfence-v8-land-brief.md`.  
- This note does **not** change Rust; implementers follow the land brief.

---

## 12. Success checklist (when implemented)

1. Soft=0, await=0, exclude=0.  
2. Nonempty median SF/OCC **> 0.744** with JSON.  
3. Quiet median ≈1.0 **and** quiet p10 ≥0.85.  
4. **14689597 ≥0.85 @8 N≥3.**  
5. Bind-count on 14689597 **falls** while aborts approach OCC (Bind↑∧abort↓ = fail).  
6. `prefer_admit` without lane progress = 0; SerialLane never Ready+Spec.  
7. `note_fence` count ≤ successful Fence verbs; process.record sees Bind/WaitFor.  
8. B0≪aborts on fan_out when PE+certs present; R1 used on certified fail-a.  
9. Ready refuse only with ProducerStage runnable; no schedule spin deadlock.  
10. Empty-PE path: AccessOrdinalLog HashMap ops = 0; no template PE on quiet lone abort.  
11. Gate \(k\) = live ordinal when PE-on; fan_out templates = 0.  
12. Computer owns schedule; CC only annotates edges (no CC-only partial land).

---

## 13. Essence restated

Fusion = **one parallel EVM computer** (Stages + ready-set + steal + pipeline + ProducerStages) whose wall is `useful_EVM+idle+repair+meta`; Mode(a) is a **CC layer that annotates ReadyEdges** so Fence is **timely at the right place** (schedule-first Avoid / WaitFor pin / rare conflict-tip Bind), not abort-then-Bind-cert museum; learning writes **edges, posteriors, and covers_all** that schedule, decide, and validate all consume; Repair is **fail-a grain**; quiet is **OCC**; Soft forever 0. v6 cleared a median bar and lost fan_out to Bind tax + abandoned refuse; v7 named the prescriptions; **v8 makes the parallel computer the authoritative frame and absorbs every v7 gap for immediate full-batch implement.**
