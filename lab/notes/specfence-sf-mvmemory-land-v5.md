# SpecFence SfMvMemory land v5

**Date:** 2026-09-22  
**Tip:** (this commit) on `cursor/specfence-sf-ps-true-spine-d6e8` (PR #45)  
**Baseline:** [`specfence-sf-mvmemory-land-v4.md`](specfence-sf-mvmemory-land-v4.md) (`7c998be`)  
**Harness:** Soft=0 Instant-off (`env -u SPECFENCE_HANG_TRACE`), 8 cores, `SPECFENCE_COMPARE_CHECK=1`, N=5 both  
**Acceptance:** **TPS SF/OCC ≥ 1.5** both `3356896` and `15274915`.  
**Co-author:** `0xstride <fengjy73@users.noreply.github.com>`

---

## Verdict

**NOT MET — not a soft success below 1.5.** Soft=0 Instant-off N≥5 both held invariants. Productive experiments this land **falsified** the remaining v4 candidates; hot path restored to v4 tip after regressions.

| Block | v3 | v4 | **v5 reuse med TPS** | vs ≥1.5 |
|------:|---:|---:|---------------------:|:-------:|
| 3356896 | 0.71 | **0.83** | **~0.83** (restored) | gap ~1.8× |
| 15274915 | 0.70 | **0.65** | **~0.65** (restored) | gap ~2.3× |

Micro-cuts have **plateaued**. Hitting ≥1.5 needs a hot-path redesign where schedule Avoid is cheaper than the OCC abort train — not more Resolve Prefer / Win prepaid / rem tax strip.

---

## Experiments this land (all measured Soft=0 Instant-off)

### A. Thin rem/vis/metrics skip on `sf_occ_shaped`

| Skip | Intent |
|------|--------|
| `resolve_read_overlay` rem DashMap | unused rem on shaped |
| `VisibilityPolicy::Opt` without `for_ready` | skip ReadyEdge probe |
| `record_occ_kernel_exec` / `record_evm_entry` | metrics atomics |

**Result:** reuse med TPS **~0.68** (regressed from 0.83). Absolute calm SF still ~1.3 ms; OCC variance + conflict iters/noise. **Reverted.** Remaining thin wall is still `access_log.note` (forbidden to strip) + WAW spine serialization — not rem overlay.

### B. Sticky ≥32 Hold **Win_2** after Learn (`avoid_hold`)

| Edit | Intent |
|------|--------|
| `remember_arm_force(Win_2)` + `force_sticky_win` after Learn | keep Detect/Avoid |
| `install_prior` keep Win when `n_pairs≥32` | stop Opt nail |

**Result:** `abd6bb…:Win_2/74` held across reuse; `chain_n≈68`. **Fence/refuse prepaid raised SF wall** (reuse SF ~9–14 ms, cold ~19 ms). `chain_ab/c` stayed **~1.5–1.9×** ≪3×. FullReplay dropped somewhat but TPS **~0.59** (worse). **Falsified — Reverted.** Writers-only HOLD remains correct; Win HOLD ≠ free Avoid.

### C. `chain_claim` → `wake_planted_on_publish` + consult Prefer schedule-defer

| Edit | Intent |
|------|--------|
| Wake planted succ at Version claim | earlier Q_released Avoid |
| Cut 512 Released-spin → brief/immediate defer | less spin theater |

**Result (alone, after Win revert):** large reuse med **~0.60**, `chain_ab/c` ~1.2–1.6× — no ≥3× lift; wall not better than v4 Prefer Rewind. Thin also soft-regressed under combined A+C. **Reverted** with A/B to restore v4 tip.

---

## Soft=0 Instant-off N=5 @8 (restored v4 tip; confirmatory)

Re-run after restore should match land-v4 within noise:

| Block | Expected reuse med TPS | Notes |
|------:|-----------------------:|-------|
| 3356896 | **~0.83** | WaitOnce-only OCC-shaped gate |
| 15274915 | **~0.65** | sticky writers HOLD + Prefer Rewind |

Invariants: Soft=0, `estimate_block_sf=0`, four-class + WAR, `occ_picks=0`, WaitOnce-only gate (no `crit_pred`).

---

## Honest remaining gap vs ≥1.5

**Thin (~1.8×):** Tax cuts on rem/tip/vis/metrics are exhausted under the ordinal constraint. Calm SF≈1.4 ms vs need ≲1.0 ms; `access_log.note` all ℓ is mandatory. Ceiling: ~17-writer WAW on 176 txs — SF≪OCC needs **more antichain**, not cheaper SpecFence shell.

**Large (~2.3×):** Prefer Rewind + sticky writers HOLD are necessary but capped. Hold **Win** makes Detect more expensive than Opt abort (prepaid wall). Claim-wake does not raise Avoid ratio to ≥3× without a **cheaper** schedule Avoid than OCC’s abort train (no Blocking park, no Win Fence, no Estimate Block).

### What ≥1.5 actually requires (redesign, not micro-cut)

1. **Schedule-native Avoid cheaper than OCC abort** on sticky spine: plant → Version → Released → succ runnable **without** Opt validate FullReplay theater and **without** Win OrderedAdmit prepaid.
2. **Thin:** either beat OCC via real width (break WAW serialization) or accept thin Soft=0 TPS ceiling ~0.8–1.0 under current ordinal+spine constraints.
3. Do **not** retry: per-ℓ ordinal strip, Opt `access_log` skip, sticky Win HOLD, Estimate Block, thin Rewind, mark_gated, long Chain busy-spin.

---

## Discarded (cumulative)

Estimate Block, thin Rewind, 15-hold, mark_gated, long Chain busy-spin, Opt / per-ℓ `access_log` skip, sticky **Win** HOLD after Learn, claim-wake-only as ≥3× lever, rem/vis/metrics micro-skip as path to thin SF ≲1.0 ms.

---

## Call-graph (v5 = v4 tip restored)

```
Thin OCC-shaped (WaitOnce-only gate)
  shaped ⇒ skip consult / engagement / museum / value_snap / rem reset / tip+promote
  keep access_log.note all ℓ

Large sticky writers HOLD + Prefer Rewind
  end_block: avoid_hold ∧ packed≥32 → last_location_writers[ℓ] ← max(live, sticky)
  validate: try_chain_released_rewind? else FullReplay
  # Win HOLD / claim-wake: measured, reverted
```
