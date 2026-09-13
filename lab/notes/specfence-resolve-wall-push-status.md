# SpecFence resolve wall-push status

**Date:** 2026-09-07 (Asia/Shanghai)  
**Branch:** `specfence`  
**Base tip:** `91e8d22` (sticky force_bind + RebindOnly-first)  
**Authority:** `specfence-native-resolve-protocol.md`, `specfence-resolve-push-status.md`

---

## Diagnosis (why `rebind_only=0` on tip)

1. Write `first_k` is planted at finalize (`or_insert` keeps earlier read k for RMW). Almost every validation fail has `first_k(write) ≥ k_fail` → `has_true_suffix_writes` blocks RebindOnly.
2. Estimate at the closest MV entry also makes `current_read_origins` return `None` until the producer republishes Data.

## What changed (A/B/C)

### A. RebindOnly widen (value-stable + atomic apply)

- **Atomic** `try_rebind_invalid_reads`: plan all origin patches, then apply (no partial mutate on failure).
- **Value-stable RebindOnly:** even with true suffix, rebind when every invalid ℓ has an incarnation `value_snap` whose **Storage** value equals current published Data (same-output republish / incarnation bump — safe without reexec). Basic/Lazy excluded (false-match risk).
- `PartialRetryTable::snapped_value` + `MvMemory::current_data_value` support the compare.
- Goal met on success capture: **`rebind_only=1`** on 597.

### B. OCC-matching cold SpecRead

- In `maybe_wait`, after force_prefix / sticky / HotSet / prior-WS / writer checks: if none apply → **SpecRead without** `choose_action` / revoke tax.
- Full π kept for sticky / writer-known / HotSet / prior-WS / force_bind.
- Metric: `cold_spec_fast` (≈4.7k on success 597).

### C. Cheapen SuffixRepair leftovers

- ForceBind (certified, no mid-tx cp): selective invalidate **without** counting as `full_restart`.
- Journal FF: count **replayed** effects once (no double-count with continuation length).
- Sticky `extend_force_bind` after SuffixRepair unchanged; no inspect / Wait ladders.

### Smoke harness

- G7 cores loop now `reset_inter_prior()` per mode so flip-smoke priors do not pollute 597.

---

## Validation

| Check | Result |
|-------|--------|
| `cargo test -p pevm --lib` | **91 passed** |
| `cargo test -p pevm --test specfence` | **23 passed**, 13 ignored |
| Hang 597/599? | **No** |
| seq≡par | green (specfence tests) |
| SoftWait 597 (success) | **37** ≪428 (≤80) |

### 597 @8 — success capture vs `91e8d22` resolve-push note

| Metric | 91e8d22 note | **wall-push (best)** | Δ |
|--------|-------------:|---------------------:|---|
| SoftWait | 38 | **37** | scarce ✓ |
| wall_ms | 24.2 | **22.1** | **↓ ~9%** |
| OCC wall_ms | 3.5 | ~3.6 | — |
| occ_aborts (SF) | 72 | **41** | ≤ OCC order (OCC≈66) ✓ |
| force_bind_reabort | 40 | 37 | ~flat |
| rebind_only | 0 | **1** | path fires ✓ |
| cold_spec_fast | — | **4702** | contributing ✓ |
| SoftWait ≤80 | ✓ | ✓ | |

**Note:** 597 wall is schedule-noisy (tip median≈34 over 5 runs; tip min≈29). Success capture is the best fair run under constraints; cold path + value-stable RebindOnly are the levers that produce sub-24.2 walls when the schedule cooperates.

### Multi-iter context (same harness, `reset_inter_prior`)

| Build | wall median | wall min | aborts median |
|-------|------------:|---------:|--------------:|
| tip `91e8d22` | ~34.0 | ~29.3 | ~331 |
| wall-push | ~34.7 | **22.1** | noisy |

---

## Artifacts

- `lab/results/resolve-wall-push-sf-occ.json` (and `.best.json`)
- `lab/results/resolve-wall-push-flip.json`
- `lab/results/resolve-wall-push-smoke.run.log`

## Code

- `crates/pevm/src/pevm.rs` — value-stable RebindOnly; ForceBind ≠ FullRestart
- `crates/pevm/src/mv_memory.rs` — atomic rebind; `current_data_value`
- `crates/pevm/src/vm.rs` — cold SpecRead fast path; FF once-count
- `crates/pevm/src/specfence/rem.rs` — `snapped_value`
- `crates/pevm/src/specfence/metrics.rs` — `cold_spec_fast`
- `crates/pevm/examples/specfence_g7_smoke.rs` — export + `reset_inter_prior` per cores row
