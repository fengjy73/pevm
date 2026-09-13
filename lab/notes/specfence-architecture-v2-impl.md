# SpecFence architecture v2 — implementation map

**Date:** 2026-09-13  
**Branch:** `cursor/specfence-complete-cc-63b0`  
**SoT:** `lab/notes/specfence-complete-architecture-v2.md`  
**Evidence:** `lab/notes/specfence-all-blocks-deep-evidence.md`  
**Vocab:** Spec = Region; Fence = Bind / WaitFor / serial-lane+admit; Unfenced = optimistic

**Verdict:** single-iteration full land. No P0/P1/P2 remainder. Every SoT item below is **landed**.

---

## SoT item → file:fn (all landed)

| # | SoT item | file:fn | Status |
|---|----------|---------|--------|
| 1 | R1-first Resolve when `identity_stable_match` / FF value-stable | `crates/pevm/src/pevm.rs::try_validate` (`r1_eligible`); `rem.rs::identity_stable_match` / `value_stable_match` | **landed** — value-stable (snap/FF/prior) is the R1 door; `identity_held` without a value match is not R1 on `true_suffix` (that accepted stale suffix writes / seq≠par). `r1_first_bias` only widens `!true_suffix` Estimate-cleared identity. |
| 2 | Incarnation-stable residual / cold carry (tx72-class) | `rem.rs::PartialRetryState::reset` (`inc_carry_seen` / `inc_carry_snap`); `PartialRetryTable::inc_carry_seen`; `vm.rs` Unfenced→`bind_done_residual` | **landed** — carry map/snap survive `reset`; Unfenced→residual Bind only when `sketch.residual_bind` or `force_writer` (not every prior UnfencedCold). |
| 3 | WaitFor park budget + PreferAdmit heat (no SoftWait) | `scheduler.rs::admit_spine_heat` / `admit_spine_writers_heat`; `vm.rs::maybe_wait_specfence` + `fence_wait_for`; `learner.rs::note_park_heat` / `prefer_admit_heat` | **landed** |
| 4 | Dissolve `choose_edge_action` OR-salad → version-visibility SM | `edge.rs::classify_edge` → `EdgeVisibility` → `choose_edge_action` | **landed** |
| 5 | Wire `park_ns` + rewind:rebind into structural learners (read by Fence/admit/Resolve) | `learner.rs::note_park_heat` / `note_resolve_r1` / `note_resolve_r2` / `r1_first_bias` / `prefer_admit_heat`; read in `try_validate`, `maybe_wait_specfence`, `admit_spine_heat` | **landed** |
| 6 | Delete AEC / AdaptiveParams / SoftWait theater from live paths | `mod.rs::choose_resolve` dead; `pevm.rs` no `is_storm` / no `abc_top_storm` / no abort `maybe_flip_mode`; SoftWait not armed on access | **landed** |
| 7 | Metric↔L1 morph calibration safe decay | `learner.rs::morph_hat` (fan_out needs abort evidence); `InterBlockPrior::end_block` damps quiet→fan_out | **landed** |
| 8 | Protect quiet Fence-off | `learner.rs::quiet_fence_off`; `sketch.rs::seed_from_prior_morph` skips fanout-only under quiet; collapse/absorb/repair-await gated on `!quiet_fence_off` | **landed** |
| 9 | Generalize `fanout_fr_collapse` / absorb (19807137 / 19434587 class) | `pevm.rs::scan_invalid_spine` / `structural_spine_hot` — executing spine + fan≥2, no bn / no Storm / no fan≥8 hardcode | **landed** |

Roadmap rewrite (no P0/P1/P2 table): `lab/notes/specfence-complete-architecture-v2.md` §12.

---

## Hard-ban checklist

| Ban | This cut |
|-----|----------|
| SoftWait storms | Soft not armed on live Fence path; park is BlockingOther + steal |
| EV Await doors / AdaptiveParams-as-θ | `choose_resolve` / `choose_action` not on access or resolve |
| tip-identity Bind gate | Bind on published Data (A3) unchanged |
| OCC-retry as control plane | Discovery / R4 failure only |
| Morph Storm/Quiet as edge actuator | `is_storm()` gone from `try_validate`; engagement is decay-only label |
| 597 / bn hardcodes | fan≥32 first-repair yield removed; collapse uses structural fan≥2 |
| Gate salad / OR-bool π | `classify_edge` visibility machine |
| Dead AEC theater on live path | Finished |
| Celebrating abort↓ | Bar remains SF/OCC wall vs OCC @8 |
| Unfenced as hang-freedom | Known essential → Bind or WaitFor |

---

## Control loop (live)

```
begin_block:
  seed sketch from InterBlockPrior with flip/quiet decay (fanout-only skipped if quiet)
  engagement := decay-only label

per access EdgeKey:
  verb := classify_edge → Bind | WaitFor | Unfenced*
  if WaitFor: PreferAdmit Ready spines; park_heat skips execution_idx fetch_min
  if inc_carry_seen: residual Bind (never UnfencedCold)
  if Done∅Data under Avoid: residual Bind

per validation abort:
  if identity_stable / FF / certified prefix → R1 RebindOnly
  else if prefix held → R2 SuffixRepair + carry residual map
  else → R3/R4 rare
  collapse/absorb when structural_spine_hot (not Storm)

end_block:
  pack_top_locations → InterBlockPrior EMA (quiet→fan_out damped)
  morph hat for decay only
```

---

## Tests

| Suite | Expect |
|-------|--------|
| `edge.rs` | visibility machine; hot-alone Unfenced; existing A1–A4/D6 |
| `learner.rs` | park/r1 bias readable; quiet_fence_off; morph_hat no over-fan_out; inter-prior damp |
| `rem.rs` | `inc_carry_seen` + snap survive reset |
| `sketch.rs` | quiet fanout-only does not seed H |
| `cargo test -p pevm --lib --release` | green |
| `cargo test -p pevm --test specfence --release -- --test-threads=1` | green |

Sweep JSON: `lab/results/arch-v2-*-sweep.json`. Honesty vs prior median SF/OCC **0.356**.
