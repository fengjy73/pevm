# SpecFence SfMvMemory land v4

**Date:** 2026-09-22  
**Tip:** `230d0a0` on `cursor/specfence-sf-ps-true-spine-d6e8` (PR #45)  
**Baseline:** [`specfence-sf-mvmemory-land-v3.md`](specfence-sf-mvmemory-land-v3.md) (`2c32f91`)  
**Harness:** Soft=0 Instant-off (`env -u SPECFENCE_HANG_TRACE`), 8 cores, `SPECFENCE_COMPARE_CHECK=1`, N=5 both  
**Acceptance:** **TPS SF/OCC ≥ 1.5** both `3356896` and `15274915`.  
**Co-author:** `0xstride <fengjy73@users.noreply.github.com>`

---

## Verdict

**NOT MET — not a soft success below 1.5.** Soft=0 Instant-off N≥5 both: `seq=par`, `occ_picks=0`, `estimate_block_sf=0`, four-class + WAR first-class held.

| Block | v1 | v2 | v3 | **v4 reuse med TPS** | vs ≥1.5 |
|------:|---:|---:|---:|---------------------:|:-------:|
| 3356896 | ~0.76 | 0.61 | 0.71 | **0.83** | gap ~1.8× |
| 15274915 | ~0.48 | 0.72 | 0.70 | **0.65** | gap ~2.3× |

Thin improved (crit_pred no longer blocks OCC-shaped). Large sticky HOLD keeps `chain_n≈60–62` (no 62→4–5 collapse); `chain_ab/c` still **~1.2–2.0×** (target ≥3× unmet). Prefer Rewind fires but FullReplay wall remains.

---

## Levers verified this land

### 1. Thin OCC-shaped finish shell (`ef54d09`)

When `sf_occ_shaped` (thin Soft=0 ∧ no WaitOnce peer):

| Skip | Keep |
|------|------|
| `maybe_note_value` / rem reset | **all** `access_log.note` (fail_k) |
| finegrain `deep_begin_consumer` | WaitOnce/crit producer tip |
| finish tip publish + promote museum | `mv_memory.record` |

### 2. WaitOnce-only OCC-shaped gate (`230d0a0`) — **main thin win**

`has_wait_once_peer_before` no longer returns true on **crit_pred alone**. v3 falsely taxed ~every tx after the first sticky writer → calm shell ~0.5 ms. After fix, calm reuse SF≈1.4 ms on OCC≈1.1–1.2 ms (TPS ~0.78–0.85).

### 3. Large sticky ≥32 HOLD (`f7dc900`)

`avoid_hold` + reflect sticky writers into `last_location_writers` → focus `chain_n` stays **60–62** (v3 collapsed to 4–5). Learn still demotes sticky ℓ to **Opt** on wall — Avoid ratio does not jump to ≥3×.

### 4. Chain FullReplay→prefix when peer Released (`f7dc900` + widen `230d0a0`)

`try_chain_released_rewind`: crit ℓ in invalid (not only `len==1`) ∧ peer `chain_released` / done / validated → `PartialAbortRewind` + prefix. `resolve_rewind` 37–94/iter; FullReplay still 40–103. Wall cut is real but insufficient vs OCC.

### Discarded this land

**Per-ℓ ordinal strip** on OCC-shaped non-WaitOnce/crit (`note_access_ordinal`): thin iter outlier SF~3.9 ms / TPS~0.24 — fail_k gaps → FullReplay storm. Reverted; keep full `access_log.note`.

---

## Soft=0 Instant-off N=5 @8 (tip `230d0a0`, post–WaitOnce-only gate)

### 3356896

| Iter | OCC | SF | TPS | (c) | est |
|-----:|----:|---:|----:|----:|----:|
| 0 | 1.57 | 1.72 | 0.91 | 22 | 0 |
| 1 | 1.10 | 1.40 | 0.78 | **0** | 0 |
| 2 | 1.17 | 1.38 | 0.85 | 2 | 0 |
| 3 | 1.21 | 1.46 | 0.83 | 1 | 0 |
| 4 | 0.88 | 1.40 | 0.63 | **0** | 0 |

Reuse med TPS **0.83**. Calm SF still ≳ OCC (~0.2–0.5 ms); need SF ≲ OCC/1.5 ≈ 0.8 ms for ≥1.5.

### 15274915

| Iter | OCC | SF | TPS | Full | chain_ab/c | chain_n | est |
|-----:|----:|---:|----:|-----:|-----------:|--------:|----:|
| 0 | 7.28 | 13.68 | 0.53 | — | — | — | 0 |
| 1 | 5.29 | 9.13 | 0.58 | 82 | 196/131 (1.50×) | 61 | 0 |
| 2 | 5.82 | 7.90 | 0.74 | 58 | 156/126 (1.24×) | 61 | 0 |
| 3 | 5.31 | 8.13 | 0.65 | 40 | 144/71 (**2.03×**) | 61 | 0 |
| 4 | 5.32 | 9.23 | 0.58 | 72 | 201/141 (1.43×) | 61 | 0 |

Reuse med TPS **0.65**. WAR `war_ab≫war_c` intact. Sticky HOLD holds spine; Learn→Opt on `abd6bb…` keeps Avoid theater for Chain.

---

## Invariants (held)

| Check | 3356896 | 15274915 |
|-------|:-------:|:--------:|
| seq≡par | ok | ok |
| occ_picks | 0 | 0 |
| soft_wait_arms | 0 | 0 |
| estimate_block_sf | **0** | **0** |
| WAR first-class | yes | yes |
| Four-class RAW/WAR/WAW/Chain | yes | yes |

---

## Honest remaining gap vs ≥1.5

**Thin (~1.8× short):** OCC-shaped tax cut recovered calm SF toward OCC, but SF≪OCC needs either (a) real antichain beyond the ~17-writer WAW spine, or (b) another ~40% absolute wall cut on ~1.4 ms SF — not available from rem/tip alone without breaking ordinals (ordinal strip falsified).

**Large (~2.3× short):** Sticky HOLD + Prefer Rewind are necessary but not sufficient. `chain_ab/c` peak 2.0× ≪ 3× because Learn demotes sticky Win→Opt; Opt→validate→FullReplay/Rewind still dominates wall (~8–11 ms vs OCC ~5–6 ms). Next candidates (not shipped):

1. **Hold Win (not only writers) on sticky ≥32 when `avoid_hold`** — risk Fence tax / Soft≠0 regression  
2. **Producer-done schedule-defer earlier** (Version claim → Q_released before Data) — correctness risk  
3. Accept large may need native WaitOnce Avoid that beats Opt wall, not more Resolve Prefer

---

## Discarded (still)

Estimate Block, thin Rewind, 15-hold, mark_gated, long Chain busy-spin, Opt / per-ℓ `access_log` skip on OCC-shaped.

---

## Call-graph (v4 delta)

```
Thin OCC-shaped
  has_wait_once_peer_before := WaitOnce arms/edges only  # NOT crit_pred
  shaped ⇒ skip value_snap / rem reset / finegrain begin / finish tip+promote
  keep access_log.note all ℓ

Large sticky HOLD
  end_block: avoid_hold ∧ packed≥32 → last_location_writers[ℓ] ← max(live, sticky)
  validate Opt/edged:
    try_early_waw_rewind?
    else try_chain_released_rewind?  # any crit ℓ in invalid, peer Released
    else FullReplay
```
