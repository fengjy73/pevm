# SpecFence v8 land brief — SoT → file/fn

**Date:** 2026-09-14 (Asia/Shanghai, UTC+8)  
**SoT:** `lab/notes/specfence-complete-architecture-v8-parallel-computer.md` (**AUTHORITATIVE**)  
**Tip:** `3376ac4`  
**Posture:** immediate full-batch implement; **no P0/P1/P2**; Soft=0; do not change this brief’s mapping without updating SoT.  
**Bars:** median >0.744; **14689597 ≥0.85 @8 N≥3**; quiet p10 ≥0.85; Soft=0.

---

## 1. Cut order (one computer)

| # | Cut | Primary files | Done when |
|---|-----|---------------|-----------|
| 1 | **ProducerStage + ReadyEdge admit** (fix deadlock) | `ready_edge.rs`, **NEW** `producer_stage.rs`, `computer.rs::next_sf_task`, `scheduler.rs::next_task_with_wave_ready` / `try_execute_ready` | refuse Execute(t) **on**; ProducerStage(w) always in ready if work; no spin |
| 2 | **First-wave edges** from HotSet/WŜ/prior/abort | `learner.rs`, `hotset.rs`, `prior.rs`, `ready_edge.rs` | edges before doomed satellite Execute; ESTIMATE→PE still banned |
| 3 | **Bind-rare decide + true-\(k\)** | `access_policy.rs::decide`, `vm.rs::specfence_access_gate`, `access_log.rs`, `access_vis.rs` | WaitFor/lane primary; Bind only `tip_is_conflict_producer`∧EV; `ordinal.note` when PE-on; **no** fan_out templates |
| 4 | **Validate split → R1** | `executor.rs`, `certificate.rs`, `repair.rs`, `pevm.rs` SpecFence validate branch | RS_spec bool; RS_fence `covers_all` → R1a/R1b; **ban** always-`validate_occ_kernel` when strips exist |
| 5 | **SerialLane exclusive** | `lane.rs`, `vm.rs::pcc_serial_lane` | ban Ready→`occ_unfenced` on live unfinished head |
| 6 | **Telemetry + falsifiers** | `process.rs`, `metrics.rs` | every Fence verb `process.record`; Bind↑∧abort↓ → roi_skip class |
| 7 | **Quiet OCC identity held** | `pevm.rs` hybrid, `vm.rs` empty-PE gate | empty PE ∧ no edges ≡ OCC helpers; ordinal HashMap ops=0 |

Ship as **one PR / one coherent plant**. Partial land (edges without ProducerStage, or Bind-rare without R1) is a non-land.

---

## 2. SoT § → file/fn map

| SoT topic | File:fn / module |
|-----------|------------------|
| Ready set + steal + pipeline | `specfence/computer.rs` (`next_sf_task` → wire `Some(ready)`); `scheduler.rs` |
| ProducerStage reserve/progress | **NEW** `specfence/producer_stage.rs`; call from `computer` + `ready_edge` |
| ReadyEdge insert / may_execute / release | `specfence/ready_edge.rs` |
| Mode(a) decide Bind-rare | `specfence/access_policy.rs::decide` |
| Gate + Bind/WaitFor/SerialLane act | `vm.rs::specfence_access_gate`, `pcc_wait_for_writer`, `pcc_serial_lane` |
| Vis unfinished=!done + tip_is_conflict_producer | `specfence/access_vis.rs::compose_unfinished` (+ tip helper) |
| True-\(k\) ordinal | `specfence/access_log.rs` — **call `note(ℓ)` on PE-on Execute**; kill template spray in `learner.rs::note_abort_access` for fan_out |
| Cert strips / covers_all | `specfence/certificate.rs` |
| Repair R1a/R1b/B0 | `specfence/repair.rs` (wire from SpecFence validate; stop ignoring) |
| Validate dispatch | `specfence/executor.rs`; `pevm.rs` SpecFence branch — split RS_spec/RS_fence |
| HotSet/WŜ → edges + PE posterior | `specfence/hotset.rs`, `prior.rs`, `learner.rs` (`note_hot_ws_posterior` must insert ReadyEdge, not bump-only) |
| Lane tokens | `specfence/lane.rs` |
| Process record Fence verbs | `specfence/process.rs` from Bind/WaitFor/lane success paths |
| Empty-PE OCC retreat | `pevm.rs` `!has_any_predicted` schedule/execute; `vm.rs` `specfence_access_is_occ` |
| Kernel debug mirror | `specfence/kernel.rs` — not SoT |

---

## 3. Explicit kill list (tip bugs)

| Bug @ `3376ac4` | Fix |
|-----------------|-----|
| `computer.rs` `let _ = ready` | Wire `next_task_with_wave_ready(..., Some(ready))` **with** ProducerStage |
| Bind on any published Data | `decide`: require `tip_is_conflict_producer` ∧ EV_win |
| `access_log.note` never called | Call on PE-on path only |
| fan_out templates `[1,6,10,20]` | Delete / forbid in `note_abort_access` when morph=fan_out |
| Gate `dominant_k.max(1)` sole | Prefer live ordinal \(k\); dominant_k = prior mean only |
| SpecFence always `validate_occ_kernel` | Split validate; R1 when `covers_all` |
| SerialLane Ready → `occ_unfenced` | Exclusive permit only |
| HotSet posterior bump only | ReadyEdge insert + PE posterior |
| Bind/WaitFor invisible to process | `process.record` on every successful verb |

---

## 4. Named-block recipes (implementer smoke)

| Block | Expect |
|------:|--------|
| **14689597** | ReadyEdge star consumers←producer 0 at k_true≈6; WaitFor/lane ≫ Bind; R1>0; SF/OCC **≥0.85** @8 N≥3; Bind↓ while aborts→OCC |
| **19807137** | OrderedAdmit + off-spine steal; park without head progress → fail |
| **2179522** / quiet | empty-PE OCC identity; no template PE; quiet p10 cohort ≥0.85 |
| Soft | **0** everywhere |

---

## 5. Falsifiers (stop / revert class)

- Soft>0  
- Refuse without ProducerStage runnable  
- Bind↑ ∧ abort↓ on 14689597  
- R1a=R1b=0 while PE+certs present on fan_out  
- Template PE on fan_out  
- Median claim without JSON  
- CC-only partial land leaving ready refuse off  

---

## 6. Out of scope this land

No SoftWait Soft, canary, ForcePrefix-as-π, Occ\|Pcc incarnation fork, rem-overlay WaitFor resurrection, P0/P1/P2 staging, celebrating median while fan_out≪OCC.
