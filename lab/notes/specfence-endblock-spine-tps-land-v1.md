# Continue: lean `end_block` + under-covered sticky Opt → 99-block TPS vs OCC

**Baseline:** PR #38 `cursor/specfence-midband-spine-rename-7361` @ `37eb27b`  
**Residuals:** `19860366` wait-set 8 but `end_block` ~3.9 ms (×≈2.27); `19807137` no Full/Seg uphill but still ~3.66× with Opt→Win_1  
**Terms:** dependency-gated admission / OrderedAdmit wait-set / ungated OCC task selection / lazy-update chain / under-covered conflict spine / cover_window / over-admission OrderedAdmit / Detect+Resolve double charge / large block  
**Soft=0 · one spine · `select_arm` is the only mouth**

## Function (same PR)

| ID | Content |
|----|---------|
| **E1** | Mid-band lean `end_block`: reuse with stored D1 **or** already-seen conflict structure skips HotSet / inter-prior / sketch / MV merge / persist clone / pair-merge / morph flush. Target: `19860366`-class `end_block` far below OCC wall share. First mid-band still persists. |
| **E2** | Under-covered conflict spine **or** ordered prepaid ≥ OCC abort: sticky OptimisticRead. No empty Win_1 churn; no costly Seg/Full invite; `hops_to_admit=0`; cached Win_1 is ignored. Thin short-chain (3356896) keeps light-cover Win_2. |
| **E3** | Large lazy-update / near-independent: skip ungated execute+validate path tax (`skip_ungated_path_tax`). Same-block reuse keeps the structure flag. Ungated OCC task selection is unchanged. |
| **E4** | Keep PR36/38: lazy-update never OrderedAdmit; wait-set predicate soft-cap; Done-on-success; Soft=0. |

## Acceptance

1. `19860366` / `19807137` move the right way (`end_block` share down; no Opt→Win_1 churn).
2. Full loadable-block Soft=0 TPS vs OCC (`SPECFENCE_ALL_BLOCKS=all`): per-block wall / TPS / SF/OCC TPS ratio + aggregates.
3. iter11 / erc20 / Soft=0; no 4×+ lazy-update fat-tail return.
4. One draft PR; do not merge.
