# SpecFence general fixes (U1–U6 + S1/S4) — impl map

**Date:** 2026-09-11  
**PR:** https://github.com/fengjy73/pevm/pull/3  
**Source contract:** `lab/notes/specfence-general-fix-from-multiblock.md`  
**Vocabulary:** Spec = Region; Fence = Bind / WaitFor / serial-lane + admit;
Unfenced = optimistic access (not Spec).

---

## File:fn

| Item | Contract | Symbol |
|------|----------|--------|
| **U1** | force_prefix carries writer id; never bare Unfenced | `PartialRetryTable::force_writer` / `note_force_writer`; `Vm::maybe_wait_specfence` (observed = MV ∨ residual ∨ force_writer); `choose_edge_action` serial-lane when writer=None; `Vm::fence_wait_for` (`must_wait` never Unfenced while a live pred exists) |
| **U2** | no predicted-writer stickiness across repair | `HotSketch::forget_writer`; `next_writer_before` / `next_unfinished_writer_before` spine-only (no predicted fallback); `pevm.rs` forget on suffix invalidate |
| **S1+U3** | prefer-admit Ready; park only Executing | `Scheduler::is_ready` / `admit_spine_writers`; `Vm::maybe_wait_specfence` admit unfinished before Unfenced; `fence_wait_for` parks `is_executing`, admits Ready |
| **U4** | R2/R4 keep ℓ→writer; R1 when value-stable | `PartialRetryTable::{note_force_writer,force_writer}` survives `escalate_full_restart`; `pevm.rs` `try_validate` records identity on abort; existing value-stable → `try_rebind_invalid_reads_value_stable` (R1) |
| **S4** | per-ℓ multi-spine admit | `HotSketch::unfinished_writers_before`; `Scheduler::admit_spine_writers`; both wait and Unfenced paths admit **all** unfinished writers on that ℓ |
| **U5** | no force_prefix→Unfenced (incl. repair) | `maybe_wait_specfence` Unfenced arm converts to `fence_wait_for` when `force_prefix` only (Avoid serial-all rejected: seq≠par); `debug_assert` on force_prefix ∧ writer < reader; plant-TLS does not Unfence `must_wait` |
| **U6** | quiet Fence revoke + decayable morph prior | `HotSketch::seed_from_prior_morph` (quiet×flip decay); `revoke_prior_fences_if_quiet` (live Avoid kept); `pevm.rs` block-start; `SpecFenceCtx::try_revoke_unified` |
| **S2/S5** | independents stay Unfenced | independence Unfenced path unchanged after prefer-admit; no 597-index / SoftWait / Await@a doors |

---

## Tests

| Test | File |
|------|------|
| `force_prefix_with_writer_never_unfenced` | `edge.rs` |
| `prefer_admit_ready_does_not_unfence_must_wait` | `edge.rs` |
| `forget_writer_drops_predicted_stickiness` | `sketch.rs` |
| `multi_spine_unfinished_all_writers` | `sketch.rs` |
| `quiet_revoke_drops_warm_fence_keeps_avoid` | `sketch.rs` |
| `r2_r4_preserve_location_writer_identity` | `rem.rs` |
| `general_fixes_force_prefix_writer_and_multi_spine` | `tests/specfence.rs` |

---

## Hard bans (this cut)

| Ban | Status |
|-----|--------|
| SoftWait storms | Soft=0 (no new SoftWait arms) |
| EV Await / AdaptiveParams-as-θ | Await@a unchanged (0) |
| tip-identity Bind gate | Bind still on Data |
| OCC-retry as π | no |
| Storm morph as π | U6 decays/revokes prior only |
| 597-only hardcodes | none |
