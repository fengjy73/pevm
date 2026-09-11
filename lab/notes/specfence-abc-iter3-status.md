# SpecFence A+B+C Iter 3 status

**Date:** 2026-09-08 (Asia/Shanghai)  
**Branch:** `specfence`  
**Base tip:** `a031a58` (Iter2)  
**Authority:** `specfence-abc-unified-protocol.md`, `specfence-abc-iter2-status.md`, `specfence-global-cc-evm-rethink.md`

## Mandate

Attack the resolve hole (fb≈full_restart~90) **without** Lean `inspect_run` / live_prime. Preferred bet: **serial-barrier resolve** after first force_bind_reabort — park the escalated tx behind an unfinished conflict writer (or small clique), then FullRestart once against Data. Keep A (BO Await, SoftWait Soft~0) and C (Quiet|Storm). No hang.

## What we landed (B resolve)

### Serial-barrier resolve (production)
On Lean escalate (`was_force_bind` → FullRestart), **Storm only**, once per tx:
1. Find highest unfinished conflict writer of `invalid` ℓ that is actively **`Executing`**.
2. `add_dependency_from_aborting(consumer, writer)` — leave Aborting; writer `finish_execution` wakes → Ready FullRestart.
3. Metric `serial_barrier_resolve`. Cap 1/tx/block (anti-cascade).

### Rejected / bisected variants
| Variant | Result |
|---------|--------|
| Steal-first defer on every escalate | Park tax↑; 597 med **15.2** — rejected |
| Unlimited `!is_done` barrier + sticky-on-escalate | Rare cascade (evm~4500, wall~23) — rejected |
| Cap=3 `!is_done` | Park↑, med ~13.6 — no clear win vs once |
| Once + Executing, no Storm gate | Noisy N=5 (13.4–14.3); N=10 **13.4** |
| **Once + Executing + Storm** (landed) | Stable; sb_res>0; SoftWait=0; no hang |

### Not done (Iter2 hang risk)
- Lean `live_prime` + delay-escalate **still OFF**.
- Hang-free PC capture ≠ inspect_run **not** implemented (no rem Handler PC sample yet).

## Multi-block table

### Primary: N=10 medians @8 (`abc-iter3-n10`)
| Block | SF wall med | OCC wall med | SoftWait Soft | SF aborts med | sb_res (last) | vs Iter2 |
|------:|------------:|-------------:|--------------:|--------------:|--------------:|----------|
| **597** | **13.4** | 3.4 | **0** | 182 | ~19 | **↓ vs 13.9**; stretch &lt;10 **not met** |
| **599** | 19.7 | 9.7 | **0** | 243 | ~39 | ≈ Iter2 19.7 |
| **097** | 12.9 | 6.2 | **0** | 238 | ~39 | ≈ Iter2 12.4 |
| **598** | 2.3 | 1.2 | **0** | 15 | ~1 | quiet OK |

### Secondary: N=5 (`abc-iter3`) — noisier
597 med **14.1** p90 17.5 (schedule variance / park spikes). Prefer N=10 for 597 judgment.

`absolute_jump_applied=0`. SoftWait Soft **0**. No hang on 597/599.

Tests: `cargo test -p pevm --lib` **95 ok**; `--test specfence` **23 ok / 13 ignored**.

## Iter 3 diagnosis (required answers)

### 1. detect / avoid / resolve — which hole now?
**Resolve still primary.** Serial-barrier fires (`serial_barrier_resolve` ≫ 0 on storm blocks) and modestly helps 597 wall in N=10 (**13.4** vs Iter2 **13.9**), but fb_reabort/full_restart remain ~70–100 and repair is still FullRestart-class EVM. Stretch &lt;10 unmet.  
**Avoid** OK (SoftWait Soft=0; BO Await kept; sticky from first SuffixRepair). Aggressive barrier/sticky stacking caused WaitHard cascades — correctly capped.  
**Detect** OK.

### 2. Region / Fence / intra / inter
| Axis | Verdict |
|------|---------|
| **Region** | ℓ / `a` OK; barrier attaches to conflict writer of invalid ℓ (access-grain resolve fence). |
| **Fence** | New resolve fence = dependency barrier behind Executing writer; SoftWait Soft still dormant; absolute-jump still unarmed on Lean. |
| **Intra** | Hot-only π unchanged; does not cheapen FullRestart body. |
| **Inter** | Quiet\|Storm **gates** the barrier (C actuates B); Quiet keeps OCC-lite escalate path. |

### 3. vs Iter2 / plateau (~13.9 SoftWait=0)
**Slight down in N=10 (13.4)**; N=5 noisy around plateau. SoftWait Soft **0**. No hang. aj=0. **&lt;10 not met.**

### 4. Cause for Iter 4 (named)
**Named cause:** Serial-barrier removes some ESTIMATE-race FullRestarts but **does not cut interpreter-seconds of the remaining ~80 FullRestarts**. Jump still needs live PC/gas; Lean `inspect_run` hang is still the blocker for `absolute_jump_applied>0`.

**Iter4 bets (falsifiable):**
1. **Hang-free PC/gas at rem effect boundary without `inspect_run`** — Handler-side sample at SSTORE/call boundary (or gas-only write-prefix jump) so SuffixRepair can `jump_is_safe` before escalate; must prove no 597/599 hang N≥5.
2. **Stronger clique barrier** — serialize only the hot-ℓ consumer fan-out behind Validated writer (not global mutex); success = fb_reabort≪90 and 597 med&lt;10 with SoftWait Soft≪50.
3. Do **not** re-enable Lean live_prime+delay-escalate until (1) is hang-free.

## Artifacts
- `lab/results/abc-iter3-n10-sf-occ.json`, `abc-iter3-n10-flip.json`, `abc-iter3-n10.run.log` (primary)
- `lab/results/abc-iter3-sf-occ.json`, `abc-iter3-flip.json`, `abc-iter3.run.log` (N=5)
- Rejected: `abc-iter3-smoke1` (defer), `smoke2` (cascade), `smoke5` (cap=3)

## Code touched
- `scheduler.rs` — `add_dependency_from_aborting`, `finish_validation_fenced_barrier_park`, `is_executing`; defer helper kept unused
- `pevm.rs` — Storm∧was_force_bind serial-barrier on escalate FullRestart
- `rem.rs` — `serial_barrier_count` / `try_claim_serial_barrier` (cap 1)
- `metrics.rs` — `serial_barrier_resolve` / `serial_barrier_defer`
- `specfence_g7_smoke.rs` — export sb_res/sb_def
- `mod.rs` — resolve blurb
