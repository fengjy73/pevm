# SpecFence v6 — Spec↔Fence / OCC↔PCC switch + Bind-tax audit

**Date:** 2026-09-13 (Asia/Shanghai, UTC+8)  
**Tip:** `3376ac4` (`feat(specfence): live v6 plant — OCC while PE empty, Mode(a) after abort`)  
**Plant SoT:** `lab/notes/specfence-complete-architecture-v6-essence.md`  
**Impl map:** `lab/notes/specfence-v6-essence-impl.md`  
**Sweep digest:** `lab/notes/v6-essence-sweep-summary.json`  
**Prior audits:** v5 Mode(a) `@9a49b5f` (`specfence-v5-mode-a-switch-path-audit.md`); PC `@4a91b5f` (`specfence-occ-pcc-switch-path-audit.md`)  
**Read-only.** No code rewritten. Full v7 learning/architecture SoT is a sibling task — §6 here is brief prescriptions only.

**Honesty at this tip (Soft=0, N=1):** nonempty median SF/OCC **0.795** (bar >0.744 **hit**). Quiet median **1.047**, quiet p10 **0.644**. Named fan_out **14689597 = 0.162** (28.0 vs 4.5 ms; aborts 624 vs 66; bind=535, wait=20). Soft=0. Live all-blocks: `edge_bind` **972**, `edge_wait_for` **1047**.

---

## 0. One-paragraph verdict

Live switch at `3376ac4` is a **hybrid computer**, not an Occ\|Pcc incarnation fork: **empty PE ⇒ OCC schedule/execute/validate**; **`has_any_predicted()` ⇒ Mode(a) mid-read** (`decide` → Spec \| Bind \| WaitFor \| SerialLane) with wave steal. That switch is **timely relative to PE existence** (at access, after abort heat or non-quiet prior seed), but **not timely relative to first conflict** (incarnation‑0 always Spec; first RAW still abort-then-PE). v6 fixed S2 (`unfinished` = `!done` only) and cert-after-success Bind, restored quiet/cost brakes in `decide`, and killed rem-overlay WaitFor — yet **14689597 still dies on Bind tax** because PE spray + `dominant_k` gate + Bind-on-Data certifies without preventing B0 storms (`bind≪aborts` inverted: 535 binds, 624 aborts). Ready-edge **schedule-refuse was deadlocked** (producer off collaborative index) and is **observe-only**; PC leftover is wave park/steal + execute-first. SpecFence validate is **always** `validate_occ_kernel` (R1 / cert strips unused on the main loop). Learning that moves verbs is still mostly **PE class + \(e_{\mathrm{vis}}\) + morph/quiet**; HotSet/WŜ/access_log ordinal remain under-consumed.

---

## 1. Empty-PE OCC retreat vs PE-on Mode(a)

### 1.1 Hybrid carrier (`pevm.rs`)

| PE state | Schedule | Execute | Access | Validate |
|----------|----------|---------|--------|----------|
| `!has_any_predicted()` | `next_occ_task` | `try_execute(..., None, None)` | `specfence_access_is_occ` → `Ok(())` (no detect/vis/decide) | `validate_occ_kernel` (B0 + train PE) |
| `has_any_predicted()` | `next_sf_task` → `next_task_with_wave` (ready **ignored**) | `try_execute` + wave + fence graph | Mode(a) if `inc>0` ∧ `location_predicted(ℓ)` | **still** `validate_occ_kernel` |

Switch predicate is **`learner.has_any_predicted()`** (`predicted_n > 0`), re-checked every task fetch and every Execution arm (`pevm.rs` ~526–550, ~591–597).

### 1.2 When PE becomes nonempty

| Source | File:fn | Arms PE? | Notes |
|--------|---------|----------|-------|
| Inter-block seed | `pevm.rs` begin_block `seed_predicted_essential` if `!quiet` | yes (prior-only) | Quiet morph skips seed |
| Spec validate abort | `executor.rs::validate_occ_kernel` → `note_abort_access` | yes (intra) | True \(k\) if `access_log`/`rem`/`edges` yield \(k>0\); else templates |
| Quiet lone abort | `learner.rs::note_abort_access` | **no** templates if `quiet_fence_off` ∧ cascade<8 | Protects 2179522 |
| ESTIMATE observe | `vm.rs::note_unpublished_raw` | **no** (explicit ban) | Comment: PE from ESTIMATE Bind-theaters 14689597 |

### 1.3 Mode(a) at access (`vm.rs::specfence_access_gate`)

```
incarnation==0 ∨ ¬location_predicted(ℓ)  → Spec OCC (ESTIMATE plant)
else:
  vis := access_vis(ℓ)                   # unfinished = !done only (S2 ✓)
  k   := dominant_k(ℓ).max(1)            # NOT live access ordinal
  decide → Spec | Bind | WaitFor | SerialLane
  Bind: Data confirm THEN note_fence_success; OCC continue (no rem)
  WaitFor(executing): note_fence_success + park Blocking; never pcc_armed
  SerialLane: grant + admit_spine; WaitFor if executing else occ_unfenced
```

`access_policy::decide` (v6):

- empty / ¬PE → Spec  
- PE ∧ independence_certified ∧ unfinished=0 ∧ ¬intra → Spec (FM9)  
- PE ∧ unfinished==1 ∧ executing ∧ `!quiet_off` ∧ (intra ∨ `prior_pe_fire_wins`) → WaitFor  
- PE ∧ (unfinished>1 ∨ lane) ∧ park_ok ∧ executing → SerialLane  
- PE ∧ unfinished==0 ∧ published_data ∧ park_ok → **Bind**  
- else Spec `{roi_skip}`

`quiet_fence_off` = `dominant_quiet ∧ abort_events<4 ∧ park_heat<2`.  
`prior_pe_fire_wins` = ¬quiet_off ∧ ¬park_storm ∧ fan_out ∧ (Data ∨ executing).

### 1.4 Timeliness verdict

| Question | Answer |
|----------|--------|
| Right **mechanism**? | Mostly yes: hybrid OCC retreat (T6) + Mode(a) verbs + cert-after-success is the v6 intent |
| Right **place**? | Access-local yes; **schedule-first PE refuse no** (SoT §2.1 missed) |
| Timely vs first conflict? | **No** — first wave / inc‑0 always Spec; PE after abort (or prior seed that still needs vis+EV) |
| Timely once PE exists? | **Yes at access** for that ℓ; Bind reachable when Data∧unfinished=0 (S2 fixed vs v5) |
| Validate switch? | **Wrong place** — Fence cert does not select R1; always OCC B0 |

**Summary:** switch is **after abort heat (or non-quiet seed), at later access** — correct relative to empty-PE OCC identity, **late** relative to fan_out first-wave Avoid.

---

## 2. Why Bind tax kills 14689597

**Evidence:** `v6-essence-sweep-summary.json` named_n1 — SF/OCC **0.162**, bind=**535**, wait=**20**, ab_sf=**624** vs ab_occ=**66** (~9.5×). Wall 28 ms vs 4.5 ms.

### 2.1 File:fn chain

| Step | File:fn | Mechanism |
|------|---------|-----------|
| 1 | First-wave Spec RAW → B0 | `specfence_access_gate` `tx_incarnation==0`; `validate_occ_kernel` |
| 2 | PE arm (wide) | `note_abort_access`: if no ordinal \(k\), **templates `[1,6,10,20]`** on fan_out / ¬quiet → `has_any_predicted` |
| 3 | Computer flip | `pevm.rs` `has_any_predicted` → wave computer + gate opens for PE ℓ |
| 4 | Gate \(k\) | `dominant_k(ℓ).max(1)` — abort-mean class, **not** this access’s ordinal (`access_log.note` **never called** on plant path) |
| 5 | Bind Fire | `decide` Bind when `unfinished==0 ∧ published_data ∧ park_ok` (fan ∧ ¬quiet ∧ intra) |
| 6 | Cert + OCC continue | `specfence_access_gate` Bind arm: Data check → `note_fence_success` → `Ok(())` (no rem, no pin of later writers) |
| 7 | Still B0 | SpecFence validate **always** `validate_occ_kernel` → full restart; cert/`covers_all` unused |

### 2.2 Why tax > benefit on this block

1. **Bind volume without abort relief:** 535 binds vs 624 aborts — Fence is not converting RAW into WaitFor/R1 progress; it mostly stamps Data tips that OCC would have read anyway, then still B0s.  
2. **PE spray width:** template classes + sticky `predicted_locs` open Mode(a) for many ℓ after a few fan_out aborts; independents pay decide/vis/cert.  
3. **Stale-Data theater (historical + residual):** comments at `note_unpublished_raw` / `pcc_wait_for_writer` document prior rem-skip Bind theater (581 vs 24; 503 vs 113). Live plant removed `pcc_armed`, but **Bind-on-published tip** still certifies a tip that may not be the conflicting producer grain; sibling Spec RS still fails.  
4. **WaitFor starved relative to Bind:** wait=20 ≪ bind=535 — plant parks rarely; most “Fence” is Bind meta, not producer pinning.  
5. **No R1 on certified fail:** even successful Bind sets `kernel.note_fence` / strip, but main loop never routes SpecFence through `try_validate` → rem museum / R1a — so Fence pays cert meta and still pays B0.

**One-liner:** on 14689597, Bind tax = **PE-spray Mode(a) + Bind-on-Data certificates without schedule Avoid or R1**, while abort count stays OCC-catastrophe×9.

---

## 3. Why ready-refuse deadlocked; what’s left of PC schedule

### 3.1 Deadlock

SoT §2.1: PE unpublished-RAW **refuses Execute(t)** for known consumers.  
Landed API: `scheduler::try_execute_ready(..., Some(ready))` + `ReadyEdgeTable::may_execute` / `defer`.

**Live:** `computer.rs::next_sf_task` does `let _ = ready; scheduler.next_task_with_wave(Some(wave))` — i.e. `ready=None`. Commit message / impl map: *schedule-refuse of known consumers deadlocks when the producer is not on the collaborative index (spin in `next_task`)*.

Mechanism of hang:

1. Consumer abort → `note_consumer(t, w)` (`validate_occ_kernel` / WaitFor / ESTIMATE).  
2. Schedule refuses reincarnation of `t` until `note_producer_done(w)`.  
3. If `w` is Aborting / not claimed on `execution_idx` / never reaches Done release, **no thread progresses `w`** while workers spin yield on refused ready — classic collaborative-index hole.

`ready_edge.rs` comment + test: **suffix-global refuse forbidden**; known-consumer refuse still needs a live producer progress path that the Block-STM index does not guarantee.

### 3.2 What’s left of PC schedule

| Piece | Live? | Role |
|-------|------:|------|
| Wave park / steal after WaitFor | **yes** | `WaveParkTable` + `next_task_with_wave` |
| Execute-first (avoid validation stampede) | **yes** | when `wave.is_some()` |
| `admit_spine` from WaitFor/SerialLane | **yes** | mid-access priority nudge |
| ReadyEdge observe (unpublished / consumer / publish / producer_done) | **yes** | wake deferred list **if** defer were used |
| PE refuse Execute / `next_task_with_wave_ready(..., Some(ready))` | **no** | API present, unwired |
| Stage ready-set = PE-satisfied ∪ Validate ∪ Repair | **no** | still Block-STM indices |
| SoftWait Soft | **0** | held |

**Avoid remains the access verb** (WaitFor/Bind/SerialLane), not schedule admission — same structural fan_out leftover as v5 §7, with refuse explicitly abandoned.

---

## 4. Learning produced vs consumed

### 4.1 What `decide()` actually reads

```
has_any_predicted
predicted_essential(ℓ, k)          # k = dominant_k from gate
predicted_essential_intra(ℓ, k)
quiet_fence_off / morph.dominant_fan_out
prior_pe_fire_wins(vis)            # fan ∧ (Data∨executing), ¬quiet, ¬park_storm
AccessVis: unfinished, writer, writer_executing, published_data,
           in_serial_lane, independence_certified
# gathered but not verb OR-doors: hot, ws_hat
```

### 4.2 Produce → consume map (v6 live)

| Signal | Producer | Consumed by `decide`? | Elsewhere on switch? |
|--------|----------|----------------------:|----------------------|
| PE intra (abort) | `validate_occ_kernel` → `note_abort_access` | **yes** | flips hybrid computer |
| PE prior seed | `pevm.rs` if `!quiet` | **yes** via `prior_pe_fire_wins` / intra false | may Fence under fan EV |
| PE emptiness | `predicted_n` | **yes** | `specfence_plant_is_occ` / schedule hybrid |
| True-\(k\) ordinal | `AccessOrdinalLog` | **broken** — `note` never called; `first_k` usually empty | fallback templates / rem / edges |
| Gate \(k\) | `dominant_k` abort mean | **yes (wrong grain)** | class mismatch risk |
| \(e_{\mathrm{vis}}\) !done | `access_vis` + `compose_unfinished` | **yes** | S2 fixed |
| quiet / park_storm / prior EV | learner | **yes** in decide | Resolve heuristics also |
| HotSet | finalize / abort `note_abort` | **no** as verb | `note_hot_ws_posterior` bumps readers only |
| WŜ / `rw_prior` | finalize observe | **no** as verb | feeds `hot`/`ws_hat` → posterior bump |
| independence_certified | sketch | **yes** (Unfence stale prior) | FM9 |
| Certificate strips | `note_fence_success` | **no** on validate path | `covers_all` unused; SpecFence always OCC validate |
| ReadyEdge consumer bits | abort / WaitFor / ESTIMATE | **no** at schedule | observe / Done wake only |
| Lane tokens | `lanes.grant` | partial | SerialLane; Ready head still `occ_unfenced` |
| Bayes / DecisionField / morph engagement | various | **no** at Fire | labels / lab |

**Consequence:** verbs move on **PE class + corrected vis + quiet/fan EV**. Cross-block / HotSet / WŜ / ordinal / ready-edge / cert-grain learning is **produced but not closing the SoT consume loop**.

---

## 5. Top 8 gaps ranked by TPS / fan_out impact

Hypothesis ranking for median hold (0.795) vs fan_out bar (14689597 ≥0.85) and quiet p10 (≥0.85). Not measured A/B.

| Rank | Gap | TPS / fan_out mechanism | Evidence |
|-----:|-----|-------------------------|----------|
| **1** | **Bind tax on fan_out without abort relief** (§2) | Mode(a) Bind meta + PE spray; aborts stay ≫OCC; wall×6 on 14689597 | bind=535, ab=624, SF/OCC **0.162** |
| **2** | **First-wave / inc‑0 always Spec; no PE refuse Execute** (§1/§3) | Abort-then-PE; doomed consumers still scheduled | SoT S1; ready refuse deadlocked/off |
| **3** | **True-\(k\) ordinal dead → template PE / `dominant_k` gate** | Wrong class Fires Bind/Wait; or misses real first-cross | `access_log.note` absent; templates `[1,6,10,20]` |
| **4** | **SpecFence validate always B0 (`validate_occ_kernel`)** | Fence cert never buys R1; every miss = full restart | `pevm.rs` SpecFence branch; checklist §12.8 fail |
| **5** | **SerialLane Ready → admit + `occ_unfenced`** (SoT ban) | Token without exclusive progress; ESTIMATE cascade risk | `pcc_serial_lane` Ready path |
| **6** | **HotSet/WŜ posterior not driving edges/PE** | Learning observe-only; cannot pre-Fence RAW | `note_hot_ws_posterior` = reader bump |
| **7** | **Quiet p10 / prior-PE EV still soft** | Quiet cohort protected at decide, but p10 **0.644**; fan morph over-Fires | digest quiet_p10; T3 partially held |
| **8** | **WaitFor≪Bind; park not pinning producers** | Fence mass is Bind-on-Data not WaitFor; schedule Avoid absent | wait=20 vs bind=535 on killer block |

**Not top-8 (held or lower):** Soft=0; S2 unfinished=!done; cert-before-Data miss (Bind confirms Data first); `mark_pcc` fork; rem-overlay WaitFor (removed).

### Story in one line

v6 **won the empty-PE OCC retreat and median 0.795**, but on fan_out the plant still **aborts first, sprays PE, Binds published tips, never refuses Execute, never R1s** — so **14689597 stays Bind-taxed (~0.16)**.

---

## 6. Prescriptions for v7 learning + architecture (brief)

Full SoT is the sibling task. Live plant constraints from this audit:

1. **Learning loop:** produce ⇒ consume must include **ReadyEdge insert + schedule admit**, not only `decide`. HotSet/WŜ update **PE posterior / edge prior** with a measured EV; independence Unfence stays.  
2. **Ordinal:** restore **lightweight true-\(k\)** only when PE nonempty (or learning arm); gate `decide` with **this access’s \(k\)**, not `dominant_k` alone; kill blind template spray on fan_out or scope templates to abort class only.  
3. **Bind policy:** Bind only when EV[Fence] < EV[B0 cascade] **and** tip is the conflicting producer; prefer **WaitFor/lane pin** on star ℓ; treat Bind-count↑ without abort↓ as failure (14689597 falsifier).  
4. **Schedule:** redesign PE refuse so producer stays runnable (explicit producer Stage / index reservation) — do not re-enable the deadlocked “defer consumer only” path unchanged.  
5. **Validate/Repair:** SpecFence must split RS_spec vs RS_fence; **R1 on strip-covered fail-a**; Spec-only → B0 + PE train. Sticky tx `may_resolve` from one Bind remains forbidden — use `covers_all`.  
6. **SerialLane:** exclusive Execute permit; **forbid** admit+Spec-continue on live unfinished head.  
7. **Quiet:** keep hybrid OCC identity; raise quiet p10 via stricter prior-PE Fire EV (not Soft).  
8. **Success bars (unchanged):** Soft=0; median **>0.744**; quiet p10 **≥0.85**; **14689597 ≥0.85 @8 N≥3**; structure counters without wall = fail.

---

## 7. File:fn index (read set)

| Area | Path |
|------|------|
| Hybrid OCC↔Mode(a) | `crates/pevm/src/pevm.rs` (`has_any_predicted` schedule/execute) |
| Mode(a) decide | `crates/pevm/src/specfence/access_policy.rs::decide` |
| Vis S2 | `crates/pevm/src/specfence/access_vis.rs::compose_unfinished` |
| Gate / Bind / WaitFor / SerialLane / cert | `crates/pevm/src/vm.rs::specfence_access_gate`, `access_vis`, `note_fence_success`, `pcc_wait_for_writer`, `pcc_serial_lane` |
| Ready-edge | `crates/pevm/src/specfence/ready_edge.rs` |
| Computer (refuse off) | `crates/pevm/src/specfence/computer.rs::next_sf_task` |
| Schedule refuse API | `crates/pevm/src/scheduler.rs::try_execute_ready` / `next_task_with_wave_ready` |
| Lane tokens | `crates/pevm/src/specfence/lane.rs` |
| Cert strips | `crates/pevm/src/specfence/certificate.rs` |
| Kernel cert mirror | `crates/pevm/src/specfence/kernel.rs::note_fence` / `may_resolve` |
| OCC validate + PE train | `crates/pevm/src/specfence/executor.rs::validate_occ_kernel` |
| Quiet / prior EV / HotSet posterior | `crates/pevm/src/specfence/learner.rs` |
| HotSet / WŜ prior | `crates/pevm/src/specfence/hotset.rs`, `prior.rs` |

---

## 8. Report for parent

- **Path:** `lab/notes/specfence-v6-switch-and-bind-tax-audit.md`  
- **Top gaps:** (1) Bind tax 14689597, (2) no PE refuse Execute / first-wave abort-then-PE, (3) true-\(k\) ordinal dead → template/`dominant_k`, (4) SpecFence always B0 validate, (5) SerialLane admit+Spec, (6) HotSet/WŜ unused at edges, (7) quiet p10, (8) WaitFor≪Bind.
