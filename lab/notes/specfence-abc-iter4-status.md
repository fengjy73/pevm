# SpecFence A+B+C Iter 4 status

**Date:** 2026-09-08 (Asia/Shanghai)  
**Branch:** `specfence`  
**Base tip:** `0a72e55` (Iter3)  
**Authority:** `specfence-abc-unified-protocol.md`, `specfence-abc-iter3-status.md`, `specfence-global-cc-evm-rethink.md`

## Mandate

Attack resolve hole (FullRestart EVM ~90 on 597; aj=0) **without** Lean `inspect_run` / live_prime. Dual bets if coherent:
1. Hang-free PC/gas at effect boundaries via rem/plant on Handler::run (no Inspector).
2. Stronger hot-ℓ clique serial barrier (capped park tax).

Keep A BO Await + C Quiet|Storm. SoftWait Soft~0. No ESTIMATE prefix poison.

## What we landed

### 1) Hang-free SSTORE plant capture (plumbing; Lean OFF)
- Installed `sstore_plant_capture_eth` on Mainnet instruction table (`ethereum.rs`).
- Lite tip: pc/gas/refund/code_hash only (full stack/memory clone livelocked WaitHard m2/p4).
- `with_plant_tls` on Handler::run **only when research inspect** — Lean plant (even resume-only) re-armed WaitHard livelocks under concurrency.
- `try_apply_pending_pc_resume` extracted for future Handler jump apply; absolute jump arm stays research-inspect only (Iter2 hang).
- Metric `handler_sstore_capture` (0 on Lean production path).

**Falsified for Lean production this iter:** arming plant TLS on Handler::run (all-tx, storm, or SuffixRepair resume) hangs WaitHard/p4. Capture≠inspect_run is real, but **not yet safe under BO Await concurrency**.

### 2) Hot-ℓ clique barrier (production)
- On Storm∧was_force_bind escalate: among Executing conflict writers, pick writer whose invalid ℓ has **max higher-reader fan-out** (storm spine), then once-park (cap 1/tx).
- **Rejected:** parking Aborting sibling consumers behind same writer — raced `finish_validation` / hung m2/p4.
- Metric `serial_barrier_clique` counts barriers where chosen ℓ fan>1.

## Multi-block table

### Primary: N=10 medians @8 (`abc-iter4-n10`)
| Block | SF wall med | OCC wall med | SoftWait Soft | SF aborts med | sb_res / sb_clique | vs Iter3 N=10 |
|------:|------------:|-------------:|--------------:|--------------:|-------------------|---------------|
| **597** | **14.2** | 4.0 | **0** | 188 | >0 / >0 | **↑ vs 13.4** (no win) |
| **599** | 23.0 | 10.2 | **0** | 224 | >0 | ↑ vs 19.7 (noise/regress) |
| **097** | 12.6 | 6.5 | **0** | 242 | >0 | ≈/↓ vs 12.9 |
| **598** | 2.2 | 1.2 | **0** | 19 | ~0 | quiet OK |

### Secondary: N=5 (`abc-iter4`)
597 med **14.1** p90 16.8 SoftWait=0; hsstore=0; aj=0; no hang.

`absolute_jump_applied=0`. SoftWait Soft **0**. No hang on 597/599.

Tests: `cargo test -p pevm --lib` **95 ok**; `--test specfence` **23 ok / 13 ignored**.

## Iter 4 diagnosis (required answers)

### 1. detect / avoid / resolve — which hole now?
**Resolve still primary.** Hot-ℓ fanout writer pick fires (`serial_barrier_clique>0`) but does not cut FullRestart interpreter-seconds; 597 N=10 **14.2** not below Iter3 **13.4** / stretch &lt;10. Hang-free Handler capture exists but **cannot arm on Lean** without WaitHard livelock — so aj stays 0 and repair remains FullRestart-class.  
**Avoid** OK (SoftWait Soft=0; BO Await kept).  
**Detect** OK.

### 2. Region / Fence / intra / inter
| Axis | Verdict |
|------|---------|
| **Region** | ℓ / `a` OK; hot-ℓ fanout now steers which writer the resolve barrier attaches to. |
| **Fence** | Serial-barrier resolve fence kept (once/Executing); sibling-consumer clique fence **rejected**; absolute-jump still unarmed on Lean. |
| **Intra** | Hot-only π unchanged; Handler SSTORE tips not yet feeding SuffixRepair jump. |
| **Inter** | Quiet\|Storm still gates barrier; must not drive Lean plant capture today. |

### 3. vs Iter3 / plateau
**No wall win on 597** (14.2 vs 13.4 N=10). SoftWait Soft **0**. No hang. aj=0. **&lt;13.4 / &lt;10 unmet.**

### 4. Cause for Iter 5 (named)
**Named cause:** Remaining FullRestarts still cost ~full EVM incarnation. Hang-free PC/gas capture on Handler::run is **blocked by plant-TLS × BO Await livelock** (same family as inspect hang), so `jump_is_safe` never arms on Lean. Hot-ℓ writer selection alone is insufficient to drop fb_reabort≪90.

**Iter5 bets (falsifiable):**
1. **Serial one-tx capture window** — briefly serialize only the jumped/captured tx (or disable WaitHard mid-capture) so lite SSTORE tips can land without plant-under-Await hang; then one SuffixRepair absolute jump before escalate.
2. **Resolve ≠ barrier≠jump** — certified write-prefix replay / gas-equal skip without PC (rem write_replays only) to cut interpreter-seconds without plant TLS.
3. Do **not** re-enable Lean plant-on-Handler or live_prime inspect until (1) proves hang-free on 597 N≥5 with SoftWait Soft≪50.

## Artifacts
- `lab/results/abc-iter4-n10-sf-occ.json`, `abc-iter4-n10-flip.json`, `abc-iter4-n10.run.log` (primary)
- `lab/results/abc-iter4-sf-occ.json`, `abc-iter4-flip.json`, `abc-iter4.run.log` (N=5)

## Code touched
- `boundary.rs` — lite SSTORE plant capture; `try_apply_pending_pc_resume`; `pending_resume_armed` / `plant_tls_active`
- `chain/ethereum.rs` — install SSTORE wrap on build
- `vm.rs` — plant TLS gated to research inspect; jump arm research-only
- `pevm.rs` — hot-ℓ fanout writer serial-barrier select; `serial_barrier_clique` metric
- `scheduler.rs` — `is_aborting` helper (sibling park rejected)
- `metrics.rs` / `specfence_g7_smoke.rs` — hsstore / sb_clique export
- `tx_runner.rs` — JournalExt bound (Handler unchanged default loop)
- `mod.rs` — resolve blurb + exports
