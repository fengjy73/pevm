# SpecFence A+B+C Iter 2 status

**Date:** 2026-09-08 (Asia/Shanghai)  
**Branch:** `specfence`  
**Base tip:** `2928b12` (Iter1)  
**Authority:** `specfence-abc-unified-protocol.md`, `specfence-abc-iter1-status.md`, `specfence-global-cc-evm-rethink.md`

## Mandate

Make SuffixRepair resume apply hang-free absolute jump when safe (live `jump_snap` **without** the failed inspect-only live-prime path). Measure `absolute_jump_applied > 0` on 597, or cut resume EVM cost. Keep A (BO Await, SoftWait Soft~0) and C (quiet|storm). No ESTIMATE-prefix poison.

## What we tried (B resolve)

### Ladder (falsified under concurrency)
1. Narrow **live_prime**: open Lean `inspect_run` only on SuffixRepair resume with Storage|Basic FF prefix + `needs_live_capture` (not bare force_bind).
2. Preserve `needs_live_capture` across **BlockingOther** retries (take-on-first-attempt was wiping the flag before a full resume).
3. On force_bind reabort: **delay escalate** only when `preview_next_jump_safe` (live snap + `jump_is_safe`).
4. Widen `jump_is_safe` ERC-20 gates: bytecode ≤24KB; steps ≤2048 with Storage/write_replays.
5. Prefer **live** snaps in `build_continuation` (lite exact-`cp.k` no longer shadows live); always attach post-SSTORE live tip.

### Evidence
| Probe | Result |
|-------|--------|
| live_prime inspect | `inspector_steps_resume` ≫ 0 (capture runs) |
| After Blocking fix | `has_live_after=true` on completed primes |
| `preview_next_jump_safe` | **`jump_ready=true why=ok` observed** (tx=8 on 597) |
| Enabling delay-escalate + jump | **Hang / timeout** on SF@597 and flip→599 under concurrency |
| Storm-only live_prime | Still timed out on SF@597 |
| Inspect tax without successful jump | 597 wall 16–36ms (regress vs ~13 plateau) |

**Verdict:** The capture→jump ladder is **unsafe under concurrency** today. Hang matches historical “inspect-only live-prime / whole-block inspect” failure mode — not fixed by Storage+WR narrowing alone. Jump gates can be satisfied in isolation; applying the ladder livelocks/timeouts the block.

### Production Iter2 landing (safe)
- **live_prime OFF** in Lean (`capture_inspect = suffix_jump` only).
- **Classic escalate** restored: `was_force_bind || depth≥2`.
- Keep plumbing for Iter3: `needs_live_capture`, Blocking-preserve peek/take, `has_live_boundary`, `preview_next_jump_safe(_why)`, live-preferring snap select, SSTORE attach, widened gates, g7 export of `inspector_steps*` / `live_pc_resume_count`.
- A/C unchanged: SoftWait Soft=0; Quiet|Storm actuation intact.

## Multi-block table (N=5 medians, @8 cores) — `abc-iter2`

| Block | SF wall med | OCC wall med | SoftWait Soft | SF aborts med | aj | vs Iter1b / plateau |
|------:|------------:|-------------:|--------------:|--------------:|---:|---------------------|
| **597** | **13.9** | 3.5 | **0** | 168 | **0** | ≈ plateau (~13±1); Iter1b 13.3 |
| **599** | 19.7 | 10.3 | **0** | 242 | 0 | ~Iter1b 19.6 |
| **097** | 12.4 | 8.9 | **0** | 242 | 0 | ~Iter1b 12.6 |
| **598** | 2.2 | 1.2 | **0** | 14 | 0 | quiet OK |

`force_bind_reabort ≈ full_restart` still ~90 on 597. `absolute_jump_applied=0`. SoftWait Soft **0**. No hang on measured path.

Tests: `cargo test -p pevm --lib` **95 ok**; `--test specfence` **23 ok / 13 ignored**.

## Iter 2 diagnosis (required answers)

### 1. detect / avoid / resolve — which hole now?
**Resolve still primary** (unchanged shape): fb_reabort≈full_restart≈90 on 597; repair ≈ extra EVM incarnation.  
**Avoid** still OK on sticky/force_prefix/prior BO Await; SoftWait Soft=0.  
**Detect** OK.  
Iter2 proved the named bet (hang-free live snap → jump before escalate) is **blocked by inspect concurrency hang**, not by missing `jump_is_safe` predicates alone.

### 2. Region / Fence / intra / inter
| Axis | Verdict |
|------|---------|
| **Region** | ℓ / `a` OK; `armed_at_k` still under-used for SoftWait Soft (dormant). |
| **Fence** | BO Await fences live; SoftWait Soft dormant; absolute-jump fence **cannot arm** on Lean without inspect hang. |
| **Intra** | Hot-only π unchanged; does not cheapen FullRestart. |
| **Inter** | Quiet\|Storm actuates; must **not** drive live_prime today (storm∧inspect hung 597). |

### 3. vs 2928b12 / plateau (~13ms SoftWait=0)
**In band:** 597 median **13.9** (plateau ~13±1; Iter1b 13.3). SoftWait Soft **0**. Stretch &lt;10 / aj&gt;0 **not met**. Rejected live_prime variants (wall↑ or hang).

### 4. Cause for Iter 3 (named)
**Named cause:** Lean **live PC capture requires `inspect_run`**, and under multi-worker concurrency that path **hangs / livelocks** on 597 (and 599 when armed) even when narrowed to Storage|Basic FF + write_replay and even when `jump_is_safe` previews true. Classic escalate must stay or fb storms return.

**Iter3 bets (falsifiable, pick one):**
1. **Hang-free capture ≠ full inspect_run** — e.g. single-boundary snapshot hook / Handler-side PC sample without per-opcode Inspector, or serial-barrier execute for the one jumped tx only.
2. **Serial barrier resolve** for conflict tx after first SuffixRepair fail (skip jump) — cut FullRestart fan-out without inspect.
3. Do **not** re-enable Lean live_prime+delay-escalate until (1) or (2) is hang-free on 597 N≥5 with SoftWait Soft≪50.

## Artifacts
- `lab/results/abc-iter2-sf-occ.json`, `abc-iter2-flip.json`, `abc-iter2-smoke5.run.log`
- Rejected: `abc-iter2-smoke1/2` (inspect tax / 599 storm), `abc-iter2-dbg*` (jump_ready=ok then hang), `abc-iter2-smoke3/4` (timeout)

## Code touched
- `vm.rs` — live_prime plumbing (disabled); Blocking-preserve; ff_prefix; suffix_jump Basic|Storage
- `pevm.rs` — classic escalate; SuffixRepair marks `needs_live_capture`
- `rem.rs` — `has_live_boundary`, `preview_next_jump_safe(_why)`, live-preferring snap select
- `boundary.rs` — post-SSTORE attach; bytecode/steps widen for ERC-20
- `specfence_g7_smoke.rs` — export inspector/live_pc metrics
