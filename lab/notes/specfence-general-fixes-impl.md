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

## Process traces (N=3 @8, last SF iter)

`force_prefix ∧ writer=None` Unfenced (**U1 leak**) is **0** on 597 / 599 / 097.
Reason `unfenced_after_avoid` as a verb is **0** (fence-cover 597 had 265).
Residual `unfenced_after_avoid_total` is writer-done storage-origin while Avoid
is on — not a must_wait fallthrough.

| Block | force_prefix_none | unfenced_after_avoid reason | writer_done | indep | bind | wait | multi_spine | identity |
|------:|------------------:|----------------------------:|------------:|------:|-----:|-----:|------------:|---------:|
| **14689597** | **0** | **0** | 595 | 1141 | 754 | 26 | 31 | 157 |
| **19606599** | **0** | **0** | 2899 | 1432 | 528 | 37 | 12 | 163 |
| **19469097** | **0** | **0** | 1520 | 1017 | 449 | 17 | 30 | 182 |

597 warm: Unfenced 4231 / Wait 90 / Bind 800 / after-Avoid reason 0 / hot-after-fence 0.

Independents stay Unfenced (S2). SoftWait Soft = 0. Await@a = 0. xblock **598 sf-cold did not hang** (fence-cover did).

JSON: `lab/results/exec-process-{14689597,19606599,19469097}-general-fixes.json`,
`lab/results/general-fixes-xblock-{sf-occ,flip,xblock}.json`

---

## Wall / TPS honesty vs OCC (N=3 @8, this host)

This host matches the **rename-cut** OCC 597 median (**6.6 ms**), not the slower fence-cover host (OCC 13.3). Compare **ratios**.

| Block | SF wall med | OCC wall med | SF/OCC TPS | SF abort med | OCC abort med | Soft |
|------:|------------:|-------------:|-----------:|-------------:|--------------:|-----:|
| **14689597** | **20.5** | **6.6** | **0.288** | **47** | **117** | 0 |
| **19606599** | **32.8** | **10.5** | **0.349** | **139** | **78** | 0 |
| **19469097** | **20.1** | **7.6** | **0.376** | **174** | **109** | 0 |
| **19606598** | **2.9** | **1.3** | **0.492** | **18** | **7** | 0 |

Mean SF/OCC = **0.376**. Rename-cut mean was 0.353; fence-cover on a slower host was 0.268. **Not a makespan win vs OCC** (597 still ~3.1×). Abort↓ on 597 is not the bar. Prefer-admit is rare (Ready already Executing); multi-spine admit is live on 097 (40 on warm).

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
