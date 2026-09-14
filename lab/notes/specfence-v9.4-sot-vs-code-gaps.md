# SpecFence v9.4 — AUTHORITATIVE SoT vs LIVE code gaps

**Date:** 2026-09-14 (Asia/Shanghai, UTC+8)  
**Tip:** `3687da6` (PR #10)  
**User frame:** wall miss is **incomplete land**, not bad design.  
**SoT stack (AUTHORITATIVE):**
- bars + call-flow: `lab/notes/specfence-complete-architecture-v9.1-cc-pc-bayes.md`
- spine / dual-computer ban: `lab/notes/specfence-complete-architecture-v9.3-pevm-unified.md`
- file-SRP: `lab/notes/specfence-complete-architecture-v9.4-file-srp.md`
- cut map: `lab/notes/specfence-v9-land-brief.md`
- claimed land + honesty: `lab/notes/specfence-v9.4-full-land-impl.md`, `lab/notes/specfence-v9.4-sweep-honesty.md`  
  (median **0.6853**, WaitFor pin/aborting **2061/3280**, R1 **6/1636**, schedule_refuse **0**, Soft=0 held)

**Posture:** audit only — **no plant code changes** in this note. Status = **LANDED** / **PARTIAL** / **MISSING** with `file:fn` evidence. No redesign.

---

## 1. Ruthless gap table

| # | SoT requirement | Status | Live evidence | Gap (incomplete land) |
|---|-----------------|--------|---------------|------------------------|
| A | **Call-order** Bayes → admit → decide → PinHold → Validate/R1 | **PARTIAL** | `pevm.rs` begin_block → `admit::admit_seed_begin_block`; `vm::specfence_access_gate` → `bayes::query_access` + `access_policy::decide_queried` → `fence_act::act_wait_for` / `pcc_wait_for_writer`; validate → `executor::validate_specfence` | Chain symbols exist, but each hop is thin: admit under-seeds; decide still OR-bool; PinHold still `add_dependency`→Aborting; R1 almost never wins. End-to-end plant **not** the SoT call-flow. |
| B | **schedule-first Avoid** (refuse doomed Execute before mid-tx Fence) | **MISSING** | `metrics::record_schedule_refuse` defined; **zero call sites**; sweep `schedule_refuse=0` | Avoid still happens mid-tx (WaitFor/Bind/canary). Schedule refuse is telemetry-only scaffolding — not a plant verb. |
| C | **ProducerStage-safe refuse** (known consumers not ready while `ProducerStage(w)` Executing; deadlock ban held) | **PARTIAL** | `scheduler::try_execute_ready` refuses only when `incarnation > 0` ∧ `is_executing(w)`; Ready/Validated fall through canary; `computer::next_sf_task` promotes reserved stages | **First incarnation never refused** — satellites Execute before edges/refuse matter. Ready-producer consumers Spec-canary instead of refuse. Deadlock ban held; schedule-first admit **not**. |
| D | **Bind rare** (WaitFor/refuse ≫ Bind; Bind-after-Done <10% star) | **PARTIAL** | `access_policy::decide_queried` Bind gate (`tip_is_conflict_producer` ∧ `bind_ev` ∧ `!bind_tax_losing`); Done→`FenceAct::DoneUnfenced` no longer `note_bind_success` (`vm::pcc_wait_for_writer`) | N=1 agg Bind **2208** ≈ WaitFor **2151** — Bind not rare. `bind_after_done` **205** (share 0.0928) still counted; fan **14689597** N=3 Bind **472** vs Wait **47**. Path fixed; volume not. |
| E | **true-k** (PE / abort train at stream ordinal, never residual-1 / template spray) | **PARTIAL** | `access_log::note` on PE-on gate; `feeder::observe_abort` + `learner::note_abort_access(loc_k)`; template `[1,6,10,20]` banned in learner comments/tests | `loc_k` often `None` → any-k arm; ESTIMATE path must not mark PE (good) but first-wave PE still late. Fan star `k≈6` admit before Execute is not guaranteed on empty InterPrior (N=1). |
| F | **cert survival** (strips survive PinHold / same-tx resume; wipe only `begin_block`) | **LANDED** | `certificate::begin_execute` no longer clears `locs` on `inc==0`; `begin_block` only wipe; unit test “M5: same-incarnation resume must not wipe strips” | Strip survival holds. Downstream R1 still fails for other reasons (identity/value / covers). |
| G | **decide ← Bayes** (EV/liveness/depth shape WaitFor; no OR-bool-only π) | **PARTIAL** | `bayes::query_access` returns `ev_pin_beats_abort`, `depth_frac`, `known_star`, `ev_bind_beats_b0`; `vm` passes `Some(bayes_q)` into `decide_queried` | Live π still `ev_win = known_star \|\| (!quiet_off && (intra \|\| (fan && prior_pe_fire_wins)))`. **`ev_pin_beats_abort` / `depth_frac` unused** in decide body (`access_policy.rs`). Bayes is a side door, not the decide spine. |
| H | **no dual computer** (SpecFence always one spine; cold = Spec cost class, not `next_occ_task` retreat) | **LANDED** (with footnote) | `pevm` worker: SpecFence → `next_sf_task` / `validate_specfence`; `specfence_cost_class_spec` = empty PE only; `Occ` mode separate | Footnote: `wave_ref` None still falls to `next_occ_task` (should not happen on SpecFence product path). Deprecated alias `specfence_plant_is_occ` remains but is cost-class, not schedule retreat. |
| I | **Validate → R1 live** (fenced RAW + tip snap; R1 win ≥50% cert-bearing; R1b when EV) | **PARTIAL** | `validate_specfence`: `covers` / selective; `identity_stable_match` ∨ `prior_read_value_stable`; `record_r1_win` / `record_r1_attempt`; else `validate_occ_kernel` B0 | Attempts **1636**, wins **6** (rate **0.0037**). No Bayes covers/EV query at validate. R1b SuffixRepair lives in `rem` / `pevm` lean path — **not** wired as SpecFence validate grain. Fail → almost always B0. |
| J | **PinWithoutThrow / PinHold Stage** (high depth_frac → pin **without** Aborting+FullRetry) | **PARTIAL** | `fence_act::FenceAct::PinHold`; `ParkKind::PinHold`; pevm Blocking arm skips steal-convert on PinHold; `record_waitfor_pin` | **Every** Blocking path still calls `scheduler::add_dependency` → status **`Aborting`** then incarnation++ wake. PinHold is a **park kind label**, not PinWithoutThrow. ESTIMATE/`fence_wait_for` still arms `ParkKind::BlockingOther` (`vm.rs` ~1032) → `waitfor_aborting` **3280** > pin **2061**. |
| K | **admit_seed before satellite Execute** (known-star ReadyEdges @ true-k; ProducerStage reserved) | **PARTIAL** | `admit_seed_begin_block`: `feeder::seed_known_stars` + OrderedAdmit hints ≥16 txs when fan/star; `admit_seed_on_abort` | No location-true-k consumer←producer edges from PE alone. First-block / empty prior: edges arrive post-abort. Mid-tx `access_vis` / WaitFor / ESTIMATE still `note_consumer` (`vm.rs` ~582, ~833, ~721) — refresh+first-insert bleed. |
| L | **dual π deleted from hot path** | **LANDED** | `edge::choose_edge_action`, `resolve::choose_action`, `bayes::{decide,should_wait_hard}` are `#[cfg(test)]`; `mode.rs` deleted; `kernel` `#[cfg(test)]` | Bodies remain as museums (OK if gated). Hot export killed. |
| M | **file-SRP leftovers** (gods split; rem→wave only; learner feeder≠decide; museums quarantined) | **PARTIAL** | Extracted: `fence_act.rs`, `wave.rs` (~531), `admit.rs`, `feeder.rs` (56). Soft=0 held. | **Leftovers:** `vm.rs` **3273** still hosts `fence_wait_for` BlockingOther + large Fence body; `learner.rs` **1758** megaclass (PE/morph/tax/quiet); `rem.rs` **2797** SoftWait Soft + SuffixRepair still product-compiled; `boundary.rs` **3548** / `finegrain.rs` **1980** still `mod` + pub surface; no cfg-gate on museums. |

**Legend:** LANDED = SoT duty holds on product path. PARTIAL = symbols/path exist but SoT semantics incomplete. MISSING = SoT verb absent or never fires (sweep proves).

---

## 2. Top gaps that explain the honesty wall

Sweep @ `3687da6`: nonempty median **0.6853** (base 0.728, Δ−0.043); WaitFor pin/aborting **2061/3280**; R1 **6/1636**; schedule_refuse **0**; Soft=0; fan **14689597** N=3 **0.3482**.

### 2.1 Why `waitfor_aborting` stays high (3280 > pin 2061)

1. **PinHold is not PinWithoutThrow.** `pevm.rs` Blocking arm always `add_dependency` → `IncarnationStatus::Aborting` before park (`scheduler.rs:add_dependency`). SoT M1 (“park WITHOUT Aborting”) is not landed — only steal-convert is gated off for `ParkKind::PinHold`.
2. **ESTIMATE / legacy `fence_wait_for` still arms `BlockingOther`** (`vm.rs` ~1029–1035). Those count as `waitfor_aborting` and keep Aborting+steal-convert theatre.
3. **schedule-first refuse never fires** (`schedule_refuse=0`). Consumers reach mid-tx WaitFor/ESTIMATE instead of being kept out of ready while producer Executing — especially **incarnation 0** (`try_execute_ready` refuse gated on `incarnation > 0`).
4. **decide does not consume `ev_pin_beats_abort` / `depth_frac`.** High-depth WaitFor shape is not Bayes-driven; AbortingThrow is not “last”.

Net: WaitFor volume ↑ without abort↓ — exact SoT falsifier (WaitFor↑ ∧ abort≈OCC).

### 2.2 Why R1 is 6/1636

1. **Cert strips survive (F LANDED) but Resolve rarely converts.** `validate_specfence` requires `value_stable` (identity snap **and** current Data / prior value) before rebind; most attempts fall through to `validate_occ_kernel` B0 after `record_r1_attempt`.
2. **No Bayes at validate** — SoT port “Validate/Repair ← P(covers_all∣strips), tip snap prior” missing; covers is strip-membership only.
3. **R1b / selective grain not SpecFence-default.** `repair::RepairGrain` exists; live SpecFence validate does not drive SuffixRepair from EV. Lean SuffixRepair in `pevm`/`rem` is a parallel museum path, not the fused R1 SoT.
4. **Fan certs are Bind/WaitFor theatre on wrong timing** — DoneUnfenced certs + canary Spec siblings → `covers_all` false (M2 class) → selective empty → B0.

Token R1 path (attempt>0) without win rate = SoT falsifier.

### 2.3 Why median is 0.685 (and below base 0.728)

Causal chain (incomplete land, not redesign):

```
admit under-seed (K PARTIAL)
  → first-wave satellites Execute (C refuse inc>0 only; B refuse MISSING)
    → mid-tx WaitFor/Bind/canary + ESTIMATE BlockingOther (J PARTIAL)
      → add_dependency Aborting mass (waitfor_aborting 3280)
        → FullRetry / B0 cascade; R1 almost never saves (I PARTIAL, 6/1636)
          → useful_EVM↓ + idle/repair↑ → SF/OCC median 0.685
```

Extra tax vs base: PinHold park **plus** Aborting dependency (double cost) without schedule refuse or live R1 payoff — call-order scaffolding added meta without completing the Avoid/Resolve ends.

Named blocks match: **19807137** Wait/pin/aborting dominate (N=3 wait 442 / aborting 937 / sf_occ 0.23); **14689597** Bind-heavy + R1 0/42 (sf_occ 0.35).

---

## 3. Ordered full-batch land list — MISSING / PARTIAL only

**Ban:** redesign, new π, SoftWait Soft, folder `pc/cc/bayes`, patch-salad one-offs.  
**Do:** finish SoT duties already named in v9.1/v9.3/v9.4 on the unified spine.

| Order | Item | Status | Concrete land (existing files/fns) | Done when |
|------:|------|--------|-------------------------------------|-----------|
| 1 | **Schedule-first refuse on incarnation 0** | MISSING/PARTIAL | `scheduler::try_execute_ready`: drop `incarnation > 0` gate for known ReadyEdge consumers when `ProducerStage(w)` Executing; call `metrics::record_schedule_refuse` on defer | sweep `schedule_refuse ≫ 0`; first-wave satellites not Executing while star producer Executing |
| 2 | **PinWithoutThrow = no Aborting** | PARTIAL | Blocking arm: PinHold path must **not** `add_dependency`→Aborting (park Stage / dependency without status Aborting, or dedicated pin wake that keeps incarnation); keep steal-convert ban | `waitfor_aborting ≪ waitfor_pin`; AbortingThrow rare on high depth_frac |
| 3 | **Kill / retarget BlockingOther WaitFor as default Avoid** | PARTIAL | `vm::fence_wait_for` ESTIMATE path: either route PE-known RAW through PinHold/refuse, or keep BlockingOther only for true unknown ESTIMATE — not counted as SoT WaitFor Aborting default | waitfor_aborting collapses toward OCC baseline |
| 4 | **admit_seed true-k star edges before Execute** | PARTIAL | `admit::admit_seed_begin_block` + `feeder::seed_known_stars`: seed ReadyEdge consumer←producer for PE/HotSet/InterPrior stars at `k_template`/true-k, not only hints≥16 OrderedAdmit | 14689597 consumers←38 edged before satellite Execute on warm prior; Bind-after-Done star share <10% with refuse/pin dominant |
| 5 | **decide consumes Bayes EV** | PARTIAL | `access_policy::decide_queried`: WaitFor shape from `ev_pin_beats_abort` / `depth_frac` / liveness; demote OR-bool `ev_win` to adapter | Bind rare; AbortingThrow last; Bayes unused falsifier closed |
| 6 | **Validate R1 converts certs** | PARTIAL | `executor::validate_specfence`: tip snap / value-stable path that actually wins; query Bayes covers prior; wire selective R1 + R1b when EV says so; ban silent fallthrough to always-B0 while strips exist | R1 win rate ≥50% on cert-bearing fan_out attempts (or honest attempt definition matches SoT) |
| 7 | **Stop mid-tx first ReadyEdge insert bleed** | PARTIAL | `vm::access_vis` / WaitFor / ESTIMATE: refresh-only unless producer already predicted **and** admit_seed owned the edge; abort strengthen stays `admit_seed_on_abort` | admit_seed is sole first insert (sweep/falsifier) |
| 8 | **File-SRP leftovers close** | PARTIAL | Move remaining Fence Aborting policy out of `vm` into `fence_act` (or thin wrapper); cfg/quarantine SoftWait Soft arms in `rem`; keep `boundary`/`finegrain` off default product surface; trim `learner` to store+feeder ports (decide already out) | gods no longer host Aborting WaitFor body; museums not default-compiled; **not** “create pc/cc/bayes dirs” |
| 9 | **Bind volume follow-through** | PARTIAL | After 1–5: Bind only tip==conflict ∧ Bayes EV; DoneUnfenced cert-without-Bind kept | Bind ≪ WaitFor/refuse; star Bind-after-Done <10% |
| 10 | **true-k close** | PARTIAL | Ensure abort/PE always carry `access_log::first_k` when stream noted; no any-k residual when ordinal known | fan PE at k≈6 not residual-1 |

**Already LANDED — do not re-land:** cert strip survival (F); SpecFence spine unity / no `plant_is_occ`→`next_occ_task` (H); dual-π hot delete + `mode.rs` gone + kernel test-only (L); Soft=0; wave extract + admit/feeder/fence_act file split (partial S0 — leftovers in row 8).

**Ship rule (SoT):** one coherent batch finishing rows 1–6 at minimum before claiming call-order land. Partial land that raises WaitFor/Bind without abort↓ **and** R1 win = non-land.

---

## 4. Essence

v9.4 PR #10 landed **scaffolding** (spine, file splits, ports, PinHold label, R1 attempt counter, cert survival). Honesty wall (median 0.685, aborting WaitFor, R1≈0, refuse=0) is **incomplete SoT land**: schedule-first Avoid never fires, PinHold still Aborting, decide still OR-bool, R1 almost never converts. Finish the named duties — do not redesign.
