# SpecFence A+B+C Iter 6 status

**Date:** 2026-09-08 (Asia/Shanghai)  
**Branch:** `specfence`  
**Base tip:** `9af132d` (Iter5)  
**Authority:** `specfence-abc-unified-protocol.md`, `specfence-abc-iter5-status.md`, `specfence-global-cc-evm-rethink.md`

## Mandate

Fewer FullRestarts / interpreter-seconds on 597 without SoftWait Soft storms and without Lean inspect/jump hang. Keep A BO Await + C Quiet|Storm; keep head-FF retain on escalate.

Iter6 bets named in Iter5 / user:
1. Widen RebindOnly (value-stable / Estimate→Data)
2. Raise SuffixRepair depth before escalate when head-FF / rewind available
3. Strengthen hot Await if it cuts first fb_reabort

## Attack chosen (evidence-backed combo)

### Landed (production)
1. **RebindOnly widen** — brief validate-path yield (≤48) for Estimate→Data; multi-origin Basic `value_snap` (origin=`None` so FF refuses lazy); `prior_read_value_stable` accepts last MvMemory in lazy chain + aborted-but-same-incarnation Data.
2. **One extra SuffixRepair when cheap resume** — if `is_rewind_resume` / `has_ff_resume_values` / `has_ff_head`, escalate only at `repair_depth >= 2` (ignore first `was_force_bind`). Classic path unchanged (`was_force_bind || depth>=2`). Head-FF retain on escalate kept (Iter5).
3. **A BO Await + C Quiet|Storm unchanged** on production path (see falsified below).

### Measured / rejected this iter
| Trial | Result |
|-------|--------|
| `depth>=3` when cheap resume | **fr↓~60%** but 597 N=5 wall **14.3** (↑ vs Iter5 13.1) — fb loops cost more than head-FF FullRestart |
| Storm unfinished Data `note_hot_touch` + `live_fanout_hot`-alone Await | aborts↓ but wall↑ / park idle; **reverted** |
| Spin-before-BO 96 | no wall win; kept 64 |

## Multi-block table

### Primary: N=5 (`abc-iter6` = resolve-only tip)
| Block | SF wall med | OCC wall med | SoftWait Soft | SF aborts med | full_restart (last) | fb_reabort (last) | vs Iter5 N=5 |
|------:|------------:|-------------:|--------------:|--------------:|--------------------:|------------------:|--------------|
| **597** | **13.0** | ~3.5 | **0** | 151 | **49** (vs Iter5 97) | 105 (vs 97) | **↓ wall; fr≈½** |
| **599** | 19.4 | ~9.8 | **0** | 219 | 69 | 119 | ≈ |
| **097** | **11.9** | ~5.8 | **0** | 220 | 51 | 84 | **↓ vs 12.6** |
| **598** | 2.2 | ~1.2 | **0** | 17 | 3 | 6 | quiet OK |

### Secondary: N=10 (`abc-iter6-n10`)
| Block | SF wall med | SoftWait | aborts med | full_restart | vs Iter5 N=10 |
|------:|------------:|---------:|-----------:|-------------:|---------------|
| **597** | **14.0** | **0** | 170 | **49** (vs 81) | **≈/↓ wall; fr↓~40%** |
| **599** | 20.4 | **0** | 228 | 62 | slight↑ wall |
| **097** | 13.1 | **0** | 216 | 55 | ≈/↓ |
| **598** | 2.3 | **0** | 17 | 2 | quiet OK |

`absolute_jump_applied=0`. SoftWait Soft **0**. No hang on 597/599.

Tests: `cargo test -p pevm --lib` **97 ok**; `--test specfence` **23 ok / 13 ignored**.

## Iter 6 diagnosis (required answers)

### 1. detect / avoid / resolve — which hole now?
**Resolve still primary, but shape changed.** One extra SuffixRepair under RewindTo/FF cuts **FullRestart count ~40–50%** on 597; wall only barely moves (N=5 13.0 vs 13.1; N=10 14.0 vs 14.1). So remaining cost is **SuffixRepair / resume interpreter-seconds + schedule**, not only FullRestart count. RebindOnly remains scarce (true_suffix + real value deltas).  
**Avoid** OK (SoftWait Soft=0; BO Await kept; storm fanout-Await strengthen **falsified** for wall).  
**Detect** OK.

### 2. Region / Fence / intra / inter
| Axis | Verdict |
|------|---------|
| **Region** | ℓ / `a` OK; cheap-resume gate uses RewindTo/FF at tx grain. |
| **Fence** | BO Await unchanged; SoftWait Soft dormant; no Lean jump. |
| **Intra** | Hot-only π unchanged; Estimate→Data spin is validate-local only. |
| **Inter** | Quiet\|Storm unchanged; must not drive SoftWait Soft / Lean plant. |

### 3. vs Iter5 / plateau
- N=5: **13.0** — slight win vs Iter5 13.1; **full_restart 97→49**.  
- N=10: **14.0** — ≈ Iter5 14.1; **full_restart 81→49**.  
- SoftWait Soft **0**. No hang. aj=0. Stretch &lt;10 unmet.

### 4. Cause for Iter 7 (named)
**Named cause:** Fewer FullRestarts via one extra SuffixRepair proves FR count was partly optional, but **wall ≈ plateau** because SuffixRepair resumes still pay interpreter-seconds (and fb_reabort stays ~high). RebindOnly cannot absorb true_suffix value-changing RAW. Hang-free opcode skip remains the only known lever for &lt;10, but Lean jump/memory still falsified.

**Iter7 bets (falsifiable):**
1. **Make the extra SuffixRepair succeed more often** — stronger sticky BO Await only on the *second* repair incarnation (force_prefix), not storm-wide fanout; cut fb_reabort without SoftWait Soft.
2. **RebindOnly on certified-prefix-only fails** — when true_suffix writes are empty *after* Estimate clears mid-validate, or value-stable on Storage-only invalid set (no Basic lazy).
3. **Hang-free memory-lite jump** only after seq≡par fixtures with aj&gt;0 (still forbidden empty-memory / plant×Await).
4. Do **not** re-enable depth≥3 or storm-wide live_fanout Await without wall proof.

## Artifacts
- `lab/results/abc-iter6-sf-occ.json`, `abc-iter6-flip.json`, `abc-iter6.run.log` (N=5 primary)
- `lab/results/abc-iter6-n10-sf-occ.json`, `abc-iter6-n10-flip.json`, `abc-iter6-n10.run.log`
- `lab/results/abc-iter6b-*` (depth≥2 + Await fanout — intermediate)
- First `abc-iter6` depth≥3 trial retained in git history / superseded by resolve-only tip

## Code touched
- `pevm.rs` — Estimate→Data spin; cheap-resume escalate at depth≥2
- `rem.rs` — `has_ff_resume_values`; unit test
- `mv_memory.rs` — widen `prior_read_value_stable` (multi-origin last MvMemory)
- `vm.rs` — multi-origin Basic value_snap (FF-safe origin=None)
- `mod.rs` — resolve blurb
