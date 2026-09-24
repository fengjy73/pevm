# Mid-band over-admission × under-covered spine × terminology — full-package land

**Baseline:** PR #36 `cursor/specfence-s-lazy-object-14f0` @ `c063844`  
**Evidence:** `lab/notes/specfence-pr36-k8-pc-cc-learn-analysis.md`  
**Soft=0; `select_arm` is the only mouth; no staging**

## 0. Terminology

| Old | New |
|-----|-----|
| holes / begin 洞 | **dependency-gated admission** / **OrderedAdmit wait-set** |
| plant / 种 | **admit** / **seed wait-for dependency** |
| pick_occ | **ungated OCC task selection** |
| S-lazy | **lazy-update chain** |
| Spine-U | **under-covered conflict spine** |
| S-mixed | **mixed spine** |
| double_pay | **Detect+Resolve double charge** |
| fat / 肥块 | **large block** (`n≥512`) or describe by predicate |
| w_need | **cover_window** / **ordered_window_width** |
| 过预付 | **over-admission OrderedAdmit** |
| 壳 / meta-gap | **scheduler / validate / end-block overhead** |

Identifiers, metrics, comments, compare output, and land notes use the new terms. User-visible strings must not use the old jargon.

## 1. Function (same PR)

| ID | Content |
|----|---------|
| **A1 wait-set cap** | Not only `n≥512`: any **over-admission OrderedAdmit** risk (wait-set too large, or cover_window inflating while the wall does not drop) → soft-cap wait-set. Covers mid-band real spines (`19716145` begin~108, `19860366` begin~76). |
| **A2 mid-band end_block** | Lean end_block extends from large+lazy-seen to mid-band stable D1 / already-seen conflict structure. |
| **A3 under-covered spine** | Long storage conflict spine: no Full hard-order; ĉ compares deepening cover_window vs whole-spine OptimisticRead / OCC abort; if cover cannot close, do not learn-uphill (19807137-class). |
| **A4** | Keep PR36: lazy-update chain never OrderedAdmit; ungated OCC task selection; Done-on-success; Soft=0. |

## 2. Acceptance

1. Mid-band reps (`19716145`, `19860366`): wait-set / end_block down, ratio improves.
2. `19807137`: no learning-uphill Full/Seg; directional ratio improve or clearly yield to OCC.
3. Terminology: code + metrics + PR body have no banned jargon.
4. Soft=0; iter11; erc20; lazy-update does not return a 4×+ large-block tail; 3356896 no severe regress.

## 3. One PR
