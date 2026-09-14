# SpecFence v8 — WaitFor↑ ∧ abort≈OCC ∧ R1 dead (autopsy)

**Date:** 2026-09-14 (Asia/Shanghai, UTC+8)  
**Tip:** `bb67ff7` (`cursor/specfence-v8-pc-cc-computer-f6cf`)  
**Plant:** Soft=0; PC⊗CC co-equal; empty-PE OCC computer  
**Codepath companion (folded):** `lab/notes/specfence-v8-waitfor-r1-codepath.md`  
**Impl map:** `lab/notes/specfence-v8-parallel-computer-impl.md`  
**Catalog:** `lab/notes/specfence-v8-all-blocks-catalog.json`  
**Sweeps (this tip, Soft=0 @8):**
- N=1 all 99 → `lab/results/v8-waitfor-autopsy/all-blocks-n1.json`
- N=3 focus15 → `lab/results/v8-waitfor-autopsy/focus-n3/summary.json`
- Process per-tx → `lab/results/v8-waitfor-autopsy/process-per-tx/`
- Effect-raw truth (14689597) → `lab/results/effect-raw-deep-b14689597.json`

**No Rust plant edits.**

---

## 0. Direct answer (read first)

### Why WaitFor rose but aborts still ≈ OCC

WaitFor volume is real (N=1 nonempty totals **WaitFor 2260 > Bind 1895**; 70/98 blocks fire WaitFor). It does **not** buy abort reduction because WaitFor is **Aborting-shaped scheduling**, not a validate-winning Fence consume:

1. First wave is still quiet-OCC / empty-PE Spec → aborts train PE **before** WaitFor arms.
2. Live WaitFor (`vm.rs::pcc_wait_for_writer`) certifies ℓ then throws mid-tx work via `Err(Blocking)` → `add_dependency` **Aborting** → FullRetry (often steal-without-park → no SoftWait-style resume). Soft=0.
3. Post-wake read is **`occ_unfenced`** (`pcc_armed` never set) — OCC MV walk, same abort class as OCC when writer reincarnates or siblings race.
4. Decide/Ready fallthrough: WaitFor only if `unfinished==1 ∧ writer_executing ∧ ev_win`; Ready/Aborting → **`occ_unfenced` without park**. Done → Bind+OCC (counts Bind, not a prior Avoid win).
5. Fan-out star (14689597 ℓ=`85335018835337005`, producer tx **38**, **448** program consumers): process one-shot shows hot ℓ **Bind 490 / WaitFor 45** — most satellites hit **after** producer Done → Bind theater, not early WaitFor.

**Counter evidence (N=1 Soft=0 nonempty 98):** aborts SF **3578** vs OCC **3272** (median SF/OCC abort ratio **1.0**). WaitFor↑ coexists with abort≈OCC.

### Why R1 = 0 (R1a≈0, R1b=0)

| Counter | N=1 all nonempty | Named 14689597 N=1 | 14689597 N=3 (last-iter metrics) |
|---------|-----------------:|-------------------:|---------------------------------:|
| R1a `rebind_only` | **4** (2 blocks) | **0** | **0** |
| R1b `rewind_to_cp` | **0** | **0** | **0** |
| Soft | **0** | **0** | **0** |

Top mechanisms (from codepath §5, measured here):

1. **WaitFor = Aborting + FullRetry, not R1 progress** — strip on ℓ does not convert validate into RebindOnly.
2. **`covers_all` dies on Spec siblings** — per-ℓ cert; mixed invalid → protocol B0 (`repair.rs` / `validate_specfence`).
3. **R1a `prior_read_value_stable` is incarnation-strict** — writer N→N+1 after wake fails even on same U256; `validate_specfence` lacks museum identity/snap fallback.
4. **Quiet-OCC + EV starve early WaitFor** — PE/intra heat required; first RAW still OCC-abort.
5. **Same-incarnation Blocking retry clears certs** — `begin_execute(inc==0)` wipe; `inc>0` keeps strips but (2)+(3) still kill R1.

Honesty falsifier: WaitFor fires while **R1a stays ~0** and **R1b=0 everywhere**.

---

## 1. Headline numbers vs prior honesty

| Metric | Prior claim (impl.md / quiet-OCC) | **This tip Soft=0 measured** |
|--------|----------------------------------:|-----------------------------:|
| Nonempty median SF/OCC N=1 | ~**0.744** | **0.728** (−0.016; bar **>**0.744 **miss**) |
| 14689597 N=3 SF/OCC | ~**0.554** (Bind7/Wait67/R1=0/ab 98 vs 71) | **0.362** (last-iter Bind**500**/Wait34/R1=0/ab 42 vs 48) |
| 14689597 N=1 | 0.440 (Bind11/Wait48) | **0.437** (Bind9/Wait24/ab 33 vs 51) |
| Soft nonzero blocks | 0 | **0** |
| R1a / R1b | 0 | R1a **4** total / R1b **0** |

**Ruthless:** WaitFor primacy vs Bind is **directionally** landed on aggregate N=1 (2260 vs 1895), but median slipped under 0.744 and fan_out N=3 **regressed** vs the 0.554 story. Bind on 14689597 is **race-unstable** (N=1 Bind≈9 vs one-shot/N=3 last-iter Bind hundreds) — Done→Bind fallthrough dominates when producer finishes first.

Quiet cohort (morph quiet+quiet_ish, n=45): median SF/OCC **0.842** — still carries the nonempty median; spine/fan_out pay.

---

## 2. Causal chain (file:fn + counters)

```
empty PE / quiet_fence_off
  → executor::specfence_plant_is_occ / vm::specfence_access_gate early Ok(())
  → first RAW: OCC Spec abort  → learner PE arm (true-k / any-k)
PE-on
  → access_log.note; access_vis (unfinished=!done S2)
  → access_policy::decide
       WaitFor iff unfinished==1 ∧ writer_executing ∧ ev_win
       SerialLane if unfinished>1 ∧ executing ∧ ev_win → often pcc_serial_lane
       Bind rare: unfinished==0 ∧ tip_is_conflict_producer ∧ !bind_tax_losing
       else UnfencedOcc (Spec≡OCC)
  → pcc_wait_for_writer
       Done → note_fence_success; record_edge_bind; occ_unfenced   # Bind count↑
       !Executing → occ_unfenced                                 # no park, often no cert
       Executing → note_fence_success; arm_hard_wait; Blocking(w) # WaitFor count↑
  → pevm try_execute Blocking: add_dependency → Aborting; steal-without-park common
  → wake / FullRetry; certificates.begin_execute
       inc==0 clears strips; inc>0 keeps locs (still !covers Spec siblings)
  → validate_specfence
       !has_cert → validate_occ_kernel B0
       covers_all ∧ value_stable rebind → R1a (almost never)
       else B0  (+ R1b SuffixRepair never default; rewind_to_cp=0)
```

| Counter | Role |
|---------|------|
| `edge_wait_for` | Successful WaitFor park path (Executing writer) |
| `edge_bind` | Bind-decide **or** WaitFor-Done fallthrough Bind |
| `wait_park_count` | Parks observed (4519 N=1 ≫ WaitFor 2260 — other park kinds / retries) |
| `rebind_only` | R1a |
| `rewind_to_cp` | R1b / SuffixRepair arm |
| `occ_aborts` / `full_restart` | B0 reincarnation (SF vs OCC compared in catalog) |
| `soft_wait_arms` | Must stay 0 (held) |

---

## 3. Five mechanisms (folded from codepath) — why certs ≠ R1 wins

### M1 — WaitFor = Aborting + FullRetry, not R1 progress

`vm.rs::pcc_wait_for_writer` → `Err(ReadError::Blocking)` → `pevm.rs` `add_dependency` marks **Aborting**, discards interpreter progress to the WaitFor site. Soft=0; `ParkKind::BlockingOther` often `arm_steal_convert_without_park` → no `resume_intent` → dependents FullRetry from tx head. Post-wake fence ℓ still read via **`occ_unfenced`**. **Net:** producer pin + location strip, same cost class as OCC reincarnation for that tx.

### M2 — `covers_all` dies on Spec siblings

`certificate.rs::note_success` is **per-ℓ**. `covers_all` requires every invalid read on the strip. Typical RS has Spec≡OCC siblings → mixed invalid → `repair_grain` B0 → `validate_specfence` selective rebind cannot clear Spec residuals → fallthrough `validate_occ_kernel`. Process: 14689597 tx29 WaitFor+UnfencedCold on star consumer.

### M3 — R1a value_stable incarnation-strict

Even with `covers_all`, `executor.rs::validate_specfence` requires `prior_read_value_stable` (matching writer **incarnation** in `mv_memory.rs`). Writer abort/republish after WaitFor wake fails R1a on same U256. Museum `try_validate` identity/snap fallbacks are **not** in `validate_specfence`.

### M4 — Quiet-OCC + EV gates starve early WaitFor

`quiet_fence_off` / `prior_pe_fire_wins` / `ev_win` hold WaitFor until abort/park heat. First-wave conflicts OCC-abort then train PE — WaitFor is **post-abort**, not prevent-first. Wrong tip (`compose_unfinished` ≠ true RAW) or Ready producer → Unfenced continue.

### M5 — Same-incarnation Blocking retry clears certs

Design: `begin_execute(..., incarnation>0)` keeps strips (`wait_resume_keeps_location_strips`). Hole: writer already Done → retry same `tx_version` with **inc==0** → `st.clear(false)` wipes strip. Kept strips on true `inc>0` still lose under M2+M3.

---

## 4. Why abort≈OCC after WaitFor↑ (mechanisms A–G compressed)

| Code | Effect on abort count |
|------|------------------------|
| A Quiet/PE gate | First aborts identical to OCC |
| B Race after wake | Reader origin N, writer N+1 → validate abort |
| C Wrong producer | Park useless; real ℓ Spec-aborts |
| D Spec siblings | B0 despite WaitFor strip |
| E Cert clear inc==0 | Strip gone before validate |
| F Aborting-shaped WaitFor | Counts as SF abort/reexec like OCC |
| G Done/Ready fallthrough | No park; Spec/ESTIMATE path |

Measured: abort_sf/occ median **1.0**; WaitFor>0 blocks still hug OCC abort counts (e.g. 5283152 8=8, 19929064 17=17, 15538827 46≈45).

---

## 5. Named / focus N≥3 (Soft=0)

| Block | N=3 sf_occ | Wait | Bind | ab SF/OCC | R1a/R1b |
|------:|-----------:|-----:|-----:|----------:|--------:|
| 19807137 | **0.209** | 438 | 318 | 782/571 | 5/0 |
| 14689597 | **0.362** | 34 | 500† | 42/48 | 0/0 |
| 15274915 | 0.388 | 30 | 6 | 29/30 | 0/0 |
| 19606597 | 0.686 | 26 | 14 | 41/45 | 0/0 |
| 19606599 | 0.704 | 48 | 70 | 75/84 | 0/0 |
| 19469097 | 0.681 | 84 | 72 | 105/98 | 0/0 |
| 2179522 | 13.2‡ | 0 | 0 | 0/1 | 0/0 |

† Last-iter metrics (sweep stores last of N=3); Bind flood = Done→Bind race, not stable Bind-rare.  
‡ OCC-slow N=1/N=3 noise — do not advertise.

19807137: WaitFor **heavy** and aborts **worse** than OCC (782 vs 571) — WaitFor tax + B0 cascade, not Avoid win.

---

## 6. Not measured

- Per-validate `covers_all` fail reason histogram (need probe).
- Exact steal-without-park vs `park_with_kind` fraction per WaitFor.
- Value-stable fail vs covers_all fail split on R1a attempts.
- True schedule refuse counts (`ready_edges.refuse_count`) in sweep JSON (not in `metrics_json`).
- N=3 **median metrics** across iters (sweep exports last-iter metrics only).
- Seq≡par re-check this tip (prior held; not re-run).

---

## 7. One-line verdict

**WaitFor↑ is real telemetry; abort≈OCC and R1≈0 because WaitFor is Aborting+FullRetry with per-ℓ certs that never `covers_all`+value_stable under Spec siblings and writer reincarnation — Quiet-OCC still owns the first wave.**
