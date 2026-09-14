# SpecFence v8 — WaitFor / R1 code-path autopsy

**Tip:** `bb67ff7` (docs honesty after quiet-OCC computer)  
**Scope:** code-path only — why WaitFor does not drive SF aborts ≪ OCC, and why R1 stays dead.  
**No Rust edits.** Honesty context: 14689597 N=3 @8 — Wait≪Bind inverted (Wait 67 / Bind 7), **R1=0**, aborts **98 vs OCC 71** (`lab/notes/specfence-v8-parallel-computer-impl.md`).

---

## 1. WaitFor lifecycle (arm → park → resume → validate) and where useful work is lost

```
PE-on access
  → access_log.note(ℓ)                    vm.rs::specfence_access_gate
  → access_vis(ℓ)                         unfinished=!done (S2); tip vs ReadyEdge
  → access_policy::decide
       WaitFor{w} iff unfinished==1 ∧ writer_executing ∧ ev_win
  → pcc_wait_for_writer
       admit_spine(w); ReadyEdge consumer
       if w Done  → note_fence_success(ℓ); occ_unfenced()     # no park
       if !Executing → occ_unfenced()                         # Ready/Aborting: no park
       else:
         note_fence_success(ℓ)          # cert BEFORE value read
         dag.arm_hard_wait(ℓ,t,w)
         wave.set_pending_park(..., BlockingOther)
         Err(Blocking(w))
  → pevm try_execute Blocking arm
       add_dependency(t,w) → status=Aborting                  # mid-tx progress discarded
       often arm_steal_convert_without_park()                 # steal without long park
       else wave.park_with_kind(BlockingOther)
  → wake: finish_execution drains dependents → set_ready_status → incarnation++
       wave.wake_writer_done may record_resume_intent (only if parked in wave table)
  → next execute: set_tx → certificates.begin_execute(t, repair?, incarnation)
       try_apply_park_resume only if resume_intent present (SoftWait/parked path)
       BlockingOther / steal-without-park → typically FullRetry from tx head
  → validate_specfence
       no strip → validate_occ_kernel (B0)
       covers_all ∧ value_stable rebind → R1a; else B0
```

**Where useful work is lost**

| Stage | Loss |
|-------|------|
| Mid-access `Blocking` | Interpreter progress to the WaitFor site is thrown away; `add_dependency` marks **Aborting** (not “pause same incarnation”). |
| Steal-without-park | `ParkKind::BlockingOther` prefers steal; if steal wins, no `park_with_kind` → **no** `resume_intent` → wake is dependents-only FullRetry. |
| Soft=0 | Even with `resume_intent`, Blocking WaitFor is not SoftWait; `try_arm_park_resume_at_k` is SoftWait/SuffixRepair shaped — WaitFor does not keep a live PC suffix. |
| `pcc_armed` never set | Comment in `pcc_wait_for_writer`: rem overlay / Bind-theater banned — after wake the fenced ℓ is still consumed via **`occ_unfenced`** (OCC MV walk), not a bound Fence read. |
| Same-incarnation retry | Writer already Done → `add_dependency` false → `continue` → `set_tx` with **incarnation==0** → `begin_execute` **clears** strips (see §3). |

Net: WaitFor buys a producer pin + a location strip, then pays **Aborting + full EVM reentry** — the same cost class as OCC reincarnation for that tx — without converting the eventual validate miss into R1.

---

## 2. Exact conditions for R1a vs fallthrough B0; why `covers_all` fails in practice

### Grain (`repair.rs::repair_grain`)

- **R1** iff `invalid.is_empty() \|\| cert.covers_all(tx, invalid)`.
- **B0** otherwise (Spec-only **or mixed** Spec+Fenced fails).

### Live validate (`executor.rs::validate_specfence`)

1. `has_cert = certificates.has_any(t) \|\| kernel.may_resolve(t)`; else → **`validate_occ_kernel` (B0)**.
2. If OCC read-set already valid → success (no R1).
3. `invalid = collect_invalid_reads`.
4. **R1a attempt** when fenced subset nonempty:
   - `covers_all` → `fenced = invalid`;
   - else selective `fenced = { ℓ ∈ invalid \| covers(t,ℓ) }`.
   - Require every fenced ℓ has current Data **and** `prior_read_value_stable`.
   - `try_rebind_invalid_reads_value_stable(fenced)`.
   - Success only if `fenced.len()==invalid.len()` **or** whole RS becomes valid after rebind.
5. Else → **`validate_occ_kernel` (B0)**.

`repair_grain` / tests encode the sticky-cert ban: one Bind/WaitFor strip must **not** cover sibling Spec misses (`certificate.rs` tests `one_bind_does_not_cover_sibling_spec`, `wait_resume_keeps_location_strips`).

### Why `covers_all` fails in practice

WaitFor certifies **only the park location** at arm time (`note_fence_success` → `CertificateTable::note_success`). A typical tx read-set has many Spec≡OCC siblings. After wake + FullRetry:

- The WaitFor’d ℓ may be covered (strip kept on `inc>0`).
- Sibling Spec RAW still race → appear in `invalid`.
- `covers_all` → false → selective rebind cannot heal Spec residuals → fallthrough **B0**.

Honesty falsifier matches: WaitFor fires (67) while **R1a stays 0**.

---

## 3. Why SF aborts can stay ≈ OCC after WaitFor

Mechanisms (code-backed):

### A. Quiet-OCC / PE gate — WaitFor never arms on the cohort that needs it first

- `specfence_plant_is_occ` when `!has_any_predicted() \|\| quiet_fence_off()` → access gate early `Ok(())` (byte OCC).
- `decide` `ev_win = !quiet_off && (intra \|\| (fan && prior_pe_fire_wins))`; quiet holds WaitFor until `abort_events≥4` / park heat (`learner::quiet_fence_off`).
- First-wave conflicts still OCC-abort then train PE — WaitFor is **post-abort**, not prevent-first.

### B. Race after wake (producer reincarnation)

After WaitFor wake the reader OCC-reads writer incarnation *N*. Writer may later validation-abort and republish *N+1*. Reader validate sees origin mismatch. Even with `covers_all`, R1a demands `prior_read_value_stable`, which **requires matching writer incarnation** (`mv_memory.rs`) — same U256 still fails → B0. (Museum `try_validate` has identity/snap fallbacks; **`validate_specfence` does not**.)

### C. Wrong producer

`decide` WaitFor uses `vis.writer` from `compose_unfinished` (sketch / last_writer / residual / force_writer). If that tip ≠ true RAW conflict producer (ReadyEdge `predicted_producer` is only a Bind tip check), the plant parks the wrong *w*; the real conflict ℓ stays Spec → abort ≈ OCC.

### D. Spec siblings (covers_all structural fail)

Strip is per-ℓ Fence success, not tx-global. Mixed invalid sets are protocol-B0 (`repair.rs` test). WaitFor volume without fencing the whole fail set cannot cut validation aborts.

### E. Cert stripped on same-incarnation resume (not on `inc>0` wake)

- Design intent: `begin_execute(..., incarnation>0)` **keeps** location strips (`certificate.rs` + test `wait_resume_keeps_location_strips`).
- Hole: Blocking with writer already Done → retry **same** `tx_version` → `incarnation==0` branch **`st.clear(false)`** wipes the strip just written by `note_fence_success`. Cert does not survive that race-retry.

### F. WaitFor is Aborting-shaped, not abort-reducing

`add_dependency` → Aborting + later FullRetry is cost-class-adjacent to OCC reincarnation. It does not mark the read Fence-consumed for validate R1 unless `covers_all`+value_stable fire — which honesty shows do not. Cascade from B0 still trains PE and reexecutes dependents like OCC.

### G. Done / Ready fallthrough without park

If `decide` says WaitFor but `pcc_wait_for_writer` sees Ready/Aborting, it **`occ_unfenced`** (no park, often no durable Fence progress beyond edge reserve). ESTIMATE/Spec continue → OCC abort path.

---

## 4. File:fn citations

| Concern | Citation |
|---------|----------|
| Decide WaitFor / Bind / SerialLane | `access_policy.rs::decide` (WaitFor: unfinished==1 ∧ executing ∧ ev_win; Bind-rare: tip_is_conflict_producer ∧ !bind_tax_losing) |
| Quiet / EV brakes | `learner.rs::quiet_fence_off`, `prior_pe_fire_wins`, `bind_tax_losing`, `pcc_makespan_win` |
| Plant OCC retreat | `executor.rs::specfence_plant_is_occ`; `vm.rs::specfence_access_gate` early return |
| Vis unfinished=!done + tip | `vm.rs::access_vis` |
| WaitFor act / cert / park | `vm.rs::pcc_wait_for_writer`, `note_fence_success`, `fence_wait_for` (legacy rem path) |
| Blocking → Aborting / steal | `pevm.rs` `try_execute` `Err(Blocking)`; `scheduler.rs::add_dependency`, `set_ready_status` (inc++) |
| Park / resume intent | `rem.rs::WaveParkTable::{set_pending_park,park_with_kind,arm_steal_convert_without_park,wake_writer_done_intents,take_resume_intent}`; `vm.rs::try_apply_park_resume` |
| Cert strip lifecycle | `certificate.rs::{begin_execute,note_success,covers_all,covers,has_any}` |
| Repair grain | `repair.rs::{RepairGrain,repair_grain}` |
| Validate R1a / B0 | `executor.rs::validate_specfence` → fallthrough `validate_occ_kernel`; dispatch `pevm.rs` SpecFence Validation arm |
| Value-stable incarnation gate | `mv_memory.rs::{prior_read_value_stable,try_rebind_invalid_reads_value_stable}` |
| Hard-wait DAG | `dag.rs::{arm_hard_wait,wake_on_data}` |

---

## 5. Five bullet mechanisms (report summary)

1. **WaitFor = Aborting + FullRetry, not R1 progress** — parks certify ℓ then discard interpreter work; Soft=0 / BlockingOther steal often skip `resume_intent`; post-wake read is `occ_unfenced`.
2. **`covers_all` dies on Spec siblings** — strip is per-ℓ; mixed invalid → B0 by protocol; selective rebind cannot clear Spec residuals.
3. **R1a value_stable is incarnation-strict** — writer reincarnation after wake fails `prior_read_value_stable` even on same output; `validate_specfence` lacks museum identity/snap fallback → R1 stays 0.
4. **Quiet-OCC + EV gates starve early WaitFor** — first wave Spec-aborts like OCC; WaitFor arms only after PE/intra heat; wrong/Ready producer fallthrough stays Unfenced.
5. **Same-incarnation Blocking retry clears certs** — `begin_execute(inc==0)` wipe; `inc>0` keeps strips but honesty shows kept strips still do not buy R1 under (2)+(3).

**Path:** `lab/notes/specfence-v8-waitfor-r1-codepath.md`
