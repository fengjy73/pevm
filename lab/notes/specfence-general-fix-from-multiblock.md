# General (morphology-agnostic) fixes from multiblock process

**Date:** 2026-09-11  
**Branch:** `cursor/specfence-complete-cc-63b0`  
**PR:** https://github.com/fengjy73/pevm/pull/3  
**SoT:** `specfence-spec-means-region.md` — Spec = Region; Fence = barrier; Unfenced ≠ Spec  
**Impl:** `specfence-general-fixes-impl.md`

Diagnosed on 597 / 599 / 097 / 096 / 098 families after Fence-cover. These
are **protocol** leaks, not morph-mode switches. Implement all in one pass.

---

## Implement all (morphology-agnostic)

1. **U1** `force_prefix` must carry a resolvable writer id / prefer-admit —
   never bare Unfenced when `must_wait` / `force_prefix`.
2. **U2** Avoid predicted-writer stickiness through repair incarnations.
   Spine identity comes from MV / residual / preserved ℓ→writer, not a
   min-sticky `predicted_writer` ghost after the writer dropped ℓ.
3. **S1+U3** prefer-admit Ready spine writers before independence Unfenced;
   WaitFor-park only lower **Executing**. Hang-freedom = admit/steal, not
   Unfenced on a known essential.
4. **U4** R2/R4 preserve ℓ→writer identity; promote R1 when value-stable.
5. **S4** per-ℓ multi-spine admit (097/096) — every unfinished writer on
   that ℓ, not only the max-writers tip.
6. **U5** post-Avoid / repair: no `must_wait` → Unfenced on that ℓ
   (debug-assert + convert-to-Fence).
7. **U6** quiet Fence revoke + decayable morph prior (098↔599 flips).
   Live Avoid stays; warm-seeded H/templates without Avoid drop.
8. **S2/S5** keep Unfenced independents overlapping the spine. Do **not**
   chase rare non-hot WaitFor / SoftWait / 597-index hardcodes.

---

## Hard bans

SoftWait storms. EV Await doors. tip-identity as Bind gate. OCC-retry as
control plane. morph Storm doors as π. 597-only hardcodes.

---

## Not in this cut

- Serial-all clique without a writer (rejected earlier: seq≠par).
- SoftWait Soft / Await@a / AdaptiveParams-as-θ.
- Chasing residual `unfenced_cold` first-wave without a visible writer.
