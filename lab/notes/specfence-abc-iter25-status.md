# SpecFence A+B+C Iter 25 status

**Date:** 2026-09-08 (Asia/Shanghai)  
**Branch:** `specfence`  
**Base tip:** `e3c1b3d` (Iter24)  
**Authority:** `specfence-abc-unified-protocol.md`, `specfence-abc-iter24-status.md`, `specfence-global-cc-evm-rethink.md`

## Mandate

597-useful opcode cut without mass SNAP, **or** new 599-safe schedule ≤12.3→<10,
**or hang-free silent-default ResumePath** (if Lean hang on silent JUMP fixed).
SoftWait Soft=0; keep ResumePath SNAP+JUMP (no mass SNAP=1); no fra≥16 / yield
deepen / SoftWait Soft / BO park regressing 599.

## Root cause (diagnosed this iter)

1. **Lean hang was Mass SNAP+JUMP**, not ResumePath — Iter19 hung Lean fixtures
   under `SPECFENCE_BIND_SNAP=1` + JUMP. ResumePath + refuse-if-stale is
   **hang-free and seq≡par** on Lean (32/32 non-ignored tests; Iter25 silent
   default fixture).
2. **Silent-default ResumePath is safe** — `bind_snap_mode()` default
   `None → ResumePath` (was Off). Production no longer needs `=resume` env.
   Force Off still `=0`; Mass dig still `=1`.
3. **tip_sloads skip of all-prefix Validated spin falsified** — Lean
   `p2_partial_retry` → **seq≠par** when jump armed without full prefix Validated.
   Keep Iter22 all-prefix spin.
4. **Broad ResumePath capture on all inc>0 falsified** — bsnap↑ (~600) wall↑
   (597 N5 **13.5**), aj still 0. Keep force_bind / SuffixRepair / needs_live_capture.
5. **597 aj still ≈0** — refuse-if-stale + prefix Validated still gate jumps;
   tips from repair path rarely match FF under fan-out. Stretch <10 unmet.
6. Wall **plateau** ≈ Iter24 (noise); SoftWait Soft=**0**; 599 safe.

## Attack landed (production = silent-default ResumePath SNAP+JUMP)

| Fix | Where |
|-----|--------|
| Default `BindSnapMode::ResumePath` when env unset | `boundary.rs` `bind_snap_mode` |
| Keep refuse-if-stale; all-prefix Validated spin; fra≥32; SoftWait Soft~0 | unchanged |
| tip_sloads prefix-spin skip | **falsified** — reverted |
| Broad inc>0 SNAP | **falsified** — reverted |
| Iter25 Lean silent-default test | `tests/specfence.rs` |

### Measured / rejected this iter

| Trial | Result |
|-------|--------|
| Silent-default ResumePath + JUMP | Lean **hang-free seq≡par**; Soft=0 — **chosen** |
| tip_sloads skip all-prefix Validated | Lean p2 **seq≠par** — **falsified** |
| ResumePath capture all inc>0 | 597 N5 **13.5** bsnap~600 aj=0 — **falsified** |
| Validated-only `note_pending_bind_snap` | bsnap~½ aj still 0 — **falsified** |

## Multi-block table

SoftWait Soft=**0**.

### Primary production N=5 (`abc-iter25` silent default)
| Block | SF wall med | OCC med | SoftWait Soft | aj | bsnap | notes |
|------:|------------:|--------:|--------------:|--:|------:|-------|
| **597** | **13.7** | 3.8 | **0** | 0 | ~310 | min 12.9; ≈ Iter24 plateau |
| **599** | **21.7** | 9.9 | **0** | 0 | ~330 | OK / 599-safe |
| **097** | **12.8** | 6.1 | **0** | 0 | ~250 | OK |
| **598** | **2.2** | 1.2 | **0** | 0 | ~8 | quiet OK |

### Secondary N=10 (`abc-iter25-n10`)
| Block | SF wall med | SoftWait | aj | notes |
|------:|------------:|---------:|--:|-------|
| **597** | **13.8** | **0** | 0 | |
| **599** | **22.6** | **0** | 0 | safe |
| **097** | **14.2** | **0** | 0 | |
| **598** | **2.3** | **0** | 0 | |

Tests: `cargo test -p pevm --lib` **97 ok**; `--test specfence` **32 ok / 20 ignored**.

## Iter 25 diagnosis (required answers)

### 1. detect / avoid / resolve — which hole now?
**Resolve still primary for <10.** Silent-default ResumePath removes env footgun
and is Lean-safe; aj still rare on 597 (refuse/prefix). Avoid: SoftWait Soft=0;
fra≥32 kept. Detect OK.

### 2. Region / Fence / intra / inter
| Axis | Verdict |
|------|---------|
| **Region** | tip_sloads Bind grain; silent ResumePath = fence at repair |
| **Fence** | SoftWait Soft dormant; abs-jump on behind refuse + prefix Validated |
| **Intra** | Stock SSTORE; ResumePath SNAP default; fra≥32 |
| **Inter** | Quiet 598 OK; Storm 597 **13.7** plateau |

### 3. vs Iter24 / plateau
- Production: **silent-default** ResumePath SNAP+JUMP (was env `=resume`). SoftWait Soft **0**.
- Quiet 597 N5 **13.7** / N10 **13.8** (≈ Iter24 13.0/13.5; noise, no wall win).
- SUCCESS: **hang-free silent-default ResumePath** + Soft=0 + diagnosis complete;
  clear falsification of tip_sloads prefix-skip and broad inc>0 SNAP.

### 4. Cause for Iter 26 (named)
1. **597-useful tip freshness** — capture Bind tips only when origin writer is
   already Validated *and* arm jump in the same Validated window (without
   tip_sloads prefix-spin skip), so refuse-if-stale passes on 597 fan-out; or
2. **New 599-safe schedule** toward ≤12.3→<10 — **not** fra≥16, yield deepen,
   SoftWait Soft, BO park, sticky-absorb, mass SNAP, broad inc>0 SNAP, or
   tip_sloads skip of all-prefix Validated.
3. Keep silent-default ResumePath; keep refuse-if-stale; SoftWait Soft~0.

## Artifacts
- Prod: `lab/results/abc-iter25-sf-occ.json`, `abc-iter25-n10-sf-occ.json`
- Flip: `abc-iter25-flip.json`, `abc-iter25-n10-flip.json`
- Logs: `abc-iter25.run.log`, `abc-iter25-n10.run.log`

## Code touched
- `specfence/boundary.rs` — default ResumePath
- `vm.rs` — Iter25 comments; falsified trials reverted
- `specfence/mod.rs` — Iter25 blurb
- `tests/specfence.rs` — Iter25 silent-default + Iter24 uses unset env
- `lab/notes/specfence-abc-unified-protocol.md` — Iter25 log
