# SpecFence complete architecture v6 — PC⊗CC essence (AUTHORITATIVE SoT)

**Date:** 2026-09-13 (Asia/Shanghai, UTC+8)  
**Status:** **AUTHORITATIVE** complete architecture — **diagnosis + design only; DO NOT implement in this task**  
**Tip at write:** `9a49b5f`  
**Diagnosis:** `lab/notes/specfence-v5-regression-all-blocks-diagnosis.md`  
**Catalog:** `lab/notes/specfence-v5-regression-per-block-catalog.json`  
**Switch audit (must absorb):** `lab/notes/specfence-v5-mode-a-switch-path-audit.md`  
**Supersedes:**  
- `lab/notes/specfence-complete-architecture-v5-pc-cc-fusion.md` — Mode(a) carrier **kept**; control loop / vis / cert / schedule / quiet-meta **replaced**  
- Incarnation Occ\|Pcc fork (already demoted in v5) — stays demoted  
**π fields KEPT:** Spec=Region; \(a\), \(e_{\mathrm{vis}}\), PE∨independence; Soft=**0**; exclude set  
**Honesty bar to beat:** nonempty median **> 0.744**; quiet median ≈1.0 with p10 ≥0.85; named fan_out **14689597 ≥0.85 @8 N≥3**; Soft=0.  
**Honesty now:** digest median **0.655**; this tip Soft=0 N=1 remeasure **0.734** still ≤ PC; fan_out N=3 **0.327**. **No celebration.**

---

## 0. Essence (ONE paragraph)

**SpecFence v6** is one **preset-order parallel EVM computer** whose concurrency control is a **ready-edge graph over Region-accesses**, not mid-read theater on top of Block-STM. Parallel-compute supplies stages `Execute` / `Validate` / `Repair` + steal; concurrency-control supplies **when an access may Spec vs must Fence**, and **what repair grain** a miss pays. Default is Spec ≡ OCC helpers. Fence verbs (Bind / WaitFor / SerialLane / OrderedAdmit) exist only as **edges that gate ready-set membership or pin a single producer**, never as `prefer_admit` without progress and never as `note_fence` without a successful verb. `EdgeVisibility.unfinished` counts **only !done** writers so `Data ∧ unfinished=0 → Bind` is real. PE unpublished-RAW **refuses Execute** of doomed consumers (first-wave Avoid at schedule, not abort-then-reincarnate). Certificates are **access-prefix strips**, not tx-global bits from one Bind. Quiet + empty PE ⇒ **byte-identical OCC path** (no AccessOrdinalLog HashMap, no PE probe). Soft=0 forever. Success = wall↑ vs 0.744 **and** fan_out↑ **and** quiet p10↑ — structure counters without wall are failure.

---

## 0.1 Why v5 fusion regressed (one screen)

```
v5 won:   Mode(a) carrier; true-k AccessOrdinalLog; mark_pcc dead; Soft=0
v5 lost:  access_vis unfinished includes done MV tips → Bind-from-decide starved (S2)
          prior-PE Fence + deleted quiet/makespan gates → fire/admit tax (T3)
          note_fence before Data confirm → cert museum / stuck cert (T1/T2)
          SerialLane Ready → prefer_admit + Spec continue, no cert (T4)
          schedule not PE-gated → first wave still abort-then-PE (S1, §7)
          B0 ≡ aborts; R1 extinct; park ≫ WaitFor

Result:   structure↑ (bind/fire/prefer)  wall↓  (0.744→0.655 honesty)
```

**Regression = more Fence/rem/admit tax without first-wave fan_out win.**

---

## 0.2 Hard bans (held + new)

| Ban | Hold |
|-----|------|
| SoftWait Soft storms | **yes** |
| Tx sticky Wait / ForcePrefix-as-π / canary live / H-OR / `inc` Avoid / morph actuator / writer_validated Bind gate | **yes** |
| Incarnation Occ\|Pcc fork as mode SoT | **yes** |
| `note_fence` / `may_resolve` without successful Fence verb | **NEW — yes** |
| SerialLane = prefer_admit + Spec continue | **NEW — yes** |
| `unfinished` counting done writers | **NEW — yes** |
| AccessOrdinalLog HashMap / PE probe on empty-PE quiet path | **NEW — yes** |
| Tx-global certificate from one Bind covering sibling Spec misses | **NEW — yes** |
| Celebrating fire↑ / abort↓ while median≤0.744 or fan_out≪OCC | **yes** |
| P0/P1/P2 staging in this design | **yes** |

---

## 0.3 What changes vs v5 plant

| Item | v5 live | **v6** |
|------|---------|--------|
| Mode carrier | Mode(a) decide mid-read | **Mode(a) + ReadyEdge(a)** — same verbs, schedule-first |
| `access_vis.unfinished` | MV tip ∪ sketch (includes done) | **only writers with `!is_done`** (+ sketch unfinished filtered) |
| Bind | starved; WaitFor→Data counts as bind | **decide Bind when Data∧unfinished=0**; Bind **after** Data confirm then cert |
| Certificate | `note_fence` early; tx `may_resolve` | **cert per Fenced access-prefix**; Spec-only RS → OCC bool always |
| SerialLane | admit_spine + Spec | **exclusive Execute permit on (ℓ,k_class)**; no Spec progress on lane until head Done/Data |
| Prior PE Fire | always may Fence | **cost-aware:** Fire only if EV[Fence tax] < EV[B0 cascade] (observe makespan; not Soft; not mark_pcc) |
| First wave | abort-then-PE | **PE / cross-block seed → ready-edge refuse Execute(t)** until producer published **or** lane grant |
| Quiet empty PE | detect+access_log.note always | **`plant_is_occ` before note**; zero meta |
| HotSet/WŜ | gathered unused | **feed PE posterior + ready-edge priors**; still not OR-doors for Wait |
| Repair | B0 if ¬may_resolve(tx) | **R1 at first failed Fenced a**; Spec-only fail → B0; mixed → selective |
| Schedule | execute-first Block-STM | **ready = PE-satisfied Executes ∪ Validates ∪ Repairs**; steal independent |

---

## 1. System model

### 1.1 Objects

| Object | Meaning | SoT? |
|--------|---------|------|
| **Block** | txs `0..n-1`; commit = preset order | yes |
| **Access-event \(a\)** | \(a=(t,k,\mathrm{depth},\ell,\mathrm{mode})\) | **yes — primary** |
| **EdgeVisibility \(e_{\mathrm{vis}}\)** | writer?, published_Data?, unfinished_**!done**, executing? | **yes** |
| **ReadyEdge** | `(consumer_a | consumer_t) ← producer_t` on PE class / RAW | **yes — schedule** |
| **Gate** | PE\((\ell,k,\mathrm{morph})\) ∨ independence_certified | **yes** |
| **Mode(a)** | Spec \| Bind \| WaitFor \| SerialLane \| OrderedAdmit | **yes — verb** |
| **Stage** | Execute(t) \| Validate(t) \| Repair(grain) | **yes — computer** |
| **Certificate strip** | rem/CallEntry/first_k **for Fenced prefix only** | Repair |
| **SerialLane token** | mutex on PE access-class | Fence progress |
| **Incarnation** | bookkeeping | not Avoid key |

### 1.2 Cost law

```
wall = useful_EVM + idle + repair + meta

SF_v6 ≈ useful_EVM
      + Σ_{a:Spec} (OCC_read_meta ≈ 0 if empty PE; else detect+k_counter)
      + Σ_{a:Fence} (timely_Fence_tax_on_a)          # only successful verbs
      + Σ_Validate (bool Spec-RS; origin check Fenced-RS only)
      + Σ_Repair (R1a/R1b on certified fail-a; else B0)
      + idle(ready_edges, steal)
```

**Invariants:**

1. Empty PE ∧ no ReadyEdge ⇒ SF_wall ≡ OCC_wall (± one mode flag).  
2. Correct timely Fence/edge ⇒ Fence tax ≪ avoided B0 **and** sibling Spec keep OCC width.  
3. Miss on Spec-only → B0; miss on Fenced prefix → R1 at fail-\(a\).  
4. One Fenced access must not sticky-cert the Spec remainder of the tx.  
5. Soft = 0.  
6. `unfinished` never counts done writers.

### 1.3 Success metrics

**Primary:** nonempty all-blocks median SF/OCC TPS @8 Soft=0.  
**Bars:** **> 0.744**; quiet median ≈1.0 **and** quiet p10 ≥0.85; **14689597 ≥0.85 N≥3**; Soft=0; exclude=0.  
**Falsifiers:** Soft>0; `note_fence` without verb; prefer_admit without lane progress; unfinished includes done; AccessOrdinalLog HashMap on empty-PE; B0≡aborts on fan_out with PE present; median claim without JSON.

---

## 2. One computer — stages + ready-edges

```
                 ┌──────────────────────────────────────────────┐
                 │ SpecFenceComputer v6                         │
                 │  ready = PE/edge-satisfied Executes          │
                 │        ∪ Validates ∪ Repairs                 │
                 │  steal = independent Stages only             │
                 │                                              │
                 │  Execute ──publish──► Validate               │
                 │     │                    │                   │
                 │     │                    ├─ ok → commit      │
                 │     │                    └─ fail → Repair(a) │
                 │     │                          │             │
                 │     └──────── R1 / B0 ◄────────┘             │
                 │                                              │
                 │  Mode(a) on each access; ReadyEdge gates t   │
                 └──────────────────────────────────────────────┘
```

### 2.1 Ready set (completes PC intent)

```
ready =
  { Execute(t) | status=Ready
               ∧ ∀ PE-unpublished-RAW class touching t: lane_grant ∨ producer_Done
               ∧ admission_ok(t) }
∪ { Validate(t) | status=Executed }
∪ { Repair(g)   | validate_failed ∧ repair_plan(g) }
```

**PE refuse Execute(t):** if learner has PE\((\ell,k)\) and \(e_{\mathrm{vis}}\) says unfinished_!done producer exists and t is not lane head → **t not ready** (schedule Avoid). This is the first-wave fix S1/§7 demanded.

**Steal:** only Stages in ready; never steal a PE-blocked Execute "to look busy".

### 2.2 Publish → edge release

On producer publish Data for \(\ell\): wake WaitFor; release ReadyEdges; grant next SerialLane waiter **one** at a time. No fleet SoftWait.

---

## 3. Mode(a) verbs (fixed vis + cert)

### 3.1 `access_vis` (CORRECTED)

```
unfinished := sketch.unfinished_writers_before(ℓ,t) filtered by !is_done
           ∪ MV writers_before with !is_done
# FORBIDDEN: pushing last_writer_before when is_done(writer)
published_data := last_data_before.is_some()
writer_executing := writer.is_some_and(is_executing)
```

### 3.2 `decide` (event-driven, cost-aware prior)

```
empty PE ∨ ¬PE(ℓ,k)     → Spec
PE ∧ unfinished>1       → SerialLane(earliest !done)
PE ∧ unfinished==1 ∧ executing → WaitFor(w)
PE ∧ published_Data ∧ unfinished==0 → Bind   # NOW REACHABLE
PE ∧ prior_only ∧ ¬intra ∧ EV_fence ≥ EV_B0 → Spec (roi_skip)  # cost brake, not quiet Soft
else                    → Spec (roi_skip)
```

HotSet/WŜ: **update PE posterior / ReadyEdge prior only** — never SerialLane OR-door.

Independence_certified: **may Unfence** (Spec) even if PE stale — consume learning FM9 demanded.

### 3.3 Certificate discipline

| Verb success | Cert? |
|--------------|------:|
| Bind after Data confirmed | **yes** — strip for that \(a\) / prefix through \(a\) |
| WaitFor armed (park or Data bind) | **yes** |
| SerialLane exclusive grant + progress | **yes** when reader actually Fenced; **no** for "admit only" |
| Bind decide then Data miss | **no cert**; stay Spec; retry decide |
| Spec | **never** |

`may_resolve` becomes **`has_certificate_strip(t)` covering failed locations**, not a sticky tx bool from first Fence.

---

## 4. SerialLane / OrderedAdmit (progress tokens)

**SerialLane(ℓ, k_class):**

1. Token held by earliest unfinished producer (or admitted head).  
2. Consumers **blocked in ready-set** until token holder Done/Data **or** explicit WaitFor park of the single executing head.  
3. **Forbidden:** `admit_spine` + `occ_unfenced` (v5 T4).  
4. After Data: next consumer may Bind (unfinished=0) or take token.

**OrderedAdmit (spine morph):** same idea along longest_rw_chain; steal only off-spine Executes.

---

## 5. Validate + Repair

```
Validate(t):
  RS_spec   := locations Mode=Spec
  RS_fence  := locations with certificate strip
  ok_spec   := OCC bool validate(RS_spec)
  ok_fence  := origin/version check(RS_fence)
  if ok_spec ∧ ok_fence → commit progress
  else → Repair(grain):
      if fail ⊆ RS_fence ∧ strip exists → R1a rebind / R1b rewind_to fail-a
      else if fail ⊆ RS_spec only → B0 + PE(true k from ordinal counter)
      else → selective: R1 on fenced fail; Spec fail locations ESTIMATE + B0 residual
```

Train PE at **true first-touch k** (lightweight ordinal: **monotonic u32 per tx**, HashMap first_k **only if PE nonempty or learning arm**).

---

## 6. Quiet / empty-PE fast path

```
if !learner.has_any_predicted():
    # identical to OCC helpers — no detect museum, no HashMap ordinal, no vis
    return occ_read
```

When PE becomes nonempty mid-block: enable ordinal+decide for subsequent accesses only.

---

## 7. Learning — produce ⇒ consume map

| Signal | Must consume |
|--------|--------------|
| PE true-k | decide + **ReadyEdge insert** + SerialLane class |
| PE prior | decide **with EV makespan brake** |
| unfinished !done / Data / executing | decide + ready |
| HotSet / WŜ | **PE posterior / edge prior** (not Wait OR) |
| independence | **Unfence** false PE |
| morph fan_out/spine/quiet | ready policy + lane width + quiet fast path |
| DecisionField | offline lab → EV priors (not live OR-bool) |
| AccessOrdinal | PE train; **not** rem |

**Delete as live Fire brakes:** Soft, mark_pcc, park_storm fleet.  
**Keep as EV observe:** makespan Fence vs B0 (T3 fix without Soft).

---

## 8. Morph playbooks (block info → TPS)

| Morph | Structure | Fence / edge | Repair |
|-------|-----------|--------------|--------|
| **fan_out** | refuse consumers of star PE until Data/lane; steal independents | SerialLane/Bind at true \(k\) on star only | R1 satellites; B0 rare |
| **spine** | OrderedAdmit along chain; steal off-spine | spine ℓ PE | R1a tip |
| **quiet** | OCC-identical | none | B0 only if any |
| **mixed** | edges per class | minimal Fence surface | selective R1 |
| **park-prone** | no ESTIMATE fleet; lane tokens | PE only | — |

---

## 9. Control loop (live target)

```
begin_block:
  seed PE + ReadyEdges from InterBlockPrior if EV says win
  if empty PE: quiet_occ_mode=true
  clear cert strips; clear lane tokens

schedule:
  pop ready Stage (PE-satisfied Execute / Validate / Repair); steal independent

Execute(t):
  for access a:
    if quiet_occ_mode: Spec OCC; continue
    k := ordinal.note(ℓ)          # only when PE nonempty
    vis := access_vis_corrected(ℓ)
    verb := decide(...)
    act(verb); cert only on success
  publish; release edges; enqueue Validate(t)

Validate / Repair: as §5
```

---

## 10. Module map (target)

```
specfence/
  computer.rs       # ready/steal with PE refuse
  ready_edge.rs     # NEW — PE/RAW edges
  mode.rs           # decide + cost-aware prior
  access_vis.rs     # unfinished = !done only
  access_log.rs     # ordinal; gated by PE nonempty
  certificate.rs    # per-prefix strips (replace sticky kernel bool)
  repair.rs         # R1a/R1b/B0 by grain
  lane.rs           # SerialLane/OrderedAdmit tokens (progress)
  learner.rs        # PE/HotSet/WŜ → posterior+edges; EV makespan
kernel.rs           # demote to debug mirror; not SoT
```

---

## 11. Implementation non-goals (this note)

- **Do not implement v6 code in this task.**  
- No P0/P1/P2 land plan — one coherent computer cut when coding starts.  
- No SoftWait Soft, canary, ForcePrefix π, AEC OR-bool π, mark_pcc resurrection.

---

## 12. Success checklist (when eventually implemented)

1. Soft=0, await=0, exclude=0.  
2. Nonempty median SF/OCC **> 0.744** with JSON.  
3. Quiet median ≈1.0 **and** quiet p10 ≥0.85.  
4. 14689597 ≥0.85 @8 N≥3.  
5. decide→Bind rate rises when Data∧done; WaitFor→Data no longer sole bind source.  
6. `prefer_admit` without lane progress = 0; SerialLane blocks ready not Spec-continues.  
7. `note_fence` count ≤ successful Fence verbs.  
8. B0≪aborts on fan_out when PE present; R1 used on certified fail-a.  
9. wait_park with edge_wait_for=0 driven down (ESTIMATE stampede fixed by ready-edges).  
10. Empty-PE path: AccessOrdinalLog HashMap ops = 0.

---

## 13. Essence restated

Fusion = **same computer**; mode = **access-local verbs**; switch = **ready-edge + corrected \(e_{\mathrm{vis}}\)** so Fence is **timely at the right place** (before doomed Execute / at Data∧done Bind), not abort-then-rem museum; learning writes **edges and posteriors that the scheduler and decide both consume**; Repair is **fail-a grain**; quiet is **OCC**; Soft forever 0. v5 raised structure and lost wall — v6 may keep Mode(a) names only if the control loop above replaces the plant that regressed 0.744→0.655.
