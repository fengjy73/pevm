# SpecFence complete architecture v9 — PC⊗CC⊗Bayes triple peers (AUTHORITATIVE SoT)

> **SUPERSEDED** by `lab/notes/specfence-complete-architecture-v9.1-cc-pc-bayes.md` (2026-09-14).
> v9.1 keeps the PC⊗CC⊗Bayes peer frame, **raises product bars**, and makes the **call-flow rewrite** + delete/merge/rewire list authoritative.
> Whole-plant audit: `lab/notes/specfence-v9-whole-plant-callflow-audit.md`.
> This v9 note is retained for history; **do not implement from this file**.


**Date:** 2026-09-14 (Asia/Shanghai, UTC+8)  
**Status:** **AUTHORITATIVE design SoT — design only; DO NOT implement Rust yet**  
**Tip at write:** `bb67ff7`  
**Absorbed autopsies (must):**  
- `lab/notes/specfence-v8-waitfor-abort-r1-autopsy.md`  
- `lab/notes/specfence-v8-all-blocks-process-mishandle.md`  
- `lab/notes/specfence-v8-waitfor-r1-codepath.md`  
**Absorbed peer frame:** `lab/notes/specfence-complete-architecture-v8-parallel-computer.md` (PC⊗CC co-equal — **kept**, extended with Bayes as third peer)  
**Absorbed essence:** `lab/notes/specfence-complete-architecture-v7-essence.md` (prescriptions kept; framing corrected)  
**What exists today (Bayes museum):** `crates/pevm/src/specfence/bayes.rs` + `learner.rs` — Beta posteriors / EV OR-bools exist; **v9 makes them first-class shared structure**, not OR-bool museum  
**Land brief:** `lab/notes/specfence-v9-land-brief.md`  
**Supersedes as plant SoT:** v8 PC⊗CC dual SoT (peer frame kept; WaitFor/R1 Resolve protocol + Bayes demotion replaced)  
**π fields KEPT:** Spec=Region; Mode(a) verbs; Soft=**0**; exclude set; Spec=Region meaning  
**Honesty bar:** nonempty median **> 0.744**; quiet median ≈1.0 with p10 **≥0.85**; named fan_out **14689597 ≥0.85 @8 N≥3**; Soft=0; **R1 must be live when certs exist**  
**Honesty now (tip `bb67ff7` Soft=0):** nonempty median **0.728** (bar miss); **14689597 N=3 ≈0.362**; WaitFor **2260** > Bind **1895** aggregate but abort≈OCC; R1a=**4**/R1b=**0**; star **442/473 BIND_AFTER_PRODUCER_DONE**. **No celebration.**

---

## 0. Essence (ONE paragraph)

**SpecFence v9** is a **co-equal triple-frame** plant: a **parallel computer (PC)**, a **concurrency-control plane (CC)**, and a **Bayesian learning plane (Bayes)** that **jointly** author the same ReadyEdges, Mode(a) verbs, and Repair grains — **neither demoted**. PC owns Stages (`Execute` / `Validate` / `Repair`), the first-class **ready-set**, **work-stealing**, the **execute∥validate pipeline**, **ProducerStages**, and the wall law `wall = useful_EVM + idle + repair + meta`. CC owns Detect / Avoid / Resolve — **Mode(a)** (Spec \| Bind \| WaitFor \| SerialLane \| OrderedAdmit), **Fence**, **certs**, and **R1** — as a first-class control plane that **co-owns when stages may enter ready** and **how miss repairs**. Bayes owns **posteriors over RAW edges**, **PE(ℓ,k,morph)**, **EV[Fence tax vs B0]**, **producer-liveness**, and **covers_all probability**, and writes **shared structure** that admit / decide / validate / repair **must consume** (not OR-bool museum). Fusion = peers writing the same ReadyEdges, ProducerStage invariants, Fence verbs, and Repair plans together. Default Spec ≡ OCC. Empty PE ∧ no ReadyEdge ⇒ **byte-identical OCC path**. Nonempty structure ⇒ **schedule-first Avoid** while producer Executing (ProducerStage-safe refuse), **WaitFor as pin without throwing almost-finished work** when `depth_frac` / EV says hold, Bind **rare**, and Resolve that **does not kill R1** on Spec siblings when the fenced prefix covers the RAW fail set (non-incarnation-strict tip identity / snap). Soft=0 forever. Success = median >0.744 **and** 14689597 ≥0.85 @8 N≥3 **and** quiet p10 ≥0.85 **and** Soft=0 **and** R1 live when certs exist — WaitFor↑∧abort≈OCC, Bind-after-Done default, always-B0 despite covers_all, or Bayes-as-unused-posterior are failures.

---

## 0.1 Triple frame — PC ⊗ CC ⊗ Bayes co-equal (non-negotiable)

```
┌────────────────────────────────────────────────────────────────────────────┐
│ SpecFenceComputer v9  (ONE plant; THREE first-class frames)                 │
│                                                                            │
│  ready = ProducerStages ∪ PE/edge-satisfied Executes                       │
│        ∪ Validates ∪ Repairs                                               │
│  steal = independent Stages only (useful_EVM first)                        │
│  pipeline: Execute(t) publish → Validate(t) on another core                │
│                                                                            │
│  ┌──────────────┐  ┌──────────────────┐  ┌────────────────────────────┐    │
│  │ PC           │  │ CC               │  │ Bayes                      │    │
│  │ Stages       │⊗ │ Detect/Avoid/    │⊗ │ posteriors RAW edges       │    │
│  │ ready/steal  │  │ Resolve          │  │ PE(ℓ,k,morph)              │    │
│  │ pipeline     │  │ Mode(a) verbs    │  │ EV[Fence tax vs B0]        │    │
│  │ ProducerStage│  │ Fence · certs    │  │ producer-liveness          │    │
│  │ wall law     │  │ R1               │  │ P(covers_all)              │    │
│  └──────┬───────┘  └────────┬─────────┘  └────────────┬───────────────┘    │
│         │     FUSION (peers write shared structure)    │                   │
│         └───────────────────┬──────────────────────────┘                   │
│                             ▼                                              │
│           same ReadyEdges · same Mode(a) · same Repair plans               │
│           same ProducerStage invariant · same PE/EV tables                 │
│                             │                                              │
│                             ▼                                              │
│                   shared revm + MvMemory                                   │
└────────────────────────────────────────────────────────────────────────────┘
```

| Question | Co-owners |
|----------|-----------|
| Which Stage runs on which core now? | **PC** (ready / steal / pipeline) constrained by **CC** ReadyEdge / lane / OrderedAdmit; **Bayes** EV / P(RAW) seed edges |
| May Execute(t) enter ready? | **PC ⊗ CC ⊗ Bayes** — ProducerStage (PC) + ReadyEdge / PE admit (CC) + P(RAW)·EV (Bayes) |
| For this access \(a\), Spec or Fence verb? | **CC** Mode(a) **queried with Bayes** posteriors / EV / depth_frac / producer-liveness — feeds PC park / pin / refuse / release |
| Miss → B0 or R1 at fail-\(a\)? | **CC** cert coverage + **Bayes** P(covers_all) / tip identity → **PC** Repair stage plan |
| When to arm PE / insert edge / fire WaitFor vs Bind vs Spec? | **Bayes** posteriors write shared structure; CC/PC consume — **never** OR-bool museum alone |

**Ban:** treating SpecFence as “PC-primary with CC annotation.”  
**Ban:** treating SpecFence as “CC-only Mode(a)” while schedule stays Block-STM indices + wave graft.  
**Ban:** treating Bayes as “OR-bool museum / unused posterior / PolicyCtx feature dump” while decide remains threshold theater.  
All three demotions are rejected. Fusion theater of any kind is rejected.

---

## 0.2 Why v8 failed WaitFor / R1 (absorb autopsy)

```
v8 won:   Soft=0; PC⊗CC peer frame named; WaitFor volume real (N=1 Wait 2260 > Bind 1895);
          empty-PE OCC identity still carries quiet cohort

v8 lost:  WaitFor = Aborting + FullRetry (M1) — throws mid-tx work; Soft=0; often steal-without-park
          covers_all dies on Spec siblings (M2) — per-ℓ strip; mixed invalid → protocol B0
          R1a value_stable incarnation-strict (M3) — writer N→N+1 fails even on same U256
          Quiet-OCC + EV starve early WaitFor (M4) — first RAW still OCC-abort; WaitFor post-abort
          Same-incarnation Blocking retry clears certs (M5) — begin_execute(inc==0) wipe
          Dominant mishandle on 14689597 star: BIND_AFTER_PRODUCER_DONE 442/473
          depth_frac≈0.86 late WaitFor throws almost-finished work
          R1a≈4 total / R1b=0; abort median ratio ≈1.0 vs OCC
          median 0.728 < 0.744; 14689597 N=3 ≈0.362 ≪ 0.85

v9:       TRIPLE FRAME = PC ⊗ CC ⊗ Bayes peers.
          Schedule-first Avoid (ProducerStage-safe) BEFORE satellite Execute.
          WaitFor = pin / hold mid-tx when EV+depth say hold — NOT Aborting throw.
          Resolve: fenced-prefix RAW cover + non-incarnation-strict tip identity/snap → R1 live.
          Bayes writes shared structure for admit/decide/validate/repair — when to WaitFor vs Bind vs Spec.
```

**Regression vs product intent = Fence meta without first-wave schedule Avoid + pin-without-throw WaitFor + R1-live Resolve + Bayes-consumed structure — or any frame demoted.**

---

## 0.3 Hard bans (v8 held + v9 new)

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
| P0/P1/P2 staging | **yes** — one coherent PC⊗CC⊗Bayes cut |
| Bind-on-any-published-Data as default Fence | **yes** |
| Ready-edge observe without admit (or refuse without ProducerStage) | **yes** |
| Template PE spray `[1,6,10,20]` on fan_out when ordinal absent | **yes** |
| SpecFence validate always OCC B0 while certs exist | **yes** |
| Gate decide with `dominant_k` alone when live ordinal available | **yes** |
| CC-only redesign that leaves schedule as OCC indices + wave graft | **yes** |
| PC-primary redesign that demotes CC to “annotates edges only” | **yes** |
| Re-enable v6 “defer consumer only” refuse without ProducerStage | **yes** |
| **Aborting-shaped WaitFor as the only / default Avoid** | **NEW v9** |
| **Bind-after-Done as default satellite path (Avoid too late)** | **NEW v9** |
| **Always B0 despite covers_all / fenced RAW prefix** | **NEW v9** |
| **Bayes as OR-bool museum / unused posterior / threshold-only decide** | **NEW v9** |
| **Throw almost-finished work (high depth_frac) when EV says hold/pin** | **NEW v9** |
| **Incarnation-strict tip identity as sole R1a gate (no snap/value identity)** | **NEW v9** |
| **Quiet-OCC forever owning first RAW wave on known HotSet/prior stars** | **NEW v9** |

---

## 0.4 What changes vs tip plant (`bb67ff7` / v8 SoT)

| Item | v8 live @ tip / SoT | **v9** |
|------|---------------------|--------|
| Primary frame | PC ⊗ CC co-equal | **PC ⊗ CC ⊗ Bayes** three first-class peers |
| Empty-PE OCC | kept | **kept** — byte-identical OCC |
| ReadyEdge / ProducerStage | designed; refuse weak in plant | **schedule-first Avoid** while producer Executing; refuse satellites **before** Execute when Bayes P(RAW)·EV win |
| First wave | abort-then-PE common (M4) | **Bayes prior / HotSet / WŜ → edges before doomed Execute**; one Spec canary only if prior EV fails |
| WaitFor | Aborting + FullRetry (M1) | **PinWithoutThrow** when depth_frac high / EV[hold] > EV[Aborting]; else schedule-refuse (never entered Execute) preferred |
| Bind | Done→Bind flood (442/473) | **rare**; schedule Avoid should have fired earlier; Bind only tip==conflict producer ∧ EV[Bind]<EV[B0] ∧ !bind_tax |
| Decide Fire | OR-bool EV / quiet brakes | **Bayes queries:** PE(ℓ,k,morph), EV[Fence vs B0], P(producer_live), P(covers_all), depth_frac |
| Validate / R1 | covers_all dies on Spec siblings; incarnation-strict | **fenced-prefix RAW cover** → R1 on fenced fail; Spec residuals selective; **tip identity / value snap** non-incarnation-strict for R1a |
| Cert lifecycle | inc==0 wipe (M5) | **strip survives** WaitFor pin / same-tx resume until commit or explicit Repair clear |
| Learning | PE class + vis; BayesMap museum | **closed loop writes shared structure** consumed at admit/decide/validate/repair |
| Soft | 0 | **0** forever |
| Honesty | median 0.728; R1≈0 | bars held; R1 live when certs exist |

---

## 1. System model

### 1.1 Objects

| Object | Meaning | SoT? | Frame |
|--------|---------|------|-------|
| **Block** | txs `0..n-1`; commit = preset order | yes | shared |
| **Access-event \(a\)** | \(a=(t,k,\mathrm{depth},\ell,\mathrm{mode})\) | **yes — primary grain** | CC grain / PC consume / Bayes observe |
| **Stage** | Execute(t) \| Validate(t) \| Repair(grain) | **yes — PC** | PC |
| **ProducerStage** | runnable Stage for writer \(w\) reserved on index | **yes — deadlock ban** | PC (CC/Bayes edges depend) |
| **ReadyEdge** | `(consumer_a \| consumer_t) ← producer_t` on PE/RAW class | **yes — schedule** | **PC ⊗ CC ⊗ Bayes** |
| **EdgeVisibility \(e_{\mathrm{vis}}\)** | writer?, published_Data?, unfinished_**!done**, executing? | **yes** | CC → PC ready; Bayes updates liveness |
| **Gate** | PE\((\ell,k_{\mathrm{true}},\mathrm{morph})\) ∨ independence_certified | **yes** | **Bayes → CC** |
| **Mode(a)** | Spec \| Bind \| WaitFor \| SerialLane \| OrderedAdmit | **yes — CC verb** | CC ← Bayes query → PC park/pin/refuse |
| **Certificate strip** | rem/CallEntry/first_k **for Fenced prefix only** | Repair coverage | CC → PC Repair; Bayes P(covers_all) |
| **SerialLane token** | mutex on PE access-class | Fence progress | CC → PC ready |
| **BayesState** | posteriors / EV / liveness / covers priors | **yes — shared structure** | **Bayes** |
| **Incarnation** | bookkeeping | not Avoid key; not sole R1a key | shared |

### 1.2 Makespan law (PC; CC shapes idle+repair+meta; Bayes shapes Fence surface)

```
wall = useful_EVM + idle + repair + meta

SF_v9 ≈ useful_EVM
      + Σ_{a:Spec} (OCC_read_meta ≈ 0 if empty PE; else light ordinal if PE-on)
      + Σ_{a:Fence} (timely_Fence_tax_on_a)     # schedule-refuse ≫ pin-hold ≫ Aborting WaitFor ≫ Bind
      + Σ_Validate (bool Spec-RS; origin/snap check Fenced-RS)
      + Σ_Repair (R1a/R1b on certified fail-a; else B0)
      + idle(ready_edges, steal, ProducerStage progress, pipeline)
      + meta(Bayes update)                       # must stay ≪ Fence tax; no template spray

idle_frac ≈ 1 - useful_EVM / (P × wall)
```

**Invariants:**

1. Empty PE ∧ no ReadyEdge ⇒ SF_wall ≡ OCC_wall (± one mode flag).  
2. Correct timely Fence/edge ⇒ Fence tax ≪ avoided B0 **and** sibling Spec keep OCC width.  
3. Miss on Spec-only → B0; miss on Fenced RAW prefix → R1 at fail-\(a\) (**R1 live when certs exist**).  
4. One Fenced access must not sticky-cert the Spec remainder of the tx.  
5. Soft = 0.  
6. `unfinished` never counts done writers.  
7. **Bind_count↑ ∧ abort_count↓ falsifier** — Bind tax (14689597).  
8. **Refuse(consumer) ⇒ ProducerStage(w) is runnable or already Done** — no v6 deadlock.  
9. Steal never takes PE-blocked Execute “to look busy”; pipeline Validate of Executed is first-class.  
10. **Neither frame demoted** — ReadyEdge / Mode(a) / Repair / PE-EV are jointly authored.  
11. **WaitFor↑ ∧ abort≈OCC falsifier** — Aborting-shaped WaitFor without R1/abort cut is tax.  
12. **Bayes posterior unused at admit/decide/validate falsifier** — museum ≠ learning.  
13. **depth_frac high ∧ Aborting throw when EV[hold] wins** — falsifier (throws almost-finished work).

### 1.3 Success metrics

**Primary:** nonempty all-blocks median SF/OCC TPS @8 Soft=0.  
**Bars (ALL required):**  
- nonempty median **> 0.744**  
- quiet median ≈1.0 **and** quiet p10 **≥0.85**  
- **14689597 ≥0.85 @8 N≥3**  
- Soft=0; exclude=0  
- **R1a+R1b > 0 on fan_out when PE+certs present** (certs must convert)

**Falsifiers:** Soft>0; `note_fence` without verb; prefer_admit without lane progress; unfinished includes done; AccessOrdinal HashMap on empty-PE; B0≡aborts on fan_out with PE+certs present; Bind↑∧abort↓; template PE on fan_out; median claim without JSON; schedule refuse without producer runnable; CC-only / PC-primary / **Bayes-museum** land; Aborting WaitFor as sole Avoid; Bind-after-Done dominant on star; WaitFor↑∧abort≈OCC; R1=0 while strips exist.

---

## 2. PC frame — Stages, ready-set, steal, pipeline

### 2.1 Task graph

```
Execute(t, inc)  ──publish WS/RS──►  Validate(t, inc)
                      │
                      │ fail ∧ Spec-only RS
                      └──► Repair = B0 reincarnate ──► Execute(t, inc+1)

                      │ fail ∧ ⊆ RS_fence_RAW ∧ covers_all (or fenced-prefix RAW cover)
                      └──► Repair = R1a RebindThis / R1b rewind_to fail-a

                      │ fail mixed
                      └──► selective R1 on fenced RAW; B0 residual on uncovered Spec
```

Admission edges for later `Execute(t')`: ReadyEdge / SerialLane / OrderedAdmit / Unfenced OCC speculation — **authored with CC⊗Bayes**. **Not** “tx waits” — admission is per access class / edge.

### 2.2 Ready set (ProducerStage-safe; schedule-first Avoid)

```
ready =
  { ProducerStage(w) | w has Execute/Repair work }          # ALWAYS progress path
∪ { Execute(t) | status=Ready
               ∧ ∀ ReadyEdge(t←w): w Done ∨ lane_grant(t)
               ∧ admission_ok(t)                            # Bayes EV may keep edge
               ∧ ¬schedule_hold(t) }
∪ { Validate(t) | status=Executed }                         # pipeline partner
∪ { Repair(g)   | validate_failed ∧ repair_plan(g) }
∪ { PinHold(t)  | WaitFor pin without Aborting throw }      # PC Stage: keep interpreter
```

**PE refuse Execute(t):** if ReadyEdge(t←w) and not Done(w) and not lane head → **t not ready**.  
**Schedule-first Avoid (v9):** when Bayes `P(RAW(t,ℓ,w)) · EV_win` before Execute(t), insert ReadyEdge and **keep t out of ready** while ProducerStage(w) Executing — satellites never reach mid-tx Aborting WaitFor for that RAW.  
**Deadlock ban:** never refuse t unless ProducerStage(w) is in ready or running or Done. If collaborative index cannot see w, **promote w**.  
**Ban:** re-enable v6 `defer consumer only` with producer off index.  
**Ban:** letting 448 satellites Execute and Bind-after-Done (442/473 class).

### 2.3 Work-stealing

```
steal priority:
  1. local Execute of independent ready (wave-first)     # useful_EVM
  2. Validate of any Executed (pipeline)                 # hide validate latency
  3. ProducerStage progress on PE class                  # unlock refused consumers
  4. PinHold progress / wake drain (not FullRetry storm)
  5. Repair B0 / R1 of aborted
  6. serial-lane progress on PE class
  never: SoftWait Soft wake storms
  never: steal PE-blocked Execute "to look busy"
  never: steal-without-park that converts PinHold into Aborting FullRetry by default
```

Hang-freedom = ProducerStage progress **or** serial-lane **or** Bind race **or** steal from independents **or** PinHold wake.

### 2.4 Execute∥validate pipeline

- After `Execute(t)` publishes, `Validate(t)` is a **different Stage** and **may run on another core immediately**.  
- Spec-RS validate is OCC bool walk — cheap; pipeline hides it.  
- Fenced-RS may R1 without full re-execute when covers_all / fenced-prefix RAW cover + tip identity/snap.  
- Do **not** force same worker execute→validate→repair as one OCC `try_validate` museum.

### 2.5 Publish → edge release

On producer publish Data for \(\ell\): wake WaitFor / PinHold; release ReadyEdges; grant next SerialLane waiter **one** at a time. No fleet SoftWait. Bayes updates producer-liveness → Done.

### 2.6 First-wave Avoid (schedule + Detect/Avoid + Bayes joint)

```
before Execute(t) on fan_out / HotSet star:
  q := Bayes.P_RAW(t, ℓ, w) · EV[schedule_refuse vs Spec canary]
  if InterPrior ∨ HotSet ∨ WŜ ∨ q ≥ τ_admit:
       insert ReadyEdge(t←w); ensure ProducerStage(w) runnable
       # t waits in ready-set — NOT mid-read Bind theater, NOT Aborting WaitFor
  else:
       allow one Spec canary incarnation; on abort train true-k + edge + Bayes update
```

ESTIMATE observe may insert edges **without** marking PE classes that open Bind spray (v6 ban on ESTIMATE→PE kept).

---

## 3. CC frame — Mode(a) / Fence / certs / R1 (first-class control plane)

CC is **not** an annotation layer. It **co-owns** ready membership and Repair grain. Bind remains demoted relative to schedule-refuse / WaitFor-pin / lane; **the CC frame itself is not demoted**. Decide **queries Bayes** — does not replace Bayes with OR-bools.

### 3.1 `access_vis` (kept from v6 S2; + Bayes liveness)

```
unfinished := sketch.unfinished_writers_before(ℓ,t) filtered by !is_done
           ∪ MV writers_before with !is_done
# FORBIDDEN: pushing last_writer_before when is_done(writer)
published_data := last_data_before.is_some()
writer_executing := writer.is_some_and(is_executing)
tip_is_conflict_producer := writer == predicted_RAW_producer(ℓ,t)   # Bayes posterior argmax OK
producer_liveness := Bayes.P(Executing|Ready|Done) for tip         # NEW consume
```

### 3.2 `decide` (event-driven; Bayes-queried; Bind-rare; pin-aware)

```
empty PE ∨ ¬PE(ℓ,k_true)     → Spec
PE ∧ independence_certified ∧ unfinished=0 ∧ ¬intra → Spec (FM9)

# PRIMARY Avoid paths (in order of preference):
# (0) already schedule-refused via ReadyEdge — decide never sees doomed mid-tx
# (1) WaitFor PinWithoutThrow when unfinished==1 ∧ executing
#       ∧ !quiet_off ∧ (intra ∨ EV_win)
#       ∧ (depth_frac ≥ τ_depth ∨ EV[hold] ≥ EV[AbortingThrow])
# (2) SerialLane when unfinished>1 ∧ EV_win ∧ executing
# (3) rare Bind when unfinished==0 ∧ published_data
#       ∧ tip_is_conflict_producer ∧ EV[Bind]<EV[B0] ∧ !bind_tax_losing

PE ∧ unfinished==1 ∧ executing ∧ Bayes.fire_waitfor(...)
                              → WaitFor(w)           # pin; see §3.5
PE ∧ unfinished>1 ∧ Bayes.fire_lane(...)
                              → SerialLane(earliest !done)
PE ∧ unfinished==0 ∧ published_data ∧ tip_is_conflict_producer ∧ Bayes.fire_bind(...)
                              → Bind                 # RARE
PE ∧ prior_only ∧ ¬intra ∧ EV_fence ≥ EV_B0 → Spec (roi_skip)
else                          → Spec (roi_skip)
```

**Forbidden:** Bind when tip is merely "some Data".  
**Forbidden:** WaitFor that always `Err(Blocking)` → Aborting + FullRetry as the **only** Avoid (v8 M1).  
**Forbidden:** Done→Bind fallthrough as the dominant satellite path (442/473).  
**HotSet/WŜ:** update PE posterior + **ReadyEdge prior** — never SerialLane OR-door.

### 3.3 Certificate discipline

| Verb success | Cert? |
|--------------|------:|
| Bind after Data confirmed **and** tip_is_conflict_producer | **yes** — strip for that \(a\) |
| WaitFor PinWithoutThrow armed (park/hold) | **yes** |
| WaitFor schedule-refuse (never entered) | **edge only**; cert at successful Fence read after release |
| SerialLane exclusive grant + **progress** | **yes** when reader Fenced; **no** for admit-only |
| Bind decide then tip mismatch / Data miss | **no cert**; stay Spec |
| Spec | **never** |

`may_resolve` := **`covers_all(fail locations)`** OR **fenced-prefix RAW cover** (fail ℓ ⊆ certified RAW set for this conflict class) — never sticky tx bool from first Bind.  
**Strip survival:** PinHold / same-tx resume **keeps** location strips until commit or Repair clear — **ban** `begin_execute(inc==0)` wipe after WaitFor success (M5).

### 3.4 SerialLane / OrderedAdmit

**SerialLane(ℓ, k_class):**
1. Token held by earliest unfinished producer (or admitted head).  
2. Consumers **blocked in ready-set** until token holder Done/Data **or** WaitFor pin of the single executing head.  
3. **Forbidden:** `admit_spine` + `occ_unfenced` while unfinished head live.  
4. After Data: next consumer may Bind (rare) or take token.

**OrderedAdmit (spine morph):** same along longest_rw_chain; steal only off-spine Executes.

### 3.5 WaitFor semantics (v9 redesign — absorb M1 / depth_frac)

```
WaitFor(w) outcomes (CC → PC):

A. ScheduleRefuse (preferred when edge exists before Execute):
     Execute(t) never started for the RAW access; no Aborting; no throw.
     ProducerStage(w) runs; on Data → release edge → consumer ready.

B. PinWithoutThrow (when already mid-Execute and depth_frac high / EV hold):
     Park interpreter at access a WITHOUT marking Aborting.
     Keep PC suffix / call stack / rem checkpoint.
     Cert strip for ℓ armed.
     On wake: resume at a (or SuffixRepair) — NOT FullRetry from tx head by default.
     Steal-without-park MUST NOT be the default for PinWithoutThrow.

C. AbortingThrow (last resort only):
     Only when EV[AbortingThrow] > EV[hold] AND depth_frac low AND no schedule edge.
     Counts as tax if common; falsifier if dominant on fan_out.
```

**Ban:** SoftWait Soft storms (still Soft=0) — PinWithoutThrow is **hard pin of one producer**, not Soft fleet.  
**Ban:** `pcc_armed` never set + post-wake always `occ_unfenced` when cert exists — post-wake fenced ℓ must be Fence-consumed for validate R1 eligibility.

---

## 4. Validate + Repair (certs finally convert; R1 live)

```
Validate(t):
  RS_spec   := locations Mode=Spec
  RS_fence  := locations with certificate strip covering them
  ok_spec   := OCC bool validate(RS_spec)
  ok_fence  := tip_identity_or_snap(RS_fence)   # NON-incarnation-strict (v9)
  if ok_spec ∧ ok_fence → commit progress
  else → Repair(grain):
      invalid := collect_invalid_reads
      fenced_raw := { ℓ ∈ invalid | covers(t,ℓ) ∧ Bayes/CC RAW class for this conflict }
      if invalid ⊆ RS_fence ∧ covers_all → R1a rebind / R1b rewind_to fail-a
      else if fenced_raw nonempty ∧ fenced_raw covers the RAW fail set
           → R1 on fenced_raw; Spec residuals → ESTIMATE + selective B0
           # Spec siblings do NOT kill whole R1 if fenced prefix covers RAW (absorb M2)
      else if fail ⊆ RS_spec only → B0 + PE(true k from ordinal) + Bayes update
      else → selective R1 on covered; B0 residual
```

**R1a tip identity (absorb M3):**  
- Prefer `prior_read_value_stable` when incarnation matches.  
- Else **value identity / snap**: same U256 (or declared snap) from predicted conflict producer tip **without** requiring incarnation equality.  
- Museum `try_validate` identity/snap fallbacks become **first-class** in SpecFence validate — not optional debug.

**Ban:** SpecFence branch that always calls `validate_occ_kernel` while strips exist.  
**Ban:** always B0 despite covers_all / fenced RAW prefix.  
**OccKernel quiet path:** when empty PE ∧ no edges, Validate ≡ OCC bool + B0 only.

---

## 5. Quiet / empty-PE fast path (kept; first-wave exception for known stars)

```
if !learner.has_any_predicted() ∧ no ReadyEdge ∧ Bayes.cold(block):
    # identical to OCC helpers — no detect museum, no HashMap ordinal, no vis
    return occ_read / next_occ_task / validate_occ_kernel
```

When InterPrior / HotSet / WŜ / Bayes carry hot RAW for this block: **seed edges before first Execute** even if intra PE empty — quiet_fence_off must **not** forever own first RAW wave on known stars (absorb M4).  
Lone abort on true quiet morph: still `quiet_fence_off` — no template spray (2179522 protection).

---

## 6. Bayes frame — first-class learning plane (NEW peer)

Bayes is **not** a PolicyCtx feature dump and **not** an OR-bool museum. It maintains posteriors and **writes shared structure** that PC admit, CC decide, and PC⊗CC validate/repair **must query**.

### 6.1 What exists today (honest)

`bayes.rs` `BayesMap`: Beta-Bernoulli per location / account; `P_conflict`; `P_bind_useful`; decay; cold-start account fallback; Wait-vs-Spec τ thresholds; metrics means.  
`learner.rs` `LiveLearner`: morph weights; `quiet_fence_off`; `prior_pe_fire_wins`; `pcc_makespan_win`; `bind_tax_losing`; HotSet/WŜ posterior bump; dominant_k / any-k arm; InterBlockPrior.  

**Gap:** posteriors mostly feed OR-bools / τ gates; do **not** jointly author ReadyEdges, PinWithoutThrow vs Aborting, Bind rarity under EV[Fence vs B0], or P(covers_all) for R1. v9 elevates these to **shared structure ports**.

### 6.2 Priors (inter-block + morph)

| Prior | Meaning | Seeds |
|-------|---------|-------|
| \(P_0(\mathrm{RAW}(\ell, t\!\leftarrow\!w))\) | edge prior from InterBlockPrior / HotSet / WŜ / effect-raw | ReadyEdge insert |
| \(P_0(\mathrm{PE}(\ell,k,\mathrm{morph}))\) | access-class fire prior; **true-k** when known; **any-k** only if unknown — **no** `[1,6,10,20]` spray | Gate / decide |
| \(P_0(\mathrm{producer\_live}(w))\) | Executing vs Done race | WaitFor vs Bind timing |
| \(P_0(\mathrm{covers\_all} \mid \mathrm{cert}, \mathrm{RS})\) | Resolve success prior | R1 vs B0 EV |
| \(\mathrm{EV}_0[\mathrm{Fence\ tax}]\) vs \(\mathrm{EV}_0[\mathrm{B0}]\) | cost-aware Fire | decide / admit |
| Morph mix (fan_out / spine / quiet / mixed) | playbook weights | ready policy + template ban |

### 6.3 Likelihoods / updates (on events)

| Event | Update |
|-------|--------|
| Spec abort RAW \((t,\ell,w,k)\) | ↑ \(P(\mathrm{RAW})\); ↑ PE\((\ell,k_{\mathrm{true}},\mathrm{morph})\); insert ReadyEdge; ↑ EV[B0] for class |
| Fence WaitFor pin success + validate ok | ↑ \(P(\mathrm{covers\_all})\); ↑ EV[Fence win]; observe_ok on conflict posterior |
| Fence WaitFor AbortingThrow + still B0 | ↑ tax; may ↓ fire_waitfor for Aborting shape; train schedule-refuse instead |
| Bind success + abort↓ | mild ↑ bind_useful; else **bind_tax_losing** → roi_skip Bind class |
| Bind-after-Done dominant | ↑ “Avoid too late” — strengthen schedule-first / producer-liveness prior |
| Data publish / Done | update producer-liveness → Done; release edges |
| Validate R1a/R1b success | ↑ covers_all posterior; keep strip policy |
| Validate B0 despite strip | ↓ covers_all; diagnose Spec-sibling vs incarnation vs wipe; train Resolve fix class |
| Independence certified | ↓ PE; Unfence false PE |
| Quiet lone abort | **no** template spray; quiet_fence_off holds |

### 6.4 Queries (mandatory consume ports)

| Query | Consumed at | Returns |
|-------|-------------|---------|
| `P_RAW(t,ℓ,w)` / HotSet | **PC admit** + edge insert | schedule-refuse vs Spec canary |
| `PE(ℓ,k_true,morph)` | **CC decide** gate | fire or Spec |
| `EV[schedule_refuse]` / `EV[PinHold]` / `EV[AbortingWait]` / `EV[Bind]` / `EV[B0]` | **CC decide** + **PC admit** | verb + shape |
| `P(producer_executing)` | decide WaitFor vs Bind-after-Done | prefer pin / refuse while live |
| `depth_frac(a)` + `EV[hold]` | WaitFor shape | PinWithoutThrow vs AbortingThrow |
| `P(covers_all ∣ strips, invalid)` | **Validate/Repair** | R1 vs B0 |
| `bind_tax_losing(class)` | decide | roi_skip Bind |
| `quiet_cold(block)` | plant OCC retreat | empty-PE OCC |

**Ban:** `should_wait_hard` / τ-only OR-bool as SpecFence π SoT.  
**Ban:** posterior bump without ReadyEdge / PE / EV consume.  
**Ban:** template spray when ordinal absent on fan_out — prior PE uses **true-k** or **any-k location arm**, never `[1,6,10,20]`.

### 6.5 Cost-aware Fire (Bayes)

```
fire_waitfor := PE ∧ P(producer_executing) ∧ EV[PinHold or schedule_refuse] < EV[B0]
                ∧ !quiet_cold ∧ (intra ∨ prior_pe_fire_wins)
fire_bind    := PE ∧ unfinished==0 ∧ tip_is_conflict_producer
                ∧ EV[Bind] < EV[B0] ∧ !bind_tax_losing ∧ P(bind_useful) ≥ τ_b
fire_lane    := PE ∧ unfinished>1 ∧ EV[lane] < EV[B0 cascade]
roi_skip     := EV[Fence] ≥ EV[B0] ∨ bind_tax_losing ∨ false PE
```

Cost terms include: Aborting FullRetry tax, depth_frac lost work, Bind meta without abort↓, R1 save vs B0, idle from refuse without ProducerStage (must be 0 by invariant).

---

## 7. Fusion control loop (live target)

```
begin_block:
  Bayes.seed from InterBlockPrior / HotSet / WŜ / morph
  insert ReadyEdges where P_RAW · EV_admit wins
  if empty PE ∧ no edges ∧ Bayes.cold: quiet_occ_mode=true
  clear cert strips; clear lane tokens; clear ProducerStage table
  # strip clear is begin_block — NOT begin_execute after WaitFor success

schedule:                                    # PC ⊗ Bayes edges
  pop ready Stage (ProducerStage ∪ PE-satisfied Execute ∪ Validate ∪ Repair ∪ PinHold)
  steal independent only; pipeline Validate of Executed
  never default-steal PinHold into Aborting FullRetry

Execute(t):
  for access a:
    if quiet_occ_mode ∧ !edge(t): Spec OCC; continue
    if ReadyEdge refuses: should not be running — bug
    if PE-on ∨ edge_predicted(ℓ):
      k := ordinal.note(ℓ)                 # true-k when PE-on
      vis := access_vis_corrected(ℓ)
      q := Bayes.queries(vis, k, depth_frac, morph)
      verb := decide(... q ...)            # CC ← Bayes
      act(verb):
        WaitFor → PinWithoutThrow | schedule path | last-resort Aborting
        Bind rare; SerialLane exclusive; Spec OCC
      cert only on success; process.record(verb); Bayes.update(verb outcome)
    else Spec OCC
  publish; Bayes.note_Data; release edges; enqueue Validate(t)

Validate / Repair: as §4                   # CC certs ⊗ Bayes P(covers_all) → PC Repair
  on miss: Bayes.update(abort|R1|B0); train true-k; strengthen edges
```

---

## 8. Learning closed loop (shared structure)

Learning does **not** OR-bool Fire verbs alone. It writes structures **all three frames** consume at four ports: **schedule admit (PC)**, **decide (CC)**, **validate/repair (PC⊗CC)**, **prior seed (Bayes)**.

### 8.1 Ports (mandatory consume)

| Signal | Must consume at |
|--------|-----------------|
| PE true-\(k\) (ordinal.note when PE-on) | decide gate \(k\); ReadyEdge class; SerialLane class; PE train |
| HotSet / WŜ / \(P(\mathrm{RAW})\) | **ReadyEdge insert + PE posterior** (not Wait OR) |
| abort RAW | PE + ReadyEdge + morph EV + Bayes conflict↑ |
| unfinished !done / Data / executing / liveness | decide + ready + WaitFor vs Bind timing |
| independence | Unfence false PE |
| certificate strip + P(covers_all) | **Validate/Repair** → R1 live |
| depth_frac / EV[hold] | WaitFor PinWithoutThrow vs AbortingThrow |
| morph fan_out/spine/quiet | ready policy + lane width + quiet fast path + template ban |
| DecisionField / effect-raw offline | EV priors (not live OR-bool π) |
| Bind↑∧abort↓ / WaitFor↑∧abort≈OCC | falsify class → roi_skip / reshape Avoid |

### 8.2 Ordinal law

```
empty PE:          ordinal HashMap ops = 0
PE-on Execute:     k := access_log.note(ℓ)   # THIS access
decide/PE train:   use k_true; dominant_k only as prior mean, never sole gate
fan_out abort:     FORBIDDEN templates [1,6,10,20]; train at observed fail k only
                   unknown-k → location any-k arm (learner contract kept)
quiet lone abort:  quiet_fence_off — no template spray
```

### 8.3 Operational loop

```
epoch observe:
  on Spec abort: record (ℓ, k_true, consumer, producer); insert ReadyEdge; Bayes↑
  on finalize:   HotSet/WŜ → edge priors for next block / later txs
  on Fence verb: strip + process.record + Bayes update (shape + outcome)
  on Bind-after-Done flood: mark Avoid-too-late; strengthen schedule-first

epoch consume:
  schedule: ReadyEdge + ProducerStage + P_RAW          # PC ⊗ CC ⊗ Bayes
  decide:   k_true + vis + EV + depth_frac + liveness  # WaitFor pin ≫ Aborting ≫ Bind
  validate: covers_all / fenced RAW cover + tip snap → R1 else B0

epoch falsify:
  Bind↑ ∧ abort↓ → disable Bind for that class
  WaitFor↑ ∧ abort≈OCC → Aborting shape tax → prefer schedule-refuse / PinWithoutThrow
  refuse without ProducerStage → bug
  cert without R1 path → bug
  Bayes posterior unused at ports → bug
  PC-primary / CC-only / Bayes-museum land → bug
```

---

## 9. Repair (fail-a grain; R1 live)

| Grain | When | Action |
|-------|------|--------|
| **R1a RebindThis** | fail ⊆ certified ∧ tip identity/snap ok | rebind invalid fenced reads; no full re-exec |
| **R1b SuffixRepair** | fail at fenced prefix; suffix rewind cheap | rewind_to fail-\(a\); keep prefix cert |
| **Selective** | fenced RAW covered; Spec residuals | R1 fenced; B0/ESTIMATE Spec residual |
| **B0** | Spec-only miss or uncovered RAW | reincarnate; train true-k + edge + Bayes |

**14689597:** satellites that were schedule-refused or PinHeld on star ℓ at k≈6 must **R1** on certified reads — R1=0 while strips exist is a plant bug.  
**19807137:** WaitFor mass without head progress is tax; OrderedAdmit + R1 on spine tip.

---

## 10. Morph playbooks (block info → TPS)

| Morph | Structure | Fence / edge | Repair |
|-------|-----------|--------------|--------|
| **fan_out** | refuse satellites until ProducerStage Done; steal independents | schedule-first + WaitFor pin at **true k** on star; Bind rare | R1 satellites; B0 rare |
| **spine** | OrderedAdmit along chain; steal off-spine | spine ℓ PE; single WaitFor/PinHold head | R1a tip |
| **quiet** | OCC-identical | none | B0 only if any |
| **mixed** | edges per class | minimal Fence surface | selective R1 |
| **park-prone** | no ESTIMATE fleet; lane tokens | PE only; PinWithoutThrow not Soft | — |

**14689597 recipe:** Bayes/HotSet edge every consumer of producer **38** (and other stars) at first-cross ℓ with k_true≈6 **before** satellite Execute; ProducerStage(38) always runnable; satellites schedule-refused or PinHeld (not AbortingThrow at depth_frac≈0.86); validate R1 on certified reads with tip snap; Bind only if tip==conflict Data for that ℓ **and** Avoid was not late. **Target ≥0.85 @8 N≥3.**

**19807137 recipe:** OrderedAdmit + off-spine steal; WaitFor mass must not Aborting-park without head progress; B0 cut via R1 on spine tip; Unfenced cold → PE/edge not silent Spec.

---

## 11. Module map (target — design only)

```
specfence/
  computer.rs       # ready/steal/pipeline + ProducerStage + PinHold Stage
  ready_edge.rs     # edges + producer runnable invariant  (PC⊗CC⊗Bayes shared)
  producer_stage.rs # reserve/progress writers under refuse
  mode.rs / access_policy.rs  # decide ← Bayes queries; WaitFor shapes; Bind-rare
  access_vis.rs     # unfinished=!done; tip_is_conflict_producer; liveness hook
  access_log.rs     # ordinal.note when PE-on only
  certificate.rs    # covers_all; strip survival across PinHold; fenced-prefix RAW
  repair.rs         # R1a/R1b/B0 by grain — SpecFence must call; R1 live
  lane.rs           # exclusive tokens; ban Ready+Spec
  bayes.rs          # FIRST-CLASS: posteriors + queries for admit/decide/repair
  learner.rs        # HotSet/WŜ/abort → edges+PE+EV; no fan_out templates; feed Bayes
  executor.rs       # validate split; tip identity/snap; not always validate_occ_kernel
  process.rs        # record every Fence verb + WaitFor shape (pin vs Aborting)
  hotset.rs / prior.rs  # feed ReadyEdge + Bayes priors
kernel.rs           # debug mirror; not SoT
pevm.rs / scheduler.rs  # wire computer; PinHold ≠ default Aborting FullRetry
vm.rs               # access gate Mode(a); PinWithoutThrow path; empty-PE → occ_read
```

---

## 12. Explicit design-out of v8 killers

### 12.1 Aborting WaitFor (M1) + depth_frac throw

| v8 | v9 |
|----|-----|
| `Err(Blocking)` → Aborting → FullRetry | ScheduleRefuse preferred; else PinWithoutThrow keeps work |
| steal-without-park common | PinHold not default-stolen into FullRetry |
| depth_frac≈0.86 throws almost-finished | EV[hold] → pin; AbortingThrow last resort |

### 12.2 covers_all Spec siblings (M2)

| v8 | v9 |
|----|-----|
| mixed invalid → whole B0 | fenced RAW prefix → R1; Spec residual selective |
| sticky-cert ban kept | kept — still no tx-global sticky from one Bind |

### 12.3 incarnation-strict R1a (M3)

| v8 | v9 |
|----|-----|
| `prior_read_value_stable` incarnation match only | tip identity / value snap fallback first-class |

### 12.4 Quiet first wave + Bind-after-Done (M4 / 442/473)

| v8 | v9 |
|----|-----|
| quiet_fence_off / EV starve early WaitFor | Bayes/HotSet seed edges **before** satellite Execute |
| Done→Bind 442/473 | schedule-first Avoid while producer Executing |

### 12.5 Cert wipe inc==0 (M5)

| v8 | v9 |
|----|-----|
| `begin_execute(inc==0)` clears strip after WaitFor | strip survives PinHold / same-tx resume |

### 12.6 Bayes museum

| v8 / tip | v9 |
|----------|-----|
| BetaMap + OR-bool EV | posteriors write ReadyEdges / PE / EV / P(covers_all) / liveness **consumed** at ports |

---

## 13. Implementation posture

- **Status: design only; DO NOT implement Rust.**  
- No P0/P1/P2 — one coherent **PC⊗CC⊗Bayes** cut when land begins.  
- No SoftWait Soft, canary, ForcePrefix π, AEC OR-bool π, mark_pcc resurrection.  
- Do not re-enable v6 “defer consumer only” refuse without ProducerStage.  
- Do not land PC-primary, CC-only, or Bayes-museum partial plants.  
- Land brief: `lab/notes/specfence-v9-land-brief.md`.  
- This note does **not** change Rust; implementers follow the land brief when authorized.

---

## 14. Success checklist (when eventually implemented)

1. Soft=0, await=0, exclude=0.  
2. Nonempty median SF/OCC **> 0.744** with JSON.  
3. Quiet median ≈1.0 **and** quiet p10 ≥0.85.  
4. **14689597 ≥0.85 @8 N≥3.**  
5. Bind-count on 14689597 **falls**; Bind-after-Done not dominant; aborts → OCC or below.  
6. WaitFor↑ must **not** coexist with abort≈OCC as steady state (Aborting shape tax).  
7. R1a+R1b **> 0** on fan_out when PE+certs present; R1b used when SuffixRepair wins EV.  
8. `prefer_admit` without lane progress = 0; SerialLane never Ready+Spec.  
9. `note_fence` count ≤ successful Fence verbs; process.record sees Bind/WaitFor **shape**.  
10. Ready refuse only with ProducerStage runnable; no schedule spin deadlock.  
11. Empty-PE path: AccessOrdinalLog HashMap ops = 0; no template PE on quiet lone abort.  
12. Gate \(k\) = live ordinal when PE-on; fan_out templates = 0.  
13. **PC ⊗ CC ⊗ Bayes co-equal** — ReadyEdges, Mode(a), Repair, PE/EV jointly owned; no frame demoted.  
14. Bayes queries consumed at admit/decide/validate — unused posterior = fail.

---

## 15. Essence restated

Fusion = **three first-class peers** designing the same plant: a **parallel EVM computer** (Stages + ready-set + steal + pipeline + ProducerStages + PinHold; wall = `useful_EVM+idle+repair+meta`), a **concurrency-control plane** (Detect/Avoid/Resolve; Mode(a) Fence verbs; certs; R1), and a **Bayesian learning plane** (posteriors over RAW edges, PE(ℓ,k,morph), EV[Fence vs B0], producer-liveness, P(covers_all)) that **writes shared structure** admit/decide/validate/repair consume. ReadyEdges, ProducerStage invariants, Fence shapes, and Repair grains are **shared structure**, not CC annotations on a PC schedule, not PC scaffolding around Mode(a), and not Bayes OR-bool museum. Fence is **timely at the right place** (schedule-first Avoid / PinWithoutThrow / rare conflict-tip Bind), not Aborting WaitFor theater or Bind-after-Done; Resolve **keeps R1 live** when fenced RAW prefix covers with tip identity/snap; quiet is **OCC**; Soft forever 0. v8 named PC⊗CC peers and raised WaitFor volume but lost abort≈OCC and R1≈0 to Aborting WaitFor, Spec-sibling covers_all death, incarnation-strict R1a, quiet first wave, and Bind-after-Done; **v9 makes PC⊗CC⊗Bayes co-equal the authoritative frame and designs those failures out.**
