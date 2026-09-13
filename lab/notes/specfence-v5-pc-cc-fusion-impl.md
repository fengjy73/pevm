# SpecFence v5 PC⊗CC fusion — implementation map

**Date:** 2026-09-13  
**Branch:** `cursor/specfence-v5-pc-cc-fusion-e28e`  
**Plant SoT:** `lab/notes/specfence-complete-architecture-v5-pc-cc-fusion.md`  
**π SoT:** `lab/notes/specfence-complete-architecture-v4-frozen-grain.md` (unchanged)  
**Honesty baseline:** parallel-compute nonempty N=1 median **SF/OCC = 0.744**; quiet **1.020**

**Verdict:** one-iteration access-local Mode(a) fusion. Incarnation Occ\|Pcc fork removed as SoT. Sweeps below.

---

## Land map (file:fn)

| # | SoT item | file:fn | Status |
|---|---------|---------|--------|
| 1 | Architecture SoT + diagnosis + switch audit | `lab/notes/specfence-complete-architecture-v5-pc-cc-fusion.md` et al. | **landed** |
| 2 | Mode(a) certificates, not `mark_pcc(tx)` | `kernel.rs::note_fence` / `may_resolve` | **landed** |
| 3 | `decide(a, e_vis, PE, learning)` | `access_policy.rs::decide` | **landed** |
| 4 | AccessOrdinalLog true \(k\) | `rem.rs::note_access_k_only` on every SF access | **landed** |
| 5 | Spec abort PE at true \(k\) (never residual 1) | `executor.rs::validate_occ_kernel` | **landed** |
| 6 | Prior PE + Data / executing writer may Fence | `decide` Bind / WaitFor | **landed** |
| 7 | Serial-lane multi-writer PE class | `vm.rs::pcc_serial_lane` | **landed** |
| 8 | R1 only with certificates; Spec miss = B0 | `uses_specfence_resolve` = `may_resolve` | **landed** |
| 9 | Ready/steal respects Fence/Wait | `executor.rs::next_sf_task` | **landed** |
| 10 | HotSet / WŜ / morph consumed by `decide` | `vm.rs::access_vis` | **landed** |
| 11 | Empty PE ∧ ¬Fence ⇒ OCC (±detect, ±k-log) | `specfence_plant_is_occ` after k-log | **landed** |

---

## Control loop (live)

```
OCC:     next_task(); occ_read; validate_read_locations; B0
SpecFence access a:
  detect; k := note_access_k_only(ℓ)
  empty PE → Spec (OCC)
  ¬PE(ℓ,k) → Spec
  vis := e_vis + HotSet + WŜ
  decide → Spec | Bind | WaitFor | SerialLane
  Fence event → certificate (rem / R1), not a tx kernel
SpecFence validate:
  ¬certificate → OCC bool + B0 + PE(true k)
  certificate → R1a / R1b / B0
steal: WaitFor/lane ready before validation-first
```

---

## Tests / sweeps

See the rest of this note after CI and all-blocks honesty.
