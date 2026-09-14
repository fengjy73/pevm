# SpecFence v9 whole-plant call-flow audit (tip `bb67ff7`)

**Date:** 2026-09-14 (Asia/Shanghai, UTC+8)  
**Tip:** `bb67ff7` Soft=0  
**Mandate:** end-to-end live flow vs SoT (`specfence-complete-architecture-v9-cc-pc-bayes.md`) + WaitFor autopsies; raise bars; overall redesign (not patch salad).  
**Design only — no Rust plant changes.**  
**Superseding SoT:** `lab/notes/specfence-complete-architecture-v9.1-cc-pc-bayes.md`  
**Land brief:** `lab/notes/specfence-v9-land-brief.md` (updated for v9.1)

**Absorbed evidence:**
- `lab/notes/specfence-v8-waitfor-abort-r1-autopsy.md`
- `lab/notes/specfence-v8-waitfor-r1-codepath.md`
- `lab/notes/specfence-v8-all-blocks-process-mishandle.md`
- Measured @ tip Soft=0: nonempty median **0.728**; **14689597 N=3 ≈0.362**; WaitFor **2260** > Bind **1895**; R1a=**4**/R1b=**0**; star **442/473 BIND_AFTER_PRODUCER_DONE**; abort median ratio ≈**1.0** vs OCC.

---

## 1. Live call flow (as wired today) — E2E

```
pevm::execute_block (begin_block)
  ├─ hotset.begin_block(); learner.begin_block_with_params(inter_prior.morph_ema)
  ├─ inter_prior.top_locations → hotset.track_from_prior + mild bayes.observe_conflict
  ├─ sketch.seed_from_prior_morph; if !quiet: learner.seed_predicted_essential(ℓ,k_template)
  ├─ NEW tables: ready_edges, producer_stages, certificates, lanes, access_log, kernel
  └─ ⚠ NO ReadyEdge(consumer←producer) seed for known star consumers (Bayes/HotSet→track only)

worker loop:
  task := if Occ ∨ specfence_plant_is_occ(empty PE ∨ quiet_fence_off)
             then next_occ_task(scheduler)          # Block-STM indices only
             else next_sf_task(sched, wave, ready, stages)
                    ├─ ProducerStage next_reserved → try_execute_producer
                    └─ scheduler.next_task_with_wave_ready(wave, ready)
                         └─ may_execute refuse known consumers only

  Task::Execution(tx):
    try_execute → vm.execute
      storage/basic read:
        maybe_wait → SpecFence → specfence_access_gate
          if plant_is_occ → Ok(()) OCC                    # M4 first wave
          access_log.note(ℓ)                             # PE-on only
          if !location_predicted → Ok(())
          vis := access_vis(ℓ)                           # unfinished=!done
            if hot∨ws_hat → note_raw_producer + note_consumer + reserve  # TOO LATE (mid-tx)
          decide(learner, ℓ, k, vis)                     # OR-bool EV; NO bayes query
            UnfencedOcc → Ok(())
            Bind → note_fence_success; occ_unfenced      # cert + OCC read
            WaitFor{w} → pcc_wait_for_writer
              Done → note_fence_success; record_edge_bind; occ_unfenced   # BIND_AFTER_DONE
              !Executing → occ_unfenced                  # no park
              Executing → note_fence_success; Err(Blocking(w))
            SerialLane → often collapses to WaitFor or occ_unfenced canary
      on Err(Blocking):
        add_dependency → status=Aborting                 # M1 throw mid-tx
        often arm_steal_convert_without_park             # no PinHold / resume
        else wave.park_with_kind(BlockingOther)
      on Ok: finish_execution; ready_edges.note_producer_done; release

  Task::Validation(tx):
    validate_specfence
      !has_cert → validate_occ_kernel (B0 + PE train + late ReadyEdge)
      has_cert ∧ RS invalid:
        covers_all? else selective fenced subset
        prior_read_value_stable (incarnation-strict)     # M3
        try_rebind → R1a almost never (M2 Spec siblings)
        else → validate_occ_kernel B0                    # always-B0 theater
      repair_grain / R1b SuffixRepair NEVER default arm

end_block: bayes.decay; inter_prior.end_block(pack_top); Soft must stay 0
```

**Who actually owns π today**

| Port | Live owner | Bayes? |
|------|------------|--------|
| Schedule admit | `quiet_fence_off` → OCC computer; else ReadyEdge `may_execute` (consumer bits filled mid-tx / abort) | **no** |
| Mode(a) decide | `access_policy::decide` ← LiveLearner OR-bools (`ev_win`, `quiet_fence_off`, `bind_tax_losing`) | **no query** |
| Fence act | `vm::pcc_wait_for_writer` / Bind / SerialLane | observe only |
| Validate/Repair | `validate_specfence` → mostly `validate_occ_kernel` | observe conflict on B0 |
| Prior seed | InterPrior → PE class + HotSet track; **not** ReadyEdge fan-out consumers | mild conflict bump |

---

## 2. File-by-file / seam problems vs v9 SoT

### 2.1 Hot-path (live but wrong shape)

| File | Role @ tip | Broken / inconsistent vs v9 |
|------|------------|-----------------------------|
| `pevm.rs` worker | hybrid OCC↔SF by `quiet_fence_off` | First wave forced OCC computer (M4); no begin_block ReadyEdge seed; Blocking→Aborting+steal (M1); no PinHold Stage |
| `scheduler.rs` | Block-STM + wave ready + `may_execute` refuse | Refuse only **known** consumers; satellites still Execute before edge; ProducerStage promote weak vs 448 fan-out |
| `computer.rs` | `next_sf_task` ~45 LOC | No PinHold; no Validate pipeline as first-class Stage priority beyond scheduler; SoT cites v8 header |
| `vm.rs` `specfence_access_gate` | Mode(a) act | Decide never sees Bayes; WaitFor = Blocking throw; Done→Bind counts Bind (442/473); `pcc_armed` never set → post-wake `occ_unfenced` |
| `vm.rs` `pcc_wait_for_writer` | Avoid act | Cert before value read; Aborting-shaped; Bind-after-Done fallthrough |
| `vm.rs` `pcc_serial_lane` | Lane act | Ready head → `occ_unfenced` Spec canary (not exclusive refuse); process `wait_for_serial≈0` on worst set |
| `vm.rs` `access_vis` | Hot/WS → edge mid-tx | Edge insert **during** consumer Execute — schedule-first Avoid already lost |
| `access_policy.rs` | decide | OR-bool EV museum; no `P_RAW`/`EV[hold]`/`depth_frac`/`P(covers_all)` ports |
| `executor.rs` `validate_specfence` | Resolve | Incarnation-strict R1a; Spec siblings kill covers_all; fallthrough always OCC B0; R1b unused |
| `executor.rs` `validate_occ_kernel` | B0+learn | Trains PE + late ReadyEdge — learning after damage |
| `certificate.rs` | strips | `begin_execute(inc==0)` wipe (M5); strip≠fenced-prefix RAW cover semantics SoT wants |
| `repair.rs` | grain helper | `covers_all` all-or-B0; selective fenced RAW + tip snap **not** in grain; R1b not modeled |
| `ready_edge.rs` | consumer←producer | No prior/Bayes consumer fan-out insert; observe-only until mid-tx/abort |
| `producer_stage.rs` | reserve | Exists; not authoritative before satellite Execute on stars |
| `learner.rs` | PE / quiet / EV OR-bools | Real π; `quiet_fence_off` contradicts “known-star first wave”; Bayes peer demoted |
| `bayes.rs` | BetaMap | **Museum** — `decide`/`should_wait_hard` dead for SpecFence; no ReadyEdge/PE/EV/covers ports |
| `prior.rs` / `hotset.rs` | WŜ / HotSet | Feed learner posterior bump + vis mid-tx; do **not** author ReadyEdges at begin_block |
| `process.rs` / `metrics.rs` | telemetry | WaitFor shape (pin vs Aborting) not distinguished; Bind-after-Done counted as Bind success |

### 2.2 Dead / contradictory / museum (delete · merge · quarantine)

| File / symbol | Status | Action (v9.1) |
|---------------|--------|----------------|
| `edge.rs` `choose_edge_action` | **Dead on hot path** (tests + export); π replaced by `access_policy::decide` | **Merge** Detect helpers into access_log/sketch; **delete** live π dual |
| `resolve.rs` `choose_action` / `PolicyCtx` / AEC EV | Retired; `SpecFenceCtx::choose_resolve` lab-only | **Quarantine** under `research_` or delete from SpecFenceCtx |
| `bayes.rs` `decide` / `should_wait_hard` / RegionMode Wait | Explicitly “Not SpecFence π” | **Rewire** into query ports (§6 v9.1); kill Boolean π APIs |
| `mode.rs` | Thin reexport of access_policy | **Delete** or fold |
| `region.rs` Wait bits | SpecFence stub always false | Keep PCC-only; SpecFence must not read as SoT |
| `engagement.rs` Lean/Storm | Still ticks begin_tx / morph label | **Demote** — not Avoid/Resolve peer; morph → Bayes morph prior only |
| `heat.rs` | PCC | Leave PCC; ban SpecFence SoftWait seed |
| `kernel.rs` `note_fence` | Tx-bool may_resolve parallel to CertificateTable | **Merge** into certificates; kill sticky tx-global SoT risk |
| `boundary.rs` (~3.5k LOC) Bind-snap / inspect / jump | Research museum; production OFF | **Quarantine** — not on v9.1 critical path |
| `rem.rs` SoftWait / SuffixRepair / WavePark | Soft=0; WaitFor uses Aborting not rem resume | Keep WavePark/Producer wake; **strip** SoftWait Soft as live Avoid |
| `decision_field.rs` | Telemetry feats | Keep as observe; not π |
| `dag.rs` FenceGraph SoftWait | Soft=0; hard_wait arm still used | Hard-wait OK; SoftWait Soft paths dead |
| `mod.rs` module docs | Still cites **v8** SoT + Iter6–30 SoftWait archaeology | **Rewrite** header to v9.1; move Iter novel to `lab/notes/` archive |
| `SPECFENCE.md` / README plant claims | Likely lag honesty | Align to v9.1 bars + “no celebration @ tip” |

### 2.3 Seam contradictions (measured)

| Seam | SoT / intent | Live @ `bb67ff7` | Evidence |
|------|--------------|------------------|----------|
| PC admit vs quiet | Known stars seed edges before Execute | `quiet_fence_off` ⇒ `next_occ_task` + gate early Ok | M4; first aborts ≈ OCC |
| WaitFor shape | PinWithoutThrow / ScheduleRefuse | Aborting + FullRetry (+ steal-without-park) | M1; depth_frac≈0.86 throws |
| Bind rarity | tip∧EV; Avoid early | Done→Bind 442/473 on star | mishandle catalog |
| R1 | live when certs | R1a=4 total / R1b=0; star R1=0 | autopsy |
| covers_all | fenced RAW prefix → selective R1 | mixed → protocol B0 | M2; repair.rs test |
| tip identity | value snap non-incarnation-strict | `prior_read_value_stable` incarnation-strict | M3 |
| Cert lifecycle | survive PinHold / same-tx | `begin_execute(inc==0)` wipe | M5 |
| Bayes peer | queries at admit/decide/validate | OR-bool LiveLearner; Bayes observe-only | bayes.rs + decide |
| Bars | median>0.744; fan≥0.85 | median **0.728**; fan N=3 **0.362** | honesty |
| Soft | 0 | **0 held** | — |

---

## 3. Top 10 file / flow problems (priority)

1. **No schedule-first Avoid** — ReadyEdges filled mid-tx/`access_vis` or post-abort; 448 satellites Execute then Bind-after-Done (`ready_edge` + `vm::access_vis` + begin_block gap).  
2. **WaitFor = Aborting+FullRetry** — `pcc_wait_for_writer` → `Err(Blocking)` → `add_dependency` Aborting; PinHold Stage absent (`vm` + `pevm::try_execute` + `computer`).  
3. **`quiet_fence_off` owns first RAW wave** — `specfence_plant_is_occ` forces OCC computer despite InterPrior/HotSet stars (`executor` + `pevm` worker + `learner`).  
4. **Bayes demoted to museum** — `access_policy::decide` never queries `BayesMap`; no P(covers_all)/EV[hold]/depth_frac ports (`bayes` + `access_policy`).  
5. **Resolve always-B0 theater** — `validate_specfence` Spec-sibling + incarnation-strict → `validate_occ_kernel`; R1b never armed (`executor` + `repair` + `mv_memory`).  
6. **Cert wipe / weak strip** — M5 `begin_execute(inc==0)`; WaitFor cert ≠ Fence-consumed post-wake read (`certificate` + `pcc_armed` ban).  
7. **Dual π museums** — `edge::choose_edge_action` + `resolve::choose_action` + `bayes::should_wait_hard` coexist with live `decide` (`edge`/`resolve`/`bayes`/`mod`).  
8. **SerialLane Ready→Spec canary** — exclusive lane not enforced; hang-avoidance regresses to Unfenced (`pcc_serial_lane`).  
9. **ProducerStage/computer too thin** — refuse/promote exists but does not prevent satellite first Execute on stars; no PinHold/Validate steal law as SoT (`computer` ~45 LOC).  
10. **Bars too small vs product** — median>0.744 / single-block 0.85 leaves plant “honest miss” while quiet carries median; fan_out 0.362 unfixed — need product-grade bars (v9.1).

---

## 4. Overall call-flow redesign (summary — full in v9.1)

**Target authority order (one coherent plant):**

```
Bayes.seed(InterPrior, HotSet, WŜ, morph)
  → PC.admit: insert ReadyEdges where P_RAW·EV_admit; ensure ProducerStage(w)
  → PC.schedule: ProducerStage ∪ edge-satisfied Execute ∪ Validate ∪ Repair ∪ PinHold
  → Execute(t) only if admitted:
       CC.decide ← Bayes.queries(vis, k_true, depth_frac, liveness, EV[*])
       ScheduleRefuse preferred; else PinWithoutThrow; AbortingThrow last resort
       Bind rare (tip==conflict ∧ EV ∧ !tax)
  → Validate: RS_spec bool; RS_fence tip_identity_or_snap; fenced RAW prefix → R1
  → Repair: R1a/R1b/selective/B0; Bayes.update; strengthen edges
```

**When Bayes is queried:** begin_block seed; every admit; every decide; every validate/repair grain.  
**When PC admits:** before Execute — refuse satellites while ProducerStage(w) Executing.  
**When CC fences:** only on PE-on admitted Execute; verbs from Bayes EV ranking.  
**When Repair runs:** Validate fail → grain from cert cover ⊗ Bayes P(covers_all) ⊗ tip snap — never default always-OCC-B0 while strips exist.

---

## 5. Raised success bars (product-grade) — ratified in v9.1

| Metric | Old v8/v9 | **v9.1 product** |
|--------|-----------|------------------|
| Nonempty median SF/OCC @8 Soft=0 | >0.744 | **≥0.95** approaching OCC; stretch **≥0.98** |
| Quiet (+quiet_ish) median | ≈1.0 | **≥0.98** (OCC identity) |
| Quiet p10 | ≥0.85 | **≥0.90** |
| Mixed cohort median | (unset) | **≥0.92** |
| Named fan_out **14689597** @8 N≥3 | ≥0.85 | **≥0.90** stable; stretch **≥0.95–1.0** |
| Spine **19807137** @8 N≥3 | (unset) | **≥0.70** then ratchet; abort SF ≤ OCC |
| Soft / exclude | 0 | **0** |
| R1 win rate (fan_out w/ PE+certs) | >0 | **≥50%** of cert-bearing validate fails → R1a∨R1b (not token 4) |
| WaitFor↑∧abort≈OCC | falsifier | **forbidden** steady state |
| Bind-after-Done share on star | 442/473 | **<10%** of star consumers |
| useful_EVM / (P·wall) | (unset) | **≥0.70** on quiet; **≥0.55** on fan_out @8 |
| Abort SF/OCC median | ~1.0 | **≤1.0**; fan_out **≤0.9** |

---

## 6. Deliverable map

| Artifact | Path |
|----------|------|
| This audit | `lab/notes/specfence-v9-whole-plant-callflow-audit.md` |
| Authoritative SoT (supersedes v9) | `lab/notes/specfence-complete-architecture-v9.1-cc-pc-bayes.md` |
| Land brief | `lab/notes/specfence-v9-land-brief.md` |
| Code-structure audit | `lab/notes/specfence-v9.1-code-structure-audit.md` |
| Module SoT (v9.2) | `lab/notes/specfence-complete-architecture-v9.2-module-structure.md` |

**One-paragraph verdict:** Tip `bb67ff7` is a Soft=0 PC⊗CC shell with ReadyEdge/ProducerStage grafted onto an OCC-first worker that still learns after first-wave Spec aborts, Avoids with Aborting WaitFor / Bind-after-Done, and Validates with almost-never R1 — while Bayes, `choose_edge_action`, and AEC `choose_action` sit as museums. v9 named the triple frame; the plant never rewired the call order. v9.1 makes **Bayes→PC.admit→CC.decide→PinHold/Refuse→Validate/Repair** the sole live spine, deletes dual π, and raises bars to product-grade (median≥0.95, fan_out≥0.90 N≥3, quiet p10≥0.90, R1 win rate, useful_EVM).
