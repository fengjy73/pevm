# Mid-band real spine × Opt path tax — full land

**Baseline:** PR #39 `cursor/specfence-endblock-spine-tps-c471` @ `efe87f2`  
**Evidence:** 99-block Soft=0: SF TPS≥OCC **28/98**; median TPS ratio 0.857; 70 losers (Opt 36 / Win_* 24 / Full 9)  
**Terms:** OrderedAdmit wait-set / ungated OCC task selection / cover_window / under-covered conflict spine / lazy-update chain / over-admission OrderedAdmit  
**Soft=0 · one spine · `select_arm` is the only mouth**

## Function (same PR)

| ID | Content |
|----|---------|
| **M1** | Mid-band leftover-long real spine: harsher ĉ — if OrderedAdmit prepaid is not cheaper than OCC abort, whole-spine sticky OptimisticRead. Wait-set already soft-capped at 8 that still loses drops `cover_window` and withdraws order. Thin short-chain (3356896) keeps light-cover Win_2. |
| **M2** | Expand OCC-aligned Opt path-tax skip to mid/large when the wait-set is empty or only short-chain. Large lazy-update leftover steal stays a separate mouth (`ignore_leftover_reservations`) so a live wait-set is not OCC-stolen (19469101). |
| **M3** | Under-covered conflict spine and leftover-long mid/large: ban empty Win_1 churn. OptimisticRead until cover is proven cheaper (measured covering arm, `last_cover_ok`, ĉ + hysteresis &lt; abort). |
| **M4** | Re-run full 99-block Soft=0 TPS vs OCC after land. |
| **M5** | Keep: lazy-update never OrderedAdmit; Done-on-success; wait-set predicate soft-cap; Soft=0. |

## Acceptance

1. Mid-band reps (`19716145`, `19638737`, `19860366`, `16146267`) SF/OCC TPS up vs PR39.
2. Corpus SF TPS≥OCC **> 28/98**; median TPS ratio up vs 0.857.
3. Wall max does not return a 4–27× lazy-update fat-tail (one near-indep 4× line ok).
4. Soft=0; iter11; erc20. Prefer corpus TPS win-rate over polishing 3356896 alone.
