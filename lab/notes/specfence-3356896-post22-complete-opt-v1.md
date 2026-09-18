# 3356896 post-PR22 完整优化落地

**Baseline:** PR #22 `cursor/specfence-mainchain-reuse-58b4`  
**Constraint:** Soft=0; one spine; A0＝OCC-effect; no wide empty-to stars; no mid-execute ReadyEdge races.

## What changed

| ID | Change |
|----|--------|
| **O1** | Short WAW (≤`ORDER_WINDOW_K=2`) stays fully ordered (storage 14→16→17). Long Basic WAW is **A0 at begin** when leftover-aware EV loses. Persist still stores the full D1 chain. Thin write-set is OCC-identical except storage CallWaw. |
| **O2/L1** | Leftover-aware `ĉ_ordered_spine`. Long spines demote to A0; `abort_cf>prepaid` does **not** un-demote them. Write-set does not re-gate a demoted spine. |
| **O3** | EffectiveWAW abort records pairs. One idle hop only when EV still wants order. Started successors stay A0 (no done-stamp race). |
| **O4** | Storage short edges unchanged; wide 0x209c envelope pairs still skipped at begin. |
| **P1** | Always stamp A0 done. Wake only if *this* writer has waiters. Do not bag-spam ungated preds. |
| **P2** | Live D1 + promoted-ℓ MV merge only if ready missed 4→31. Thin skips HotSet storm, sketch decay, and end-block promote walk. |
| **P3** | Refuse steal is bag + 32-wide window (no full-block scan). Ready-width is bag depth, not n_tx−blocked. |

## 3356896 @8 Soft=0 N=7

| | OCC | SF cold | SF reuse |
|---|---|---|---|
| median wall | **0.863 ms** | 1.355 ms | **1.137 ms** |
| unfenced | — | 14 | 14 |
| main_inc | — | tail 67…171 | same |
| storage / commute / taxed / soft / 4→31 | — | 干净 / 77 / 0 / 0 / true | 同左 |

**PRIMARY `sf_le_occ`: false.** Reuse beat PR22 (1.396 → 1.137) by dropping the 16-writer prepaid wall, but leftover SF meta + tail abort still sits ~0.27 ms above OCC (0.863). Prefix-window plant and pick-time WAW were tried earlier; the latter SIGSEGV’d and was reverted.

## Kept

Soft=0; commute/ignore; indep tax 0; `edge_4_31`; no ERC-20 envelope stars.
