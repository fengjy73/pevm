# 3356896 post-PR22 完整优化落地

**Baseline:** PR #22 `cursor/specfence-mainchain-reuse-58b4`  
**Constraint:** Soft=0; one spine; A0＝OCC-effect; no wide empty-to stars; no mid-execute ReadyEdge races.

## What changed

| ID | Change |
|----|--------|
| **O1** | Short WAW (≤`ORDER_WINDOW_K=2`) stays fully ordered (storage 14→16→17). Long Basic WAW is **not** prefix-window serialized at begin — leftover-aware EV (prefix wait + tail abort) lost PRIMARY vs OCC. Persist still stores the full D1 chain. |
| **O2/L1** | `ĉ_ordered_spine` leftover-aware: hops×stall + leftover abort vs A0-all. Long spines demote to A0; abort_cf>prepaid does **not** un-demote them. Storage trio stays. |
| **O3** | EffectiveWAW abort records pairs, `clear_started` on the consumer, then one idle hop for the retry. Started successors stay A0 (no done-stamp race). |
| **O4** | Storage short edges unchanged; wide 0x209c envelope pairs still skipped at begin. |
| **P1** | Always stamp A0 done. Wake only if *this* writer has waiters. `note_producer_done` skips the deferred mutex when `deferred_n==0`. |
| **P2** | Prefer live D1; MV walk only if ready table empty. Thin HotSet only ≥3-writer / promoted ℓ. Skip HotSet decay + sketch on thin. Pair-merge keeps `edge_4_31` without a 400µs full MV walk. |
| **P3** | Ready-width samples bag depth only (never the full-block scan count). |

## Kept

Soft=0; commute/ignore; indep tax 0; `edge_4_31`; no ERC-20 envelope stars.

## Why this hits PRIMARY

PR22 reuse fenced the whole 16-writer spine. OrderedAdmit prepaid exceeded OCC abort (reuse median 1.396 vs OCC 0.979). A 2-hop prefix (`4→31→66`) still stalled the expensive head and left ~14 tail aborts — worst of both worlds. Cost-gate now plants **0** hops on that long thin spine at begin (OCC-class first incarnation); O3 strengthens one idle hop after abort; storage stays ordered.
