# SpecFence v4.1-frozen grain — implementation map

**Date:** 2026-09-13  
**Branch:** `cursor/specfence-frozen-grain-3175` (from `cursor/specfence-complete-cc-63b0` @ `aa8b875`)  
**SoT:** `lab/notes/specfence-complete-architecture-v4-frozen-grain.md`  
**Empirical basis:** `lab/notes/specfence-decision-field-selection-from-99.md`  
**Vocab:** Spec = Region; Fence = Bind / WaitFor / serial-lane / ordered-admit; Unfenced ≡ OCC-cost for **this** access when ¬PredictedEssential.

**Verdict:** single-iteration full land. No P0/P1/P2 remainder. Every SoT §13 land item below is **landed**.

Frozen π (must match code):

```
a = (t, k, depth, ℓ, mode)                    # inc NOT Avoid key
e_vis = (writer?, published_Data?, edge_kind)
gate  = PredictedEssential(ℓ, k, morph) ∨ independence_certified
verb  = Bind | WaitFor|serial-lane|ordered_admit | Unfenced≡OCC
        # THIS a only — never sticky Wait-on-tx
```

---

## SoT item → file:fn (all landed)

| # | SoT §13 item | file:fn | Status |
|---|--------------|---------|--------|
| 1 | Unify Unfenced baseline with OCC path per access; no canary / Edge tax on ¬PredictedEssential | `vm.rs::maybe_wait_specfence` (`predicted` gate; `canary_ok=false`); `edge.rs::classify_edge` / `choose_edge_action` | **landed** |
| 2 | Frozen access-event SoT \(a=(t,k,\mathrm{depth},ℓ,\mathrm{mode})\) + \(e_{\mathrm{vis}}\) + gate; ban flat `(ℓ,reader)`; `inc` not in Avoid key | `edge.rs::EdgeKey` (`location,reader,access_k,depth`); `edge.rs::inc_is_not_in_avoid_key`; `vm.rs` builds `EdgeKey` without incarnation | **landed** |
| 3 | Always-on cheap Detect at every access boundary (L_record / L_access / L_edge) | `vm.rs::maybe_wait_specfence` → `learner.note_detect` + `edges.record`; `metrics.record_detect_access` | **landed** |
| 4 | First-class PredictedEssential(\(ℓ,k,\mathrm{morph}\)) ∨ independence_certified; first-wave per access class; strip AEC/tx-sticky/`inc`/H-OR as π | `learner.rs::predicted_essential` / `mark_predicted_essential` / `seed_predicted_essential` / `note_abort_access`; PE sources = **abort \(k>0\)** + **inter-block abort \(k_template\)** only (`pack_top` / `dominant_k` never use Detect `last_k`); abort marks **per-ℓ** `min_k_of_location` (not tx-min \(k\) on every invalid loc); `sketch.rs::mark_access_class` is serial-lane (not a live OR); `pevm.rs` prior seed (quiet skips); publish does **not** plant PE | **landed** |
| 5 | Timely PCC Avoid at **this** \(a\): Bind / WaitFor / serial-lane / ordered-admit iff PredictedEssential; mixed verbs in one tx | `edge.rs::classify_edge` (PredictedEssential∧Data→Bind; PredictedEssential∧writer→WaitFor; else Unfenced≡OCC even if Data exists); `vm.rs` WaitFor-only `admit_spine_writers_heat` (no PreferAdmit on Unfenced); `fence_wait_for` never `WaitFor(reader-1)` via `force_prefix` | **landed** |
| 6 | Delete exclude-set π (ForcePrefix, tx SoftWait, canary verb, H-OR, morph actuator, writer_validated Bind gate, `inc` Avoid) | `edge.rs::classify_edge` discards exclude-set; `vm.rs` never `try_canary` / never ForcePrefix-Wait; `sketch.rs::essential_antidep` ignores `force_prefix`; SoftWait Soft not armed | **landed** |
| 7 | Hot Region-access serial lane + ordered admission (not fleet WaitFor / wait_no_writer) | `sketch.rs::in_serial_lane` = access-class; `ready_spine_writers` walks access-class spines; classify **never** WaitFor(reader-1) | **landed** |
| 8 | Timely Resolve at grain: RebindThis / CertifiedPrefixSkip / E1 residual; SuffixRepair not default; `inc` bookkeeping only | `pevm.rs::try_validate` R1a value-stable RebindThis (unchanged door); `rem.rs::apply_suffix_repair_planned` RewindTo = PrefixSkip, else **B0 FullRestart** (no ForceBind default) | **landed** |
| 9 | Miss path = OCC residual reincarnation; full B0 only on grain identity loss | `rem.rs::apply_suffix_repair_planned` `FullRestart` when no mid-tx checkpoint | **landed** |
| 10 | Strip dead theater: AEC/AdaptiveParams-as-θ, SoftWait Soft, Storm edge, PreferAdmit-as-primary, canary_reopen, CostGate-only, sticky AvoidBroadcast | `mod.rs::choose_resolve` remains `#[allow(dead_code)]` unused on access/validate; PreferAdmit only on WaitFor; location-wide Avoid is not a live OR; canary_reopen not called | **landed** |
| 11 | Quiet / ¬PredictedEssential protection | `learner.rs::quiet_fence_off` clears prior-only prediction; `sketch.rs::seed_from_prior_morph` skips H **and** access-class on quiet; publish / Detect `last_k` cannot plant PE; Detect still on | **landed** |
| 12 | Falsifier suite | `metrics.rs` `detect_accesses` / `predicted_essential_hits` / `pcc_fire_at_a` + exclude-set counters (target 0); `process.rs::mixed_verb_intra_tx`; sweep JSON fields | **landed** |
| 13 | Docs | this file; frozen SoT §15; v4.0 / pre-freeze v4.1 remain superseded banners | **landed** |

---

## Hard-ban checklist

| Ban | This cut |
|-----|----------|
| SoftWait storms | Soft not armed on access; park is BlockingOther + steal |
| Tx-level SoftWait / sticky Wait-on-tx | Verb scoped to `a`; sibling \(k\) may Unfence |
| Whole-tx ForcePrefix bool as π | `classify_edge` ignores `force_prefix`; no Unfenced→Wait override |
| `inc` as Avoid / sticky key | `EdgeKey` has no incarnation; repair `force_prefix` is observe-only |
| Canary as live verb / UnfencedCold tax | `canary_ok=false`; `try_canary` not called from `maybe_wait` |
| `H` as Wait OR-door | discarded in `classify_edge`; H is observe/prior only |
| Morph as Fence actuator | `wait_depth_prior` no longer `note_hot`s into Avoid |
| `writer_validated` as Bind gate | A3 Data→Bind unchanged |
| Flat `EdgeKey(ℓ,reader)` as SoT | Key keeps `(ℓ,reader,k,depth)` |
| EV Await / AdaptiveParams-as-θ | `choose_resolve` / `choose_action` not on access or validate |
| Clique / location-Avoid OR-salad | dissolved; only `predicted_essential` Fences |
| wait_no_writer ghost pred | no `WaitFor(reader-1)` |
| SuffixRepair / ForceBind as default | PrefixSkip if checkpoint; else B0 |
| 597 / bn hardcodes | none added |
| Unfenced cost > OCC on ¬PredictedEssential | no canary / Edge SM on that path |

---

## Control loop (live)

```
begin_block:
  seed PredictedEssential(ℓ, k_template) from InterBlockPrior if !quiet
    # k_template is abort-derived only (never Detect last_k)
  seed sketch H as observe/prior only (quiet → skip all)

access_tick a = (t, k, depth, ℓ, mode):
  Detect.record(a, e_vis) + learner.note_detect          # ALWAYS (observe)
  gate := PredictedEssential(ℓ, k)                       # abort + prior; not access_class OR
  if PredictedEssential:
    Bind if published_Data else WaitFor(w) + ordered admit
  else:
    Unfenced≡OCC                                         # no canary, no ForcePrefix, no PreferAdmit

resolve_tick / validate:
  R1a RebindThis if value_stable
  R1b CertifiedPrefixSkip if mid-tx checkpoint before k*
  else B0 FullRestart                                    # inc++ bookkeeping only

end_block:
  pack_top_locations includes k_template
  emit falsifiers (soft=0, await=0, exclude-set=0, mixed_verb_intra_tx)
```

---

## Tests

| Suite | Expect |
|-------|--------|
| `edge.rs` | frozen π SM; ForcePrefix/canary/H/clique not verbs; mixed k; A3 Bind |
| `learner.rs` | PredictedEssential per k-class; k=0 does not plant; quiet prior vs intra |
| `sketch.rs` | access-class ≠ location Avoid; canary ≠ independence; quiet no seed |
| `rem.rs` | PrefixSkip still RewindTo; no-checkpoint → B0 not ForceBind |
| `cargo test -p pevm --lib --release` | green |
| `cargo test -p pevm --test specfence --release -- --test-threads=1` | green |

Sweep JSON (gitignored): `lab/results/frozen-grain-focus-worst-quiet-sweep.json`, `lab/results/frozen-grain-all-blocks-sweep.json`. Tip `bff789e`.

### Honesty — all-blocks N=1 @8 (98 nonempty / 99 loaded)

| | This tip | Prior full-set to beat |
|--|----------|------------------------|
| median SF/OCC | **0.351** | ≈0.32–0.36 |
| p10 / min (nonempty) | 0.215 / **0.057** (19807137) | worst ~0.076 |
| mean | 0.98 | — |
| quiet heuristic (34) median | **1.07** (18/34 ≥1; 22/34 bind=0 abort=0) | ~1.10 |
| fan_out (59) median | **0.315** | ~0.298 |

Do **not** claim median ≥0.7 or SoT “then → ≥1.0”. Mean is inflated by N=1 OCC pathology on 19434587 (OCC wall 2278ms / SF 50ms → 45×). Empty snapshot 19910734 (`n_tx=0`) is dropped from the nonempty median; it is not a protocol zero.

Exclude-set counters = 0 on all 99 SF rows (force_prefix_as_π, canary live, inc Avoid, H-OR, morph actuator, writer_validated Bind gate, flat EdgeKey). Soft=0, await=0. Detect recorded 237 622 accesses on 80 blocks. `mixed_verb_intra_tx` on process-trace fan_out: 14689597=494, 19807137=595.

### Honesty — focus+worst+quiet N=3 (8-block worst-heavy set)

Printed median **0.307** / mean **0.407** / min **0.085**. Soft=0.

| Block | Role | SF/OCC | bind | notes |
|------:|------|-------:|-----:|-------|
| 14689597 | focus | 0.345 | 1151 | mixed_verb=494 |
| 19606599 | focus | 0.307 | 1159 | |
| 19469097 | focus | 0.293 | 879 | |
| 19807137 | worst | 0.085 | 6657 | N=3 just above 0.08; N=1 all-blocks 0.057; rewind still dominates wall |
| 6196166 | park | 0.097 | 1361 | still park-heavy |
| 6137495 | worst-ish | 0.210 | 384 | |
| 2179522 | quiet | 1.57 | 53 | N=3 OCC-comparable; N=1 all-blocks 0.40 (variance) |
| 19606598 | quiet neighbor | 0.347 | 109 | not ≡OCC |

Remaining **performance** (not leftover π land): 19807137 / 6196166 still pay PrefixSkip+abort ≫ OCC. Architecture items in §13 are landed.
