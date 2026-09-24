# SpecFence complete-arch impl gap audit

**Date:** 2026-09-10  
**Against:** `specfence-complete-cc-architecture.md` + impl `specfence-complete-arch-impl.md`  
**Code tip at audit:** `b7c7353`  
**Status:** AUDIT — closed by `specfence-complete-arch-gaps-closed.md`

This file was missing from the checkout (cited as `94b2d55`). Reconstructed from the must-close list so the audit trail lives on the PR.

| Item | Class at `b7c7353` |
|------|-------------------|
| WaitFor leak / Ready→Spec (`1283b1c`) | **MISSING** vs D6 |
| Ordered wr spines (not fanout stubs) | **PARTIAL** |
| Progressive Data-publish wake | **PARTIAL** (SoftWait-only DAG) |
| In-batch Avoid (incl. ESTIMATE) | **PARTIAL** |
| Piece-restricted abort via EdgeKey | **PARTIAL** |
| EdgeKey → R1/R2/R3 | **PARTIAL** |
| Hot serialization lane / contention-split | **MISSING** |
| Live A6 conf decay (drop H) | **PARTIAL** |

Closure report: `lab/notes/specfence-complete-arch-gaps-closed.md`.
