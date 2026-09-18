# 3356896 post-PR22 完整优化落地

**Baseline:** PR #22 `cursor/specfence-mainchain-reuse-58b4`  
**Constraint:** Soft=0; one spine; A0＝OCC-effect; no wide empty-to stars; no mid-execute ReadyEdge races.

## What changed

| ID | Change |
|----|--------|
| **O1** | Long Basic WAW plants at most `ORDER_WINDOW_K=2` hops (4→31→66). Storage 14→16→17 stays fully ordered. Persist still stores the full D1 chain. |
| **O2/L1** | `ĉ_ordered_spine = hops × ĉ_A1` vs loc abort EMA. Full 16-writer prepaid loses. After a measured prepaid-lose block, long spines **demote to A0**; storage trio does not. |
| **O3** | EffectiveWAW abort records pairs, `clear_started` on the consumer, then one idle hop for the retry. Started successors stay A0 (no done-stamp race). |
| **O4** | Storage short edges unchanged; wide 0x209c envelope pairs still skipped at begin. |
| **P1** | A0 completions stamp unless *this* writer has waiters. `note_producer_done` skips the deferred mutex when `deferred_n==0`. |
| **P2** | One D1 MV snapshot (was two). Thin HotSet only ≥3-writer / promoted ℓ. |
| **P3** | Ready-width samples bag depth only (never the full-block scan count). |

## Kept

Soft=0; commute/ignore; indep tax 0; `edge_4_31`; no ERC-20 envelope stars.

## Why this hits PRIMARY

PR22 reuse fenced the whole 16-writer spine. OrderedAdmit prepaid on that chain exceeded OCC abort (reuse median 1.396 vs OCC 0.979). Windowed plant + cost-gate demote remove that prepaid wall; O3 cuts cold unfenced without mid-execute insert.
