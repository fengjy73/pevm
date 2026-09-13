# SpecFence v5 Mode(a) OCC↔PCC / Spec↔Fence — switch-path audit

**Date:** 2026-09-13 (Asia/Shanghai, UTC+8)  
**Tip:** `9a49b5f` (`docs(specfence): post-SoT-rebase sweep honesty vs 0.744`)  
**Plant:** v5 PC⊗CC fusion (`lab/notes/specfence-complete-architecture-v5-pc-cc-fusion.md`)  
**Impl map:** `lab/notes/specfence-v5-pc-cc-fusion-impl.md`  
**Sweep digest:** `lab/notes/v5-fusion-sweep-summary.json`  
**Read-only.** No code rewritten.

**Honesty at this tip:** nonempty median SF/OCC **0.655** (↓ vs parallel-compute **0.744**). Quiet median **1.050** (19/36 ≥1); quiet p10 **0.559**. Named fan_out 14689597 N=1 **0.558** / N=3 **0.454**. Quiet tail 2179522 N=3 **0.751** (do not advertise N=1 1.526). Soft=0.  
Counters (all-blocks N=1): `pcc_fire_at_a` **4 571**, `edge_bind` **4 439**, `edge_wait_for` **132**, `prefer_admit` **16 407**, `pcc_roi_skip` **479**, `unfenced_occ_fast` **326 876**, `occ_kernel_execs` **61 871**, `pcc_kernel_execs` **4 136**, `occ_kernel_validates` **96 978**.

**Prior audit (PC plant @ `4a91b5f`):** `lab/notes/specfence-occ-pcc-switch-path-audit.md` — incarnation `mark_pcc(tx)`, residual \(k{=}1\), `decide` without \(e_{\mathrm{vis}}\). This note re-audits the **live Mode(a)** path after that fork was demoted.

---

## 0. One-paragraph verdict

Mode carrier is no longer `IncarnationKernel::{Occ,Pcc}` / `mark_pcc(tx)`. Live switch is **access-local**: `access_policy::decide(learner, ℓ, k, vis?)` → Spec | Bind | WaitFor | SerialLane; `kernel::note_fence` is a **rem/R1 certificate**, not a tx fork. Timeliness is **at access**, but PE is still mostly **abort-then-reincarnate** (plus inter-block seed that *may* Fence when \(e_{\mathrm{vis}}\) is rich). True-\(k\) training via `AccessOrdinalLog` is real (no residual-1 fallback). The 0.744→0.655 regression is consistent with **more Fence fires + rem museum** (Bind≈4.4k, admit≈16k) under a `decide` that dropped makespan/quiet kill-switches, while `access_vis` often **synthesizes unfinished writers from MV** so SoT “Data ∧ unfinished=0 → Bind” is starved and WaitFor/SerialLane/admit carry the tax. Schedule still does not PE-refuse `Execute(t)`; HotSet/WŜ/independence stay observe-only at the gate.

---

## 1. Mode(a) SM events vs old `mark_pcc`

### 1.1 Old (PC / pre-fusion)

| Event | Effect |
|-------|--------|
| `decide` → `TryPcc` | enter `pcc_overlay` |
| overlay entry | **`mark_pcc(tx)`** even if overlay then Unfenced |
| Bind / WaitFor success | stay PccKernel; rem + Resolve museum |
| `begin_execute` | reset Occ unless repair_armed |
| validate | `is_occ` → `validate_occ_kernel`; `is_pcc` → `try_validate` |

False Fire F1: **kernel bit ≠ Fire verb**.

### 1.2 Live Mode(a) @ `9a49b5f`

| Event | File:fn | Effect |
|-------|---------|--------|
| Incarnation start | `vm.rs::set_tx` → `kernel.begin_execute(t, repair_armed)` | clear Fence cert; Repair-armed keeps prefix cert |
| Empty PE / ¬PE / beneficiary / lazy | `specfence_access_gate` | Spec (`occ_unfenced`); no cert |
| PE ∧ `vis=None` | `decide` | Spec `{predicted, roi_skip}` (quiet Bind-tax guard) |
| PE ∧ unfinished>1 ∨ (lane ∧ unfinished>0) | `decide` → `SerialLane` | `pcc_serial_lane`: `mark_access_class` + `admit_spine`; **cert only if head Executing** (then WaitFor) |
| PE ∧ unfinished==1 ∧ writer_executing | `decide` → `WaitFor` | `note_fence` + `pcc_wait_for_writer` → park or Bind-on-Data |
| PE ∧ published_Data ∧ unfinished==0 | `decide` → `Bind` | `note_fence` + `pcc_bind_published` (or roi_skip Unfence **after** cert if Data race-miss) |
| else PE | Spec `{predicted, roi_skip}` | no cert |
| validate | `pevm.rs::try_validate` | `¬may_resolve` → `validate_occ_kernel`; `may_resolve` → Resolve museum |
| Spec PE publish | `vm.rs` finalize `else` branch | HotSet/WŜ always; Avoid/wake if PE(ℓ, dominant_k) — **not rem-gated** |

**Deleted as live `decide` gates (SoT §3.2 / comments):** `quiet_fence_off`, `pcc_makespan_win`, `park_storm`, HotSet/WŜ as SerialLane OR-doors.  
**Still exist** in `learner.rs` / `pevm.rs` Resolve heuristics — not Mode(a) Fire.

**`occ_kernel_*` / `pcc_kernel_*` metrics** are aliases (spec-only vs fenced/repair incarnations), not a live Occ\|Pcc SoT (`v5-pc-cc-fusion-impl.md`).

### 1.3 SM diagram (live)

```
begin_execute(t, rewind∨ff_head)  ⇒  repair cert? ; Fence cert OFF
access a:
  k := access_log.note(ℓ)                 # always on SF
  empty PE ∨ ¬PE(ℓ,k)                     → Spec (OCC helpers)
  vis := access_vis(ℓ)                    # only after PE hit
  decide → Spec | Bind | WaitFor | SerialLane
  first real Fence verb path              → note_fence (rem/R1 legal)
  SerialLane ∧ ¬executing head            → admit_spine + Spec (no cert)
validate:
  ¬may_resolve                            → OCC bool + B0 + PE(true k)
  may_resolve                             → R1 museum
```

vs old: **no `mark_pcc` on overlay entry**; certificate ≈ Fire (with Bind race-miss caveat §4).

---

## 2. Timeliness: before conflicting access / at access / after abort?

| Moment | Can Mode(a) be Fence? | Notes |
|--------|----------------------|-------|
| Block start | seed PE only | `pevm.rs` `seed_predicted_essential` if `!quiet`; **no verb** until access+\(e_{\mathrm{vis}}\) |
| Before first access of cold class | **no** | empty PE → Spec; first RAW is OCC ESTIMATE/B0 |
| **At access** (after PE exists) | **yes** | Bind / WaitFor / SerialLane from `decide`+\(e_{\mathrm{vis}}\) |
| Mid-read ESTIMATE | Spec stays OCC Blocking | unless already fenced this inc |
| At Spec validate abort | **no verb this inc** | plants PE at **true \(k\)**; B0; next inc may Fence |
| Next inc / sibling | **yes** | prior PE **may** Fence when Data / executing writer (fusion hinge) |

**Summary:** switch is **at access**, not schedule-before-Execute. First conflict of a class is still **after abort** (or after a prior seed that only Fires when visibility is already rich — often too late to avoid the first B0). Repair-armed resume is a **prefix certificate**, not a PE Fire.

---

## 3. True-\(k\) training quality vs residual-1

| Piece | Live | vs PC audit |
|-------|------|-------------|
| Ordinal source | `access_log.rs::note` on every SF `basic`/`storage` gate | **new** — Spec-safe, no rem DashMap |
| Abort PE | `validate_occ_kernel`: `first_k` ∨ rem `first_k` ∨ Edge `min_k`, **`.filter(\|&k\| k > 0)`** — **no `or(Some(1))`** | residual-1 **removed** |
| Unit | `access_log` test `records_true_k_not_residual_one`; learner test `predicted_essential_is_per_access_class_not_tx` | held |
| Class bucket | `access_k_class`: 1..=3→1, 4..=7→2, … | still coarse — true \(k{=}6\) ≠ abort \(k{=}2\) if different buckets |
| Empty PE fast path | `specfence_plant_is_occ` **after** `access_log.note` | k logged even when PE empty |

**Verdict:** training quality is **true-\(k\) first-touch of ℓ**, not residual-1. Remaining miss is **class bucketing + first-wave empty PE**, not missing ordinals.

---

## 4. Where Fence fires but wall loses (tax > benefit)

| # | Case | File:fn | Why tax > benefit |
|---|------|---------|-------------------|
| T1 | WaitFor → immediate Data Bind | `decide` WaitFor → `fence_wait_for` Data-first | Certificates rem; `edge_bind`≪park (`bind` 4439 vs `wait_for` 132). Rem/`try_validate` museum on txs that could have Spec-OCC-read published Data. |
| T2 | Bind decide then Data miss → Unfence | `specfence_access_gate` Bind arm | **`note_fence` before** `last_data_before`; miss → `occ_unfenced` **with cert stuck** → Resolve path without Fire verb (soft F1). |
| T3 | Prior PE + rich vis on quiet-ish blocks | `decide` (no `quiet_fence_off`) | SoT allows prior PE Fence; sweep quiet p10 **0.559**, more fires than PC (3636→4571). Bind/admit meta without fan_out win. |
| T4 | SerialLane Ready head | `pcc_serial_lane` | `prefer_admit` **16 407**: wave churn / ordered-admit **without** rem progress; reader continues Spec and may still B0. |
| T5 | Sticky PE class after one abort | `mark_predicted_essential` | Later independents hitting same \((\ell,k_{\mathrm{class}})\) may Bind tip or WaitFor — false Fire width. |
| T6 | ±k-log + PE probe once any PE seeded | `access_log.note` + `predicted_essential` | Inter-block seed sets `has_any_predicted` → every access pays detect/k/PE even when Spec (`unfenced` 327k). |
| T7 | Fenced finalize rem / CallEntry / wake | finalize `rem_legal` branch | Journal + Avoid broadcast meta vs Spec-only HotSet/WŜ observe. |

---

## 5. Where still Spec when should Fence

| # | Case | File:fn | Why miss |
|---|------|---------|----------|
| S1 | First-wave / empty PE | `has_any_predicted` / ¬PE | First RAW definitionally Spec — dominant fan_out leftover. |
| S2 | **Data published ∧ writer done → Spec** | `access_vis` pushes `last_writer_before` into `unfinished` **without `is_done` filter** | SoT Bind (`Data ∧ unfinished=0`) starved; reader Spec-OCC-reads Data **without** Bind cert (no rem/R1 structure). |
| S3 | Writer Ready / Aborting / Estimate tip | unfinished≥1, ¬executing | Not WaitFor; SerialLane only if >1 or lane; else Spec roi_skip — ESTIMATE storm continues. |
| S4 | PE ∧ empty vis | `decide` `vis=None` → roi_skip | Intentional quiet guard; also blocks first-cross Avoid when caller skips vis gather (gate always gathers after PE). |
| S5 | Beneficiary / lazy | `specfence_access_gate` early | Skip gate. |
| S6 | Writes / WAW at publish | finalize only | `decide` is read-side; WAW not fenced at SSTORE. |
| S7 | `independence_certified` unused at gate | `decide` / `access_vis` | Cannot Unfence false PE or certify independent — hardcoded `false` on DecisionFeat. |
| S8 | HotSet / WŜ not opening Fence | `decide` comments + `hot_is_not_a_serial_lane_door` | By design observe→posterior; also means WŜ cannot pre-Fence a live RAW without PE. |

**S2 is the structural hinge:** `access_vis` makes `unfinished≥1` whenever MV has a lower writer, so live Bind from `decide` is rare; most `edge_bind` counts are WaitFor→Data conversion / residual Bind inside `fence_wait_for`.

---

## 6. Learning produced vs `decide()` consumed

`decide` reads **only**:

```
learner.has_any_predicted
learner.predicted_essential(ℓ, k)
AccessVis { published_data, writer, writer_executing, unfinished,
            in_serial_lane, hot, ws_hat }
```

and **uses** unfinished / lane / executing / published_data / writer.  
**Does not use** `hot` or `ws_hat` (gathered, dead at gate).

| Signal | Producer | Consumed by `decide()`? | Elsewhere on switch? |
|--------|----------|-------------------------|----------------------|
| PE intra (true \(k\)) | `validate_occ_kernel` → `note_abort_access` | **yes** | sketch `mark_access_class` |
| PE prior seed | `pevm.rs` if `!quiet` | **yes** (may Fence w/ vis) | sketch templates |
| PE emptiness | `predicted_n` | **yes** | `specfence_plant_is_occ` |
| Serial-lane token | `sketch.mark_access_class` / seed | **yes** (`in_serial_lane`) | admit only |
| \(e_{\mathrm{vis}}\) unfinished/exec/Data | `access_vis` / sketch spines / MV | **yes** | — |
| HotSet | finalize + Occ abort `note_abort` | **no** (field only) | observe / lean research |
| WŜ / RŜ (`rw_prior`) | finalize `observe_write_set` | **no** (`ws_hat` unused) | `access_vis.hot` OR only |
| `quiet_fence_off` / `pcc_makespan_win` / `park_storm` | learner | **no** (deleted from decide) | `pevm.rs` Resolve heuristics |
| Bayes conflict | abort observe | **no** | promote / legacy |
| `independence_certified` | sketch | **no** | DecisionField observe |
| DecisionField | Bind/WaitFor record | **no** | lab |
| Morph / engagement | begin_block / abort | **no** at Fire | labels / lean |

**Consequence:** learning that moves verbs is **PE class + sketch lane + live MV/spine visibility**. Cross-block prior **can** Fire (fixed vs PC). HotSet/WŜ/Bayes/independence still do **not** move `decide()`.

---

## 7. Schedule vs Fence signals

Plant ready-set: Unfenced ∨ PE producers published ∨ serial-lane token ∪ Validate ∪ Repair.

Live (`executor::next_sf_task` → `scheduler::next_task_with_wave`):

| Plant object | Live | Coupled to Fence/PE? |
|--------------|------|----------------------|
| Ready deque | `WaveParkTable` after park / `admit_spine` | **Partial** — admit from WaitFor/SerialLane mid-access; not PE unpublished-RAW filter |
| Execute-first | if `execution_idx` Ready, take Execute (no fetch_add on miss) | Schedule hygiene; **not** Mode(a) |
| Validation | still when `validation_idx < execution_idx` after Execute miss | Occ validate cheap; stampede reduced not eliminated |
| Steal after park | wave ready + one `execution_idx` fetch_add | After Blocking park only |
| PE refuse Execute(t) | **absent** | Doomed consumers still scheduled |
| FenceGraph SoftWait | Soft=0; wake on Data | Wake useful for Hard Blocking arms |

**Fence is mid-execute park / admit, not schedule-stage admission.** That matches remaining idle + cascade on fan_out even after execute-first.

Double `begin_execute` wipe from the PC audit: **`Vm::execute` no longer calls `begin_execute`** — only `set_tx` does. Metrics `record_occ_kernel_exec` / `pcc_kernel_exec` at execute start key off `repair_armed` only (alias).

---

## 8. Top 8 gaps ranked by TPS impact (0.744→0.655)

Hypothesis ranking for nonempty median / fan_out / quiet tails. Not measured A/B.

| Rank | Gap | TPS mechanism | Explains regression? |
|-----:|-----|---------------|----------------------|
| **1** | **`access_vis` unfinished synthesis vs SoT Bind** (S2) | Data∧done writer → Spec or WaitFor→Bind cert; Bind-from-decide starved; rem museum / miss-timed Fence | **Primary mix shift:** more `edge_bind`/`pcc_fire` without Bind-at-\(a\) clarity; fan_out still Bind/B0 vs OCC reincarnation (14689597 0.558) |
| **2** | **Prior PE may Fence; makespan/quiet removed from `decide`** (T3) | Seeded PE + any executing/Data path Fires earlier/wider than PC | **Direct:** fires 3.6k→4.6k; quiet p10 0.559; median↓ while quiet median≈1.05 |
| **3** | **Certificate then museum** (T1/T2) | `note_fence` ⇒ `may_resolve` ⇒ `try_validate` Vec/rem vs OCC bool; Bind miss leaves cert | **Meta tax** on useful_EVM (`pcc_kernel_execs` 4.1k; `occ_kernel_validates` 97k) |
| **4** | **SerialLane Ready → admit without Fence cert** (T4) | `prefer_admit` 16k wave churn; reader Spec continues; schedule disruption | **Schedule tax** without R1 benefit; park blocks (6196166 N=3 **0.368**) |
| **5** | **First-wave still abort-then-PE** (S1) | First RAW always Spec B0/ESTIMATE | Caps fan_out (still ≪0.85 bar); shared with 0.744 baseline — not full regression alone |
| **6** | **Schedule ignores PE unpublished-RAW** (§7) | Execute-first helps vs validation stampede; still no PE ready-set | Regression amplify via admit/park; discarded broken steal was worse (0.643 / 0.159) |
| **7** | **±k-log / PE probe meta once prior non-empty** (T6) | Every access pays ordinal + PE lookup | Steady meta vs pure OCC; `unfenced`↑ vs PC |
| **8** | **k-class coarseness after true-\(k\)** (§3) | Abort \(k\) bucket ≠ consumer first-cross bucket → still Spec when should Fence | Residual miss on reincarnation; smaller than old residual-1 |

**Not top-8 (held or lower TPS):** Soft=0; journal-less Rebind on Spec (banned); HotSet-as-Wait-OR (correctly unused); Occ publish Avoid for PE ℓ (landed Spec path); double `begin_execute` wipe (fixed).

### Regression story in one line

v5 correctly demoted `mark_pcc` and fixed true-\(k\), then **opened prior-PE Fence** and **admit/SerialLane** while **`access_vis` still mis-counts unfinished**, so the wall paid **more Fence/rem/admit tax** without winning first-wave fan_out — median **0.744→0.655**.

---

## 9. File:fn index (read set)

| Area | Path |
|------|------|
| Mode(a) decide | `crates/pevm/src/specfence/access_policy.rs::decide` |
| Ordinal / true \(k\) | `crates/pevm/src/specfence/access_log.rs` |
| Certificates | `crates/pevm/src/specfence/kernel.rs::note_fence` / `may_resolve` |
| Spec validate + PE train | `crates/pevm/src/specfence/executor.rs::validate_occ_kernel` |
| Gate / vis / SerialLane / Bind / WaitFor | `crates/pevm/src/vm.rs::specfence_access_gate` / `access_vis` / `pcc_serial_lane` / `pcc_bind_*` / `pcc_wait_*` |
| Finalize Observe / Spec PE Avoid | `crates/pevm/src/vm.rs` execute finalize (~2871–3004) |
| Schedule | `crates/pevm/src/scheduler.rs::next_task_with_wave` / `admit_spine` |
| Learning | `crates/pevm/src/specfence/learner.rs` PE / (unused) makespan |
| Prior / HotSet | `crates/pevm/src/specfence/prior.rs`, `hotset.rs` |
| Validate dispatch | `crates/pevm/src/pevm.rs::try_validate` |

---

## 10. Suggested next probes (no impl here)

1. Instrument how often `decide` returns Bind vs WaitFor→Data Bind vs SerialLane→Spec (counter split).  
2. Patch-audit only: `access_vis` unfinished = sketch unfinished ∪ writers with `!is_done` (drop done MV tips) — measure Bind rate and median.  
3. Re-introduce **observe-only** makespan for prior-PE Bind tax on quiet morph — without restoring `mark_pcc`.  
4. PE-gated refuse Execute / stronger SerialLane progress when admit without cert.

