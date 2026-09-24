# SpecFence v9 land brief — call-order map + conceptual land order (DESIGN ONLY — **no land now**)

**Date:** 2026-09-14 (Asia/Shanghai, UTC+8)  
**SoT (bars + call-flow):** `lab/notes/specfence-complete-architecture-v9.1-cc-pc-bayes.md` (**AUTHORITATIVE for bars + call order**)  
**SoT (pevm-unified spine):** `lab/notes/specfence-complete-architecture-v9.3-pevm-unified.md` (**AUTHORITATIVE for one spine; bans dual OCC/SF computers**)  
**SoT (structure law — correction):** `lab/notes/specfence-complete-architecture-v9.4-file-srp.md` (**AUTHORITATIVE for file-SRP; demotes folder-layer SoC**)  
**v9.2 module tree:** `lab/notes/specfence-complete-architecture-v9.2-module-structure.md` — **DEMOTED** to optional packaging names only (not product SoC; not S2 success)  
**Audits:** `lab/notes/specfence-v9-whole-plant-callflow-audit.md`; `lab/notes/specfence-v9.1-code-structure-audit.md`  
**Tip:** `1ee6dda` (docs; lineage `bb67ff7`+docs)  
**Posture:** **DESIGN ONLY — DO NOT land; DO NOT implement Rust**; no immediate refactor. Single coherent call-order land only when **explicitly authorized** later.  
**Frame:** SpecFence = **pevm's fused** PC ⊗ CC ⊗ Bayes plant on **one** parallel executor spine. PC/CC/Bayes = **analysis / optimization lenses**, **not** a folder-split goal. Structure law = **file single responsibility** (v9.4). Quiet/cold = Spec cost class ≡ OCC (**not** `plant_is_occ` retreat).  
**Bars (product — raised):** nonempty median **≥0.95** (stretch ≥0.98); quiet p10 **≥0.90**; mixed **≥0.92**; **14689597 ≥0.90 @8 N≥3** (stretch ≥0.95–1.0); R1 win rate **≥50%** on cert-bearing fan_out fails; Bind-after-Done **<10%**; useful_EVM gated; Soft=0.  
*Retired:* median>0.744 / fan≥0.85 / quiet p10≥0.85 as success.  
**Autopsies:** M1–M5; BIND_AFTER_PRODUCER_DONE 442/473; tip median 0.728 / fan 0.362.

---

## 0. Conceptual land order (when eventually authorized — **not now**)

**SoT:** v9.3 (spine) + v9.4 (file-SRP). Feature cuts §1 are **invalid** if landed into god files / dual-π museums without **S−1** + **S0**, or if “success” is defined as creating `pc/cc/bayes` dirs.

| Phase | Work | Primary paths | Done when |
|-------|------|---------------|-----------|
| **S−1** | **Unify pevm spine** — one schedule entry, one validate entry, one execute host for `ConcurrencyMode::SpecFence`; ban hybrid `specfence_plant_is_occ` → `next_occ_task` / `validate_occ_stage` retreat; quiet/cold = Mode(a)=Spec + Bayes.cold + zero meta on **same** spine | `pevm.rs` worker, `scheduler.rs`, `vm` gate, `executor::{plant_is_occ,next_occ_task,validate_occ_stage}` product use | Bifurcation absent from SpecFence product path; SpecFence cold ≠ flip to Occ computer |
| **S0** | **File-SRP** — SPLIT gods (`vm` Fence extract; `rem`→**wave only**; `learner` feeder vs decide); DELETE dual π + `mode.rs`; QUARANTINE museums (`boundary`/`finegrain`/SoftWait Soft/edge·resolve π); MERGE `kernel`→certificate; stop exporting dead decide on Ctx | `vm.rs`, `rem.rs`, `learner.rs`, `edge`/`resolve`/`bayes` π, `boundary`/`finegrain`, `mod.rs` | Gods split; **one** decide on hot path; museums gated; **not** “dirs created” |
| **Then** | Mechanism cuts **0–9** below (call-order plant on **unified** spine, into **SRP files**) | as §1 | v9.1 bars path |

**Removed as success criterion:** former **S2** “create `pc/` `cc/` `bayes/` `fuse/` dirs.” Optional packaging may occur incidental to file moves; it is **not** the SE goal and must not become three siloed plants (v9.4).

**Ban:** PinHold / R1 / admit_seed / Bayes-port patches **before** S−1 + S0 (= patch salad into gods **or** dual-computer).  
**Ban:** Treating folder creation as S0/S2 success while gods/dual π remain.  
**Ban:** S−1/S0/cuts implied by this brief alone — **no land now**.  
**Note:** Cut **0** (dual π) overlaps S0 — do once as S0.  
**Note:** Cut **9** (quiet identity) = Spec cost class on unified spine (v9.3), **not** hybrid OCC computer switch.

---

## 1. Cut order (one call-order plant — **only when land authorized**)

| # | Cut | Frame (lens) | Primary files | Done when |
|---|-----|--------------|---------------|-----------|
| 0 | **Delete/merge dual π** (= **S0**) | hygiene | quarantine `edge::choose_edge_action` π, `resolve::choose_action`, `bayes::{decide,should_wait_hard}`; merge `kernel`→`certificate`; delete `mode.rs` | hot path has **one** decide; museums not default-compiled |
| 1 | **Bayes→PC.admit_seed** | **Bayes → PC** | `bayes` query ports, `learner` feeder / `hotset`/`prior`, `ready_edge` begin_block seed, `pevm` begin_block | ReadyEdges for known stars **before** satellite Execute; quiet_cold only when truly cold |
| 2 | **ProducerStage + refuse** | **PC ⊗ CC** | `producer_stage.rs`, thin computer/scheduler helpers | satellites not ready while ProducerStage(w) Executing; deadlock ban held |
| 3 | **WaitFor PinWithoutThrow** | **CC → PC** | Fence-act (extracted from `vm`), PinHold Stage, wave park/resume, `pevm` Blocking arm | high depth_frac → pin **without** Aborting+FullRetry; steal-without-park not default |
| 4 | **Bayes-queried decide** | **CC ← Bayes** | one decide symbol, `bayes` EV/liveness/depth, `access_vis` (no first edge insert) | Bind rare; WaitFor shape from EV; no OR-bool-only π |
| 5 | **Validate → R1 live** | **CC ⊗ Bayes → PC** | unified validate entry, `certificate`, `repair`, `mv_memory` tip/snap | fenced RAW prefix + tip snap; R1 win rate path; ban always-`validate_occ_kernel` when strips |
| 6 | **Cert strip survival** | **CC** | `certificate::begin_execute`, PinHold resume | no M5 wipe; begin_block clear only |
| 7 | **SerialLane exclusive** | **CC → PC** | `lane.rs`, serial lane act | ban Ready→`occ_unfenced` on live unfinished head |
| 8 | **Telemetry + falsifiers** | shared | `process.rs`, `metrics.rs` | WaitFor shape; Bind-after-Done share; R1 win rate; useful_EVM |
| 9 | **Quiet = Spec cost class ≡ OCC** | shared | unified `pevm`/`scheduler` spine; Bayes.cold; **no** `plant_is_occ` retreat | cold empty Mode(a)=Spec + zero meta on **same** spine; known-star priors still seed edges |

Ship as **one PR / one coherent plant** when authorized. Partial land = non-land.

**Prerequisite (when authorized):** phases **S−1** then **S0** (§0) — **unify spine, then file-SRP**, before cuts 1–9 feature behavior. **Not** “create pc/cc/bayes dirs” as a gate.

---

## 1b. Lens → cut map (not folder ownership)

PC / CC / Bayes here are **thinking lenses** (v9.4), not mandatory directories.

| Lens | Cuts that touch that concern after S0 |
|------|----------------------------------------|
| **PC** | 1 admit_seed, 2 ProducerStage, 3 PinHold Stage, 9 quiet identity (same-spine gate) |
| **CC** | 3 Fence Pin, 4 decide←Bayes, 5 validate/R1, 6 cert survival, 7 SerialLane |
| **Bayes** | 1 seed ports, 4 query ports, 5 covers prior |
| **fuse / honesty** | 8 telemetry falsifiers |
| **research quarantine** | none on hot path |
| **pevm / scheduler / vm / mv_memory** | **S−1 spine owners**; S0 hosts file splits; wiring + unified next/validate; no new π bodies; no hybrid OCC retreat |

Optional later `pc/`/`cc/`/`bayes/` packaging: **not** required for cut success.

---

## 2. SoT § → file/fn map

| SoT topic | Frame (lens) | File:fn |
|-----------|--------------|---------|
| begin_block admit_seed | Bayes → PC | `pevm` begin_block; `ready_edge` seed API; `bayes`/`hotset`/`prior`/`learner` feeder |
| Ready set + PinHold + steal | PC | scheduler / thin computer helpers |
| ProducerStage | PC | `producer_stage.rs` |
| Mode(a) decide ← Bayes | CC ← Bayes | one decide (`access_policy` lineage) |
| WaitFor PinWithoutThrow | CC → PC | Fence-act (out of `vm`); PinHold Stage; **ban** default Aborting FullRetry |
| Gate / Bind / SerialLane | CC → PC | Fence-act + `lane`; thin `vm` calls |
| Vis + liveness | CC ← Bayes | `access_vis.rs`; no first ReadyEdge insert here |
| True-k ordinal | CC | `access_log.rs` |
| Cert / covers / survival | CC | `certificate.rs` (+ merged kernel) |
| Repair R1a/R1b/selective/B0 | PC ← CC⊗Bayes | `repair.rs` wired from unified validate |
| Validate tip snap | PC ⊗ CC | unified validate; `mv_memory` identity/snap |
| Bayes query ports | Bayes | `bayes` (elevate from museum) |
| Process Fence + WaitFor shape | CC | `process.rs` |
| Dual π kill | — | `edge`/`resolve`/`bayes` Boolean APIs / `mode.rs` |
| File-SRP gods | — | `vm` Fence extract; `rem`→wave; `learner` feeder≠decide; quarantine museums |

---

## 3. Kill list (tip bugs + framing)

| Bug / framing @ tip | Design fix |
|---------------------|------------|
| WaitFor → Aborting + FullRetry (M1) | PinWithoutThrow / ScheduleRefuse |
| Bind-after-Done 442/473 | admit_seed while producer Executing |
| quiet_fence_off owns first RAW (M4) / dual OCC↔SF computers | **S−1:** one pevm spine; cold = Spec cost class (v9.3) |
| covers_all Spec siblings (M2) | fenced RAW selective R1 |
| incarnation-strict R1a (M3) | tip identity / value snap |
| begin_execute(inc==0) wipe (M5) | strip survival |
| always validate_occ_kernel | split validate; R1 live |
| Bayes / edge / AEC dual π | delete/merge; one decide (**S0** file-SRP) |
| `vm`/`rem`/`learner` gods | **S0** SPLIT (v9.4) — not “make three folders” |
| Bars median>0.744 / fan 0.85 | **raised** product bars (§1 SoT) |
| PC-primary / CC-only / Bayes-museum **or** folder-silo SoC | fused plant + file-SRP; triple = lens only |

---

## 4. Named-block recipes (implementer smoke — **later**, when authorized)

| Block | Expect |
|------:|--------|
| **14689597** | ReadyEdge consumers←**38** @ k≈6 **before** Execute; PinHold/refuse ≫ Bind-after-Done; R1 win ≥50%; SF/OCC **≥0.90** @8 N≥3; Bind-after-Done <10% |
| **19807137** | OrderedAdmit; abort_SF≤OCC; no Aborting WaitFor mass without head progress; ≥0.70 then ratchet |
| quiet / **2179522** | OCC identity (same-spine Spec cost class); p10 cohort ≥0.90; no template PE |
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
- **“Structure done” because `pc/cc/bayes` dirs exist while gods/dual π remain** (v9.4)  
- Feature cuts 1–9 without S−1 + S0 file-SRP  
- Fence Aborting policy body still living in `vm.rs` after claimed S0  
- Dual π re-exported from `mod.rs`  
- Dual-computer quiet retreat after claimed S−1  
- **Any land started from this brief without explicit authorization**  

---

## 6. Out of scope

No SoftWait Soft, canary, ForcePrefix-as-π, Occ\|Pcc incarnation fork, P0/P1/P2 staging, celebrating retired 0.744 bars; **implementing Rust / landing now**; PC-primary / CC-only / Bayes-museum partial plants; **folder-split of PC vs CC vs Bayes as SE goal**; feature-first land that leaves god files and dual π; treating v9.2 dir tree as mandatory SoC.

---

## 7. Posture restated

**No land now.** This brief is a **design map**. Triple = lenses. Spine unity (v9.3) + file-SRP (v9.4) are the conceptual prerequisites **when** land is later authorized — success is **not** “created pc/cc/bayes directories.”
