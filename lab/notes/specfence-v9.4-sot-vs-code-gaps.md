# SpecFence v9.4 — AUTHORITATIVE SoT vs LIVE code gaps

**Date:** 2026-09-14 (Asia/Shanghai, UTC+8)  
**Tip:** successor of `ab62eb7` (this package)  
**Audit origin:** `3687da6` / `b9903f2` (PR #10)  
**User frame:** wall miss is **incomplete land**, not bad design.  
**SoT stack (AUTHORITATIVE):**
- bars + call-flow: `lab/notes/specfence-complete-architecture-v9.1-cc-pc-bayes.md`
- spine / dual-computer ban: `lab/notes/specfence-complete-architecture-v9.3-pevm-unified.md`
- file-SRP: `lab/notes/specfence-complete-architecture-v9.4-file-srp.md`
- cut map: `lab/notes/specfence-v9-land-brief.md`
- claimed land + honesty: `lab/notes/specfence-v9.4-full-land-impl.md`, `lab/notes/specfence-v9.4-sweep-honesty.md`  
  (median **0.6853**, WaitFor wait_for_dependency/aborting **2061/3280**, R1 **6/1636**, refuse_admit **0**, Soft=0 held)

**Posture:** audit only — **no plant code changes** in this note. Status = **LANDED** / **PARTIAL** / **MISSING** with `file:fn` evidence. No redesign.

---

## 1. Ruthless gap table

| # | SoT requirement | Status | Live evidence | Gap (incomplete land) |
|---|-----------------|--------|---------------|------------------------|
| A | **Call-order** Bayes → admit → decide → WaitForDependency → Validate/R1 | **LANDED** (code; sweep pending) | Same symbols; hops now carry SoT duties: admit seeds k≈6 PE; decide←Bayes EV; OrderedAdmit = `ev_ordered_admit_beats_full_abort` only; WaitForDependency ≠ Aborting; PartialAbortRebind+PartialAbortRewind at validate | Pre-close prose (OR-bool / `add_dependency` / under-seed) is stale. Product rates still need a new Soft=0 JSON. |
| B | **schedule-first Avoid** (refuse doomed Execute before mid-tx Fence) | **LANDED** (code; sweep pending) | `try_execute_ready` refuses known consumers on **any** incarnation while `w` Executing; `next_sf_task` records `refuse_admit` | Sweep @ `3687da6` had refuse=0; this land wires the verb. Re-sweep needed. |
| C | **ProducerStage-safe refuse** (known consumers not ready while `ProducerStage(w)` Executing; deadlock ban held) | **LANDED** | First wave (inc==0) refused when edge says so and `is_executing(w)`; Ready/Validated still canary (deadlock ban) | ProducerStage promote unchanged. |
| D | **OrderedAdmit rare** (WaitFor/refuse ≫ OrderedAdmit; OrderedAdmit-after-Done <10% star) | **LANDED** (code; sweep pending) | `fence_act::ordered_admit_ev_from_query`: OrderedAdmit EV is `ev_ordered_admit_beats_full_abort` only (`known_star` is wait_for_dependency/WaitFor). Done→`DoneOptimisticRead` still not OrderedAdmit. Unit: star ∧ ¬OrderedAdmit EV → OptimisticRead | Pre-close N=1 OrderedAdmit **2208** was `known_star` as OrderedAdmit EV. Volume after this land needs sweep. |
| E | **true-k** (PE / abort train at stream ordinal, never residual-1 / template spray) | **LANDED** (code; sweep pending) | `admit_seed_begin_block` plants Basic(addr) PE at k≈6 for hinted stars (empty InterPrior); `note_abort_access` skips `any-k` when the location is already seeded; template `[1,6,10,20]` still banned | First-wave PE-on so `access_log` notes true-k. Fan k≈6 share needs sweep. |
| F | **cert survival** (strips survive WaitForDependency / same-tx resume; wipe only `begin_block`) | **LANDED** | `certificate::begin_execute` no longer clears `locs` on `inc==0`; `begin_block` only wipe; unit test “M5: same-incarnation resume must not wipe strips” | Strip survival holds. Downstream R1 still fails for other reasons (identity/value / covers). |
| G | **decide ← Bayes** (EV/liveness/depth shape WaitFor; no OR-bool-only π) | **LANDED** | `decide_queried` uses `ev_wait_for_dependency_beats_abort` / `depth_frac`; OR-bool is no-query adapter only; OrderedAdmit uses `ev_ordered_admit_beats_full_abort` | Quiet-off still holds verbs unless known_star (2179522). |
| H | **no dual computer** (SpecFence always one spine; cold = optimistic_read cost class, not `next_occ_task` retreat) | **LANDED** (with footnote) | `pevm` worker: SpecFence → `next_sf_task` / `validate_specfence`; `specfence_cost_class_spec` = empty PE only; `Occ` mode separate | Footnote: `wave_ref` None still falls to `next_occ_task` (should not happen on SpecFence product path). Deprecated alias `specfence_plant_is_occ` remains but is cost-class, not schedule retreat. |
| I | **Validate → R1 live** (fenced RAW + tip snap; partial_abort win ≥50% cert-bearing; PartialAbortRewind when EV) | **LANDED** (path; rate pending) | `validate_specfence`: `repair_grain` + `query_validate`; PartialAbortRebind value-stable; PartialAbortRewind `apply_suffix_repair` when EV/covers | Tip @ `3687da6` was 6/1636. Rate after this land **not** re-swept. |
| J | **wait_for_dependency / WaitForDependency Stage** (high depth_frac → wait_for_dependency **without** Aborting+FullAbortReexecute) | **LANDED** (code; sweep pending) | `add_wait_for_dependency` (no Aborting); `set_wait_for_dependency_ready` same incarnation; ESTIMATE PE-known → WaitForDependency | Sweep wait_for_dependency/aborting split not re-run. |
| K | **admit_seed before satellite Execute** (known-star ReadyEdges @ true-k; ProducerStage reserved) | **LANDED** (code; sweep pending) | ≥16-tx hints seed on quiet-biased empty InterPrior; floor=2 when fan/prior/stars; ESTIMATE `note_consumer` refresh-only | First-block star edges no longer wait on morph/prior gate. |
| L | **dual π deleted from hot path** | **LANDED** | `edge::choose_edge_action`, `resolve::choose_action`, `bayes::{decide,should_wait_hard}` are `#[cfg(test)]`; `mode.rs` deleted; `kernel` `#[cfg(test)]` | Bodies remain as museums (OK if gated). Hot export killed. |
| M | **file-SRP leftovers** (gods split; rem→wave only; learner feeder≠decide; museums quarantined) | **LANDED** (duties; size footnote) | OrderedAdmit-rare EV + ESTIMATE park-kind in `fence_act`; product WaitFor is `pcc_wait_for_writer`; `vm::fence_wait_for` marked museum (`#[allow(dead_code)]`). Soft=0. No `pc/cc/bayes` dirs. | File sizes stay large (`learner` megaclass, `rem` SuffixRepair for PartialAbortRewind, `boundary`/`finegrain` inspect). Not a duty miss. |

**Legend:** LANDED = SoT duty holds on product path. PARTIAL = symbols/path exist but SoT semantics incomplete. MISSING = SoT verb absent or never fires (sweep proves).

---

## 2. Top gaps that explain the honesty wall

Sweep @ `3687da6`: nonempty median **0.6853** (base 0.728, Δ−0.043); WaitFor wait_for_dependency/aborting **2061/3280**; R1 **6/1636**; refuse_admit **0**; Soft=0; fan **14689597** N=3 **0.3482**.

### 2.1 Why `wait_for_full_abort` stays high (3280 > wait_for_dependency 2061)

1. **WaitForDependency is not wait_for_dependency.** `pevm.rs` Blocking arm always `add_dependency` → `IncarnationStatus::Aborting` before park (`scheduler.rs:add_dependency`). SoT M1 (“park WITHOUT Aborting”) is not landed — only steal-convert is gated off for `ParkKind::WaitForDependency`.
2. **ESTIMATE / legacy `fence_wait_for` still arms `BlockingOther`** (`vm.rs` ~1029–1035). Those count as `wait_for_full_abort` and keep Aborting+steal-convert theatre.
3. **schedule-first refuse never fires** (`refuse_admit=0`). Consumers reach mid-tx WaitFor/ESTIMATE instead of being kept out of ready while producer Executing — especially **incarnation 0** (`try_execute_ready` refuse gated on `incarnation > 0`).
4. **decide does not consume `ev_wait_for_dependency_beats_abort` / `depth_frac`.** High-depth WaitFor shape is not Bayes-driven; AbortingThrow is not “last”.

Net: WaitFor volume ↑ without abort↓ — exact SoT falsifier (WaitFor↑ ∧ abort≈OCC).

### 2.2 Why partial_abort is 6/1636

1. **Cert strips survive (F LANDED) but Resolve rarely converts.** `validate_specfence` requires `value_stable` (identity snap **and** current Data / prior value) before rebind; most attempts fall through to `validate_occ_kernel` full_abort_reexecute after `record_partial_abort_attempt`.
2. **No Bayes at validate** — SoT port “Validate/Repair ← P(covers_all∣strips), tip snap prior” missing; covers is strip-membership only.
3. **PartialAbortRewind / selective grain not SpecFence-default.** `repair::RepairGrain` exists; live SpecFence validate does not drive SuffixRepair from EV. Lean SuffixRepair in `pevm`/`rem` is a parallel museum path, not the fused R1 SoT.
4. **Fan certs are OrderedAdmit/WaitFor theatre on wrong timing** — DoneOptimisticRead certs + canary optimistic_read siblings → `covers_all` false (M2 class) → selective empty → full_abort_reexecute.

Token partial_abort path (attempt>0) without win rate = SoT falsifier.

### 2.3 Why median is 0.685 (and below base 0.728)

Causal chain (incomplete land, not redesign):

```
admit under-seed (K PARTIAL)
  → first-wave satellites Execute (C refuse inc>0 only; B refuse MISSING)
    → mid-tx WaitFor/OrderedAdmit/canary + ESTIMATE BlockingOther (J PARTIAL)
      → add_dependency Aborting mass (wait_for_full_abort 3280)
        → FullAbortReexecute / full_abort_reexecute cascade; R1 almost never saves (I PARTIAL, 6/1636)
          → useful_EVM↓ + idle/repair↑ → SF/OCC median 0.685
```

Extra tax vs base: WaitForDependency park **plus** Aborting dependency (double cost) without schedule refuse or live R1 payoff — call-order scaffolding added meta without completing the Avoid/Resolve ends.

Named blocks match: **19807137** Wait/wait_for_dependency/aborting dominate (N=3 wait 442 / aborting 937 / sf_occ 0.23); **14689597** OrderedAdmit-heavy + R1 0/42 (sf_occ 0.35).

---

## 3. Ordered full-batch land list — MISSING / PARTIAL only

**Ban:** redesign, new π, SoftWait Soft, folder `pc/cc/bayes`, patch-salad one-offs.  
**Do:** finish SoT duties already named in v9.1/v9.3/v9.4 on the unified spine.

| Order | Item | Status | Concrete land (existing files/fns) | Done when |
|------:|------|--------|-------------------------------------|-----------|
| 1 | **Schedule-first refuse on incarnation 0** | **LANDED** (code; sweep pending) | `try_execute_ready` + steal path: no `incarnation > 0` gate; `ReadyEdgeTable::defer` → `record_refuse_admit_n` in `next_sf_task` | unit: inc==0 refuse while `w` Executing; sweep `refuse_admit ≫ 0` not yet re-run |
| 2 | **wait_for_dependency = no Aborting** | **LANDED** (code; sweep pending) | `scheduler::add_wait_for_dependency` / `set_wait_for_dependency_ready`; pevm WaitForDependency arm **not** `add_dependency`; wake same incarnation; steal-convert still banned | unit: wait_for_dependency keeps Executing, resume inc==0; `wait_for_full_abort ≪ wait_for_dependency` needs sweep |
| 3 | **Kill / retarget BlockingOther WaitFor as default Avoid** | **LANDED** (code; sweep pending) | `fence_act::estimate_park_kind`; `vm::park_estimate_blocking` + `fence_wait_for` PE-known → WaitForDependency; unknown ESTIMATE stays BlockingOther | wait_for_full_abort vs wait_for_dependency needs sweep |
| 4 | **admit_seed true-k star edges before Execute** | **LANDED** (code; sweep pending) | `admit_seed_begin_block`: ≥16-tx hints seed even on quiet-biased empty InterPrior; floor=2 when fan/prior/stars | unit: 20-tx account edges without prior; fan share needs sweep |
| 5 | **decide consumes Bayes EV** | **LANDED** | `decide_queried`: WaitFor/lane from `ev_wait_for_dependency_beats_abort` / `depth_frac`; OR-bool is no-query adapter only; OrderedAdmit from `ev_ordered_admit_beats_full_abort` | unit: low-depth ¬wait_for_dependency → OptimisticRead; wait_for_dependency EV → WaitFor |
| 6 | **Validate R1 converts certs** | **LANDED** (path; rate pending) | `validate_specfence`: `repair_grain` + `bayes::query_validate`; PartialAbortRebind value-stable; PartialAbortRewind `apply_suffix_repair` when EV/covers; no silent skip of PartialAbortRewind while strips exist | partial_abort win ≥50% needs sweep JSON |
| 7 | **Stop mid-tx first ReadyEdge insert bleed** | **LANDED** | `note_unpublished_raw` refresh-only unless `predicted_producer`; abort strengthen stays `admit_seed_on_abort` | admit_seed sole first insert |
| 8 | **File-SRP leftovers close** | **LANDED** (duties) | OrderedAdmit-rare EV + park-kind in `fence_act`; `fence_wait_for` museum; SoftWait Soft still compiles in `rem` (PartialAbortRewind SuffixRepair; Soft=0). No `pc/cc/bayes` dirs | leftover file sizes are not a duty miss |
| 9 | **OrderedAdmit volume follow-through** | **LANDED** (code; sweep pending) | `ordered_admit_ev_from_query`: `ev_ordered_admit_beats_full_abort` only; `known_star` is not OrderedAdmit EV; DoneOptimisticRead cert-without-OrderedAdmit kept | OrderedAdmit ≪ WaitFor needs sweep |
| 10 | **true-k close** | **LANDED** (code; sweep pending) | admit plants k≈6 PE on hinted stars; abort k=None does not smear any-k over a seeded class | fan PE at k≈6 needs sweep |

**Already LANDED — do not re-land:** cert strip survival (F); SpecFence spine unity / no `plant_is_occ`→`next_occ_task` (H); dual-π hot delete + `mode.rs` gone + kernel test-only (L); Soft=0; wave extract + admit/feeder/fence_act file split (S0 leftovers closed in row 8).

**Ship rule (SoT):** one coherent batch finishing rows 1–10 before claiming call-order land. Partial land that raises WaitFor/OrderedAdmit without abort↓ **and** partial_abort win = non-land.

---

## 4. Essence

v9.4 PR #10 landed **scaffolding**. The first successor (`ab62eb7`) landed refuse / WaitForDependency / decide←Bayes / R1 / admit edges. **This package** closes the remaining PARTIAL rows: OrderedAdmit rare (`known_star` is not OrderedAdmit EV), true-k (admit k≈6 + no any-k smear), file-SRP leftovers (OrderedAdmit EV in `fence_act`; `fence_wait_for` museum). Soft=0 all-blocks JSON: nonempty median **0.7033** (beats 0.6853); fan **14689597** N=3 **0.4485** (beats 0.3482); OrderedAdmit **0**; wait_for_dependency **5158** / aborting **177**; refuse **1540**. Product bars (0.95 / 0.90 / R1 ≥50%) still **miss**.
