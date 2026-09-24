# SpecFence sticky HOLD + Chain FullReplay→prefix (15274915 Soft=0)

**Date:** 2026-09-22  
**Branch:** `cursor/specfence-sticky-hold-chain-rewind-2be4`  
**Base:** `cursor/specfence-sf-ps-true-spine-d6e8` (@ `2c32f91` / land v3)  
**Harness:** Soft=0 Instant-off, `estimate_block_sf=0`, no Estimate Block, no `mark_gated`, antichain fill kept, four-class + WAR first-class.

---

## 1. Why sticky `crit_chain_n` collapses (62→4–5)

| Layer | What happens |
|-------|----------------|
| `ready_edges.note_location_writer` | Only admit/Detect plants D1 writers |
| ChainSpine Avoid | Suppresses Opt conflict → fewer D1 notes on `abd6bb…` |
| `lean_end` / skip pair merge | Short live snapshot survives as `last_location_writers` |
| Focus print (pre-fix) | `max_by_key(last_location_writers)` → **chain_n 4–5** |
| Learn | Short `n_pairs` → under-covered / unused-Win → **Win→Opt** (`policy.rs` `end_block_learn*`, `arm_table.rs` `end_pack`) |
| `inter_prior.crit_chain` | cd72ec8 already held ≥32 same-ℓ; quiet equal-length hop fixed — but **D1/focus/Learn still saw collapse** |

**Root cause:** Avoid-quiet D1 under-count, not lost sticky pack alone. HOLD must refresh observed writers when Avoid/wall success.

---

## 2. Edits ranked by expected TPS impact (large 15274915)

| Rank | TPS lever | File:lines | Change |
|-----:|-----------|------------|--------|
| **1** | Cut FullReplay wall on sticky spine | `resolve_plan.rs` `try_chain_released_rewind` (~526–605); `executor.rs` `validate_to_plan` Opt (~557–568) + edged (~652–662) | When first conflict is crit_loc ≥32 and peer `chain_released` / `true_publish_ready` / done → **Prefer `PartialAbortRewind` + prefix** over `FullReplay` (raise effective Avoid ratio, cut `chain_c` wall) |
| **2** | Keep spine Detect/Avoid quality across reuse | `pevm.rs` ~970–1030 | `avoid_hold` (`chain_ab ≥ chain_c`) → only strictly-longer upgrade; **reflect sticky ≥32 into `last_location_writers`** so lean/focus/Learn never see 4–5 |
| **3** | Observability (not wall) | `specfence_3356896_compare.rs` focus print; `pevm.rs` `sticky_crit_chain()` | Prefer sticky crit for `chain_n` when ≥32 |

**Constraints held:** Soft=0, `estimate_block_sf=0`, no Estimate Block, no `mark_gated`, `plant_nearest_preds` antichain for non-succ, four-class audit, WAR `enqueue_higher_revalidate` untouched. Thin still skips Rewind (`THIN_SHELL_N`).

---

## Call-graph

```
end_block:
  fresh = select_crit_chain(D1)
  avoid_hold = chain_ab ≥ chain_c ∧ chain_ab > 0
  packed = HOLD ≥32 (avoid_hold ⇒ no equal hop / no shrink)
  last_location_writers[ℓ] ← max(live, sticky writers)   # kill 62→4–5
  pack_crit_chain(packed)

validate_to_plan (Opt / edged WAW):
  try_early_waw_rewind?
  else try_chain_released_rewind?   # crit ∧ peer Released → PartialAbortRewind
  else FullReplay
```
