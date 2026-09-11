# SpecFence A+B+C Iter 5 status

**Date:** 2026-09-08 (Asia/Shanghai)  
**Branch:** `specfence`  
**Base tip:** `81917d1` (Iter4)  
**Authority:** `specfence-abc-unified-protocol.md`, `specfence-abc-iter4-status.md`, `specfence-global-cc-evm-rethink.md`

## Mandate

Cut FullRestart interpreter-seconds / wall on 597 without Lean `live_prime` / `inspect_run` hang and without unlimited sibling barrier. Keep A BO Await + C Quiet|Storm; SoftWait Soft~0.

Iter5 bets named in Iter4:
1. Serial one-tx capture window → absolute jump  
2. Write-prefix skip via rem replays / journal FF without plant TLS  

## Attack chosen (evidence-backed): **(2) head-FF retain on escalate**

### Why not (1) this iter
Landed serial capture plumbing (WaitHard→SpecRead while `plant_tls_active`, stack clone on SSTORE plant, `jump_defer` claim API) and briefly armed Lean absolute jump after capture. **Falsified:** Lean absolute jump with empty-memory / incomplete restore broke **seq≡par** on multiple `specfence` tests (`absolute_jump_applied>0`). Memory clone remains in the Iter4 hang family. Jump apply still needs `inspect_run`/`initialize_interp`. Production Lean jump **left OFF**.

### What we landed (production)
1. **`ff_head` on escalate FullRestart** — when clearing sticky force_bind / RewindTo, retain armed `ff_resume.values` in `PartialRetryTable::ff_head`.
2. **`try_ff_storage` / `try_ff_basic`** accept head-FF (not only `is_rewind_resume`), still **origin-checked** against MvMemory.
3. Clear `ff_head` with `clear_ff` / success-path `clear_repair` companions.
4. **No plant TLS on Lean production path** (`capture_window=false`); stack plant + WaitHard demote kept as Iter6 hang-free jump plumbing.
5. A BO Await + C Quiet|Storm unchanged on normal path. SoftWait Soft stays 0.

## Multi-block table

### Primary: N=10 medians @8 (`abc-iter5-n10`)
| Block | SF wall med | OCC wall med | SoftWait Soft | SF aborts med | ff_hits (last) | vs Iter4 N=10 | vs Iter3 N=10 |
|------:|------------:|-------------:|--------------:|--------------:|---------------:|---------------|---------------|
| **597** | **14.1** | 3.7 | **0** | 183 | **1592** (vs Iter4 657) | **↓ vs 14.2** | ↑ vs 13.4 |
| **599** | 19.2 | 9.8 | **0** | 230 | — | ↓ vs 23.0 | ≈/↓ vs 19.7 |
| **097** | 13.3 | 6.7 | **0** | 232 | — | ≈/↑ vs 12.6 | ≈ |
| **598** | 2.1 | 1.2 | **0** | 16 | — | quiet OK | quiet OK |

### Secondary: N=5 (`abc-iter5`)
597 med **13.1** p90 14.4 SoftWait=0; hsstore=0; aj=0; jdef=0; **no hang**.  
Beat Iter3 13.4 and Iter4 14.1 on this draw.

`absolute_jump_applied=0`. SoftWait Soft **0**. No hang on 597/599.

Tests: `cargo test -p pevm --lib` **96 ok**; `--test specfence` **23 ok / 13 ignored**.

## Iter 5 diagnosis (required answers)

### 1. detect / avoid / resolve — which hole now?
**Resolve still primary, but narrowed.** Head-FF proves certified-prefix **DB work** was real waste on FullRestart (ff_hits ~2.4× Iter4 on 597) yet **wall only barely moved** on N=10 (14.1 vs 14.2; still above Iter3 13.4). So remaining FullRestart cost is **interpreter opcode seconds / schedule**, not MV/storage I/O on the certified prefix.  
**Avoid** OK (SoftWait Soft=0; BO Await kept).  
**Detect** OK.

### 2. Region / Fence / intra / inter
| Axis | Verdict |
|------|---------|
| **Region** | ℓ / `a` OK; certified-prefix values now survive escalate as head-FF. |
| **Fence** | Serial-barrier resolve fence kept; Lean absolute-jump fence **rejected** (seq≠par); capture-window WaitHard demote retained off-path. |
| **Intra** | Hot-only π unchanged; head-FF does not need plant TLS. |
| **Inter** | Quiet\|Storm unchanged; must not drive Lean plant/jump today. |

### 3. vs Iter3 / Iter4 / plateau
- N=5: **13.1** — win vs Iter3 13.4 and Iter4 14.1.  
- N=10: **14.1** — win vs Iter4 14.2; **no win vs Iter3 13.4**; stretch &lt;10 unmet.  
- SoftWait Soft **0**. No hang. aj=0.

### 4. Cause for Iter 6 (named)
**Named cause:** Head-FF correctly cuts certified-prefix DB on FullRestart (ff_hits↑) but **does not cut interpreter-seconds**; wall remains ~plateau. Hang-free absolute jump is still the only known lever for opcode skip, but Lean jump needs **correct stack+memory restore without plant×Await livelock** (serial capture window alone insufficient without memory; memory clone historically hangs).

**Iter6 bets (falsifiable):**
1. **Hang-free memory-lite jump** — e.g. post_sstore jump that restores stack + write_replays only and **refuses** jumps that need nonzero memory words; or bounded memory pages; under WaitHard-off capture window only. Must pass `specfence` seq≡par with aj&gt;0 on fixtures before 597 N≥5.
2. **Fewer FullRestarts** — value-stable RebindOnly / longer SuffixRepair without escalate when head-FF origins still match all certified reads (cut fb_reabort≪80) rather than paying head reexec at all.
3. Do **not** re-enable Lean jump with empty memory or plant-under-Await.

## Artifacts
- `lab/results/abc-iter5-n10-sf-occ.json`, `abc-iter5-n10-flip.json`, `abc-iter5-n10.run.log` (primary)
- `lab/results/abc-iter5-sf-occ.json`, `abc-iter5-flip.json`, `abc-iter5.run.log` (N=5 win draw)
- `lab/results/abc-iter5b-*` (value_snap-merge trial — **rejected**, outlier p90)

## Code touched
- `rem.rs` — `ff_head`; escalate retain; `ff_value` / `has_ff_head` / `clear_ff_head`; jump_defer claim API (unused in prod); unit test
- `vm.rs` — try_ff_* accepts head-FF; WaitHard demote when plant_tls_active; Lean jump OFF; capture_window=false
- `pevm.rs` — clear_ff_head on success; lean disable_jump after failed jumped resume
- `boundary.rs` — SSTORE plant stack capture (Iter6 plumbing)
- `metrics.rs` — `jump_defer` counter (plumbing)
- `mod.rs` — resolve blurb
- `specfence_g7_smoke.rs` — jdef/aj print
