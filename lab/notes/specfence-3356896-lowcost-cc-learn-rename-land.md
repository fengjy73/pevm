# 3356896 — low-cost CC × optimistic-path meta × rename × learn loop

**Baseline:** PR #23 `cursor/specfence-post22-complete-opt-171a`  
**Constraint:** Soft=0; one pevm spine; no wide empty-to stars; no mid-execute ReadyEdge races.

## Naming

| Old | New |
|-----|-----|
| A0 / `a0_cohorts` | **OptimisticRead** / `optimistic_read_cohorts` |
| A1 / `a1_cohorts` | **OrderedAdmit** / `ordered_admit_cohorts` |
| demote to A0 | demote to OptimisticRead |
| `a0_majority_block` | `optimistic_majority_block` |

ReadyEdge stays dependency-aware admission. Compare / metrics / LearnReport use the new names only.

## CC (sandwich)

| ID | Land |
|----|------|
| **CC-L1** | Validate / next pick quantum: single-hop wait-for for the nearest unfinished successor after EffectiveWAW. Pred is already Publish. Not begin-plant 16 edges. |
| **CC-L2** | incarnation≥1 retry on that ℓ is OrderedAdmit even when begin stayed OptimisticRead. |
| **CC-L3** | WindowedOrdered k=1 — only the nearest unfinished writer. No full-spine prepaid list. |
| **CC-L4** | Per hot ℓ min ĉ among {OptimisticRead, WindowedOrdered(k=1), FullChain}. FullChain default only for short storage chains. |
| **CC-L5** | Storage 14→16→17 stays FullChain. Never 0x209c envelope stars. |

## Optimistic-path meta

| ID | Land |
|----|------|
| **M1** | Conflict-free validate still ≡ OCC (`validate_optimistic_fast`). |
| **M2** | Thin + D1 already has 4→31: skip HotSet notes / MV merge. Sketch still off on thin. |
| **M3** | Bag / wake still OrderedAdmit-gated only. |
| **M4** | `optimistic_path_tax_ns` = admit_seed + end_block + refuse (compare + LearnReport). |

## Learning (F1–F6)

- F1: per-ℓ reward = −(reexec_ns + ordered_ns + refuse share)
- F2: three-way ĉ updated at end_block from the decision-time action
- F3: `note_hops_decision` stores eligibility on PromotedLoc
- F4: EMA + n₀; consecutive FullChain losses demote long spines
- F5: hot path reads snapshots / promoted EMA only; writeback at end_block
- F6: loop uses incarnation / reexec_ns / ordered_ns (not `occ_aborts`)

## Same-block plant

Validate-time / pick-quantum ReadyEdge flush livelocked ERC-20 and flaked iter11 seq≡par. CC-L1/L2 pairs are persisted for the **next begin** (WindowedOrdered k=1 / FullChain storage). No mid-block abort plant.

## 3356896 @8 Soft=0 (this machine)

PR23 land note (other machine) N=7: OCC 0.863 / SF cold 1.355 / reuse **1.137** / unfenced 14.

This PR:

| N | OCC med | SF cold | SF reuse | unfenced last | PRIMARY |
|---|---------|---------|----------|---------------|---------|
| 7 | 1.084 | 1.356 | **1.092** | 15 | false (gap 8µs; OCC noisy 0.75–1.73) |
| 9 | 0.791 | 1.220 | **1.076** | **13** | false (gap 0.285ms) |

Reuse wall is below PR23’s 1.137 and far below PR22’s 1.40 full-order. Unfenced 13–15 (N=9 last 13, not 14). Soft=0, commute 77, ignore 77, taxed_indep 0, storage_inc=[], edge_4_31, seq≡par suite green.
