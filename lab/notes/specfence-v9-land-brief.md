# SpecFence v9.1 land brief — PC⊗CC⊗Bayes call-order → file/fn (design map only)

**Date:** 2026-09-14 (Asia/Shanghai, UTC+8)  
**SoT (bars + call-flow):** `lab/notes/specfence-complete-architecture-v9.1-cc-pc-bayes.md` (**AUTHORITATIVE for bars + call order**)  
**SoT (module names / DELETE inventory):** `lab/notes/specfence-complete-architecture-v9.2-module-structure.md` (**AUTHORITATIVE for tree names + S0–S2 museum order**)  
**SoT (pevm-unified spine — correction):** `lab/notes/specfence-complete-architecture-v9.3-pevm-unified.md` (**AUTHORITATIVE for one spine; bans dual OCC/SF computers**)  
**Audits:** `lab/notes/specfence-v9-whole-plant-callflow-audit.md`; `lab/notes/specfence-v9.1-code-structure-audit.md`  
**Tip:** `bb67ff7`  
**Posture:** **design only now — DO NOT implement Rust**; single coherent call-order land when authorized.  
**Frame:** SpecFence = **pevm's** PC ⊗ CC ⊗ Bayes plant on **one** parallel executor spine; **Bayes→PC.admit→CC.decide→PinHold/Refuse→Validate/Repair**; quiet/cold = Spec cost class ≡ OCC (**not** `plant_is_occ` retreat).  
**Bars (product — raised):** nonempty median **≥0.95** (stretch ≥0.98); quiet p10 **≥0.90**; mixed **≥0.92**; **14689597 ≥0.90 @8 N≥3** (stretch ≥0.95–1.0); R1 win rate **≥50%** on cert-bearing fan_out fails; Bind-after-Done **<10%**; useful_EVM gated; Soft=0.  
*Retired:* median>0.744 / fan≥0.85 / quiet p10≥0.85 as success.  
**Autopsies:** M1–M5; BIND_AFTER_PRODUCER_DONE 442/473; tip median 0.728 / fan 0.362.

---

## 0. Structural land order (unify spine **then** DELETE/MERGE **before** feature patches)

**SoT:** v9.3 (spine unity) + v9.2 §5 (museum/layer). Feature cuts §1 below are **invalid** if landed into the flat museum tree without **S−1** + S0–S2, or if S0–S2 preserve dual OCC/SF computers.

| Phase | Work | Primary paths | Done when |
|-------|------|---------------|-----------|
| **S−1** | **Unify pevm spine** — one schedule entry, one validate entry, one execute host for `ConcurrencyMode::SpecFence`; ban hybrid `specfence_plant_is_occ` → `next_occ_task` / `validate_occ_stage` retreat as product architecture; quiet/cold = Mode(a)=Spec + Bayes.cold + zero meta on **same** spine (cost-class ≡ OCC), not a rival computer | `pevm.rs` worker, `scheduler.rs`, `vm` gate, `executor::{plant_is_occ,next_occ_task,validate_occ_stage}` product use | Bifurcation absent from SpecFence product path; `Occ` **mode** may keep pure OCC helpers; SpecFence cold ≠ flip to Occ computer |
| **S0** | Quarantine museums + dual π; DELETE `mode.rs`; strip `mod.rs` Iter6–30 SoftWait novel; stop exporting `choose_edge_action` / AEC `choose_action` / `bayes.should_wait_hard` on hot Ctx | `specfence/mod.rs`, `edge`/`resolve`/`bayes`/`mode` → `research/` | **One** decide symbol on hot path; research gated |
| **S1** | MERGE `kernel`→`certificate`; EXTRACT `WavePark`→`pc/wave`; MOVE Fence policy bodies out of `vm::pcc_*` into `cc/fence_act` (shape may still be wrong until cut 3) | `kernel`/`certificate`/`rem`/`vm.rs` | Single cert SoT; SoftWait not parent of WavePark; vm thin host |
| **S2** | Create `pc/` `cc/` `bayes/` `fuse/` (+ `research/`) as **pevm-owned** modules; move live files; thin `mod.rs` | tree per v9.2 §2 under v9.3 ownership | Path ownership enforceable; layers not a parallel computer |
| **Then** | Feature cuts **0–9** below (call-order plant on **unified** spine) | as §1 | v9.1 bars path |

**Ban:** PinHold / R1 / admit_seed / Bayes-port patches **before** S−1 + S0–S2 (= patch salad **or** dual-computer with folders).  
**Ban:** S0–S2 that **keep** `next_occ_task` vs `next_sf_task` hybrid as the quiet story.  
**Note:** Cut **0** (dual π) overlaps S0 — do once as S0; do not re-open museums in later cuts.  
**Note:** Cut **9** (quiet identity) = Spec cost class on unified spine (v9.3), **not** `pevm` hybrid OCC computer switch.

## 1. Cut order (one call-order plant — when land authorized)

| # | Cut | Frame | Primary files | Done when |
|---|-----|-------|---------------|-----------|
| 0 | **Delete/merge dual π** (= **S0**+part **S1**) | hygiene | quarantine `edge::choose_edge_action` π, `resolve::choose_action`, `bayes::{decide,should_wait_hard}`; merge `kernel`→`certificate`; delete `mode.rs`; begin `pc/cc/bayes/fuse/research` tree | hot path has **one** decide; museums not default-compiled |
| 1 | **Bayes→PC.admit_seed** | **Bayes → PC** | `bayes.rs` query ports, `learner`/`hotset`/`prior`, `ready_edge` begin_block seed, `pevm` begin_block | ReadyEdges for known stars **before** satellite Execute; quiet_cold only when truly cold |
| 2 | **ProducerStage + refuse** | **PC ⊗ CC** | `producer_stage.rs`, `computer.rs`, `scheduler.rs` | satellites not ready while ProducerStage(w) Executing; deadlock ban held |
| 3 | **WaitFor PinWithoutThrow** | **CC → PC** | `vm::pcc_wait_for_writer`, `computer` PinHold, `rem` park/resume, `pevm` Blocking arm | high depth_frac → pin **without** Aborting+FullRetry; steal-without-park not default |
| 4 | **Bayes-queried decide** | **CC ← Bayes** | `access_policy::decide`, `bayes` EV/liveness/depth, `access_vis` (no first edge insert) | Bind rare; WaitFor shape from EV; no OR-bool-only π |
| 5 | **Validate → R1 live** | **CC ⊗ Bayes → PC** | `executor::validate_specfence`, `certificate`, `repair`, `mv_memory` tip/snap | fenced RAW prefix + tip snap; R1 win rate path; ban always-`validate_occ_kernel` when strips |
| 6 | **Cert strip survival** | **CC** | `certificate::begin_execute`, PinHold resume | no M5 wipe; begin_block clear only |
| 7 | **SerialLane exclusive** | **CC → PC** | `lane.rs`, `pcc_serial_lane` | ban Ready→`occ_unfenced` on live unfinished head |
| 8 | **Telemetry + falsifiers** | shared | `process.rs`, `metrics.rs` | WaitFor shape; Bind-after-Done share; R1 win rate; useful_EVM |
| 9 | **Quiet = Spec cost class ≡ OCC** | shared | unified `pevm`/`scheduler` spine; Bayes.cold; **no** `plant_is_occ` retreat | cold empty Mode(a)=Spec + zero meta on **same** spine; known-star priors still seed edges |

Ship as **one PR / one coherent plant**. Partial land = non-land.

**Prerequisite:** phases **S−1** then **S0–S2** (§0) complete or included as the opening of that PR — **unify pevm spine, then delete/merge/layer, before** cuts 1–9 feature behavior.

---

## 1b. Layer → cut map (v9.2)

| Layer | Cuts that may edit it after S2 |
|-------|--------------------------------|
| `pc/` | 1 admit_seed, 2 ProducerStage, 3 PinHold Stage, 9 quiet identity (computer gate) |
| `cc/` | 3 fence_act Pin, 4 decide←Bayes, 5 validate/R1, 6 cert survival, 7 SerialLane |
| `bayes/` | 1 seed ports, 4 query ports, 5 covers prior |
| `fuse/` | 8 telemetry falsifiers |
| `research/` | none on hot path (quarantine only) |
| `pevm`/`scheduler`/`vm`/`mv_memory` | **S−1 spine owners**; wiring + unified next/validate; no new π bodies; no hybrid OCC retreat |


---

## 2. SoT § → file/fn map

| SoT topic | Frame | File:fn |
|-----------|-------|---------|
| begin_block admit_seed | Bayes → PC | `pevm` begin_block; `ready_edge` seed API; `bayes`/`hotset`/`prior`/`learner` |
| Ready set + PinHold + steal | PC | `computer.rs`; `scheduler.rs` |
| ProducerStage | PC | `producer_stage.rs` |
| Mode(a) decide ← Bayes | CC ← Bayes | `access_policy.rs::decide` |
| WaitFor PinWithoutThrow | CC → PC | `vm::pcc_wait_for_writer`; PinHold Stage; **ban** default Aborting FullRetry |
| Gate / Bind / SerialLane | CC → PC | `vm::specfence_access_gate`, `pcc_serial_lane` |
| Vis + liveness | CC ← Bayes | `access_vis.rs`; no first ReadyEdge insert here |
| True-k ordinal | CC | `access_log.rs` |
| Cert / covers / survival | CC | `certificate.rs` (+ merged kernel) |
| Repair R1a/R1b/selective/B0 | PC ← CC⊗Bayes | `repair.rs` wired from `validate_specfence` |
| Validate tip snap | PC ⊗ CC | `executor.rs`; `mv_memory` identity/snap |
| Bayes query ports | Bayes | `bayes.rs` (elevate from museum) |
| Process Fence + WaitFor shape | CC | `process.rs` |
| Dual π kill | — | `edge`/`resolve`/`bayes` Boolean APIs / `mode.rs` |

---

## 3. Kill list (tip bugs + framing)

| Bug / framing @ tip | Design fix |
|---------------------|------------|
| WaitFor → Aborting + FullRetry (M1) | PinWithoutThrow / ScheduleRefuse |
| Bind-after-Done 442/473 | admit_seed while producer Executing |
| quiet_fence_off owns first RAW (M4) / dual OCC↔SF computers | **S−1:** one pevm spine; known-star admit_seed on same spine; cold = Spec cost class (v9.3 ban hybrid retreat) |
| covers_all Spec siblings (M2) | fenced RAW selective R1 |
| incarnation-strict R1a (M3) | tip identity / value snap |
| begin_execute(inc==0) wipe (M5) | strip survival |
| always validate_occ_kernel | split validate; R1 live |
| Bayes / edge / AEC dual π | delete/merge; one decide |
| Bars median>0.744 / fan 0.85 | **raised** product bars (§1 SoT) |
| PC-primary / CC-only / Bayes-museum | three peers + call order |

---

## 4. Named-block recipes (implementer smoke — later)

| Block | Expect |
|------:|--------|
| **14689597** | ReadyEdge consumers←**38** @ k≈6 **before** Execute; PinHold/refuse ≫ Bind-after-Done; R1 win ≥50%; SF/OCC **≥0.90** @8 N≥3; Bind-after-Done <10% |
| **19807137** | OrderedAdmit; abort_SF≤OCC; no Aborting WaitFor mass without head progress; ≥0.70 then ratchet |
| quiet / **2179522** | OCC identity; p10 cohort ≥0.90; no template PE |
| Soft | **0** |

---

## 5. Falsifiers

- Soft>0  
- Refuse without ProducerStage runnable  
- Bind↑ ∧ abort↓; WaitFor↑ ∧ abort≈OCC  
- Bind-after-Done ≥10% on star  
- R1 win rate ≪50% while PE+certs present  
- Template PE on fan_out  
- Aborting WaitFor default on high depth_frac  
- Bayes unused at admit/decide/validate  
- Dual π still on hot path  
- Median claim without JSON; B1 without B5  
- PC-primary / CC-only / Bayes-museum / patch-salad land  
- Flat museum tree still the plant shape after land (v9.2 ban)  
- Feature cuts 1–9 without S0–S2  
- Fence Aborting policy body still living in `vm.rs` after S1  
- Dual π re-exported from `mod.rs`  

---

## 6. Out of scope

No SoftWait Soft, canary, ForcePrefix-as-π, Occ\|Pcc incarnation fork, P0/P1/P2 staging, celebrating retired 0.744 bars, implementing Rust until explicitly authorized, PC-primary / CC-only / Bayes-museum partial plants; **feature-first land that leaves flat museums (folders-later-after-features)**.
