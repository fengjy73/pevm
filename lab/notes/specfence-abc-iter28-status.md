# SpecFence A+B+C Iter 28 status

**Date:** 2026-09-08 (Asia/Shanghai)  
**Branch:** `specfence`  
**Base tip:** `4d13617` (Iter27)  
**Authority:** `specfence-abc-unified-protocol.md`, `specfence-abc-iter27-status.md`, `specfence-global-cc-evm-rethink.md`

## Mandate

Hang-free first-frame tip identity for 597 aj>0, **or** schedule ≤12.1→<10.
SoftWait Soft=0; keep tip≡FF overlap + steps_cap; Nested apply OFF (hung);
no fra≥16 / yield deepen / SoftWait Soft / BO park / mass SNAP / prefix-spin skip.

## Root cause (diagnosed this iter)

1. **597 Bind tips are nested (router→token)** — JUMP_DIG `apply_refuse code_hash`
   on frame0 after `frame_init`: snap bytecode lengths 845–13904 while first
   frame is `tx.to` (router/aggregator). Lean `CALL_DEPTH` stays 0, so depth
   cannot distinguish; **code_hash / target_address** is the identity.
2. **First-frame-only capture starved tip≡FF** — filtering nested SLOADs removed
   the Storage tips that refuse-if-stale / credit need on 597 → wall↑ (~18ms
   with multi-attach) and aj stayed 0.
3. **Hang-free nested apply falsified again** — defer PENDING on code_hash
   mismatch until matching nested `frame_init` (no wait spin) got `apply_ok`
   in dig, then **hung 597 under concurrency** (same family as Iter27 nested
   apply). Production stays refuse+clear on mismatch.
4. **LAST_SNAP is worker-TLS** — steal can carry nested/wrong-tx tips into
   `attach_current_live_snap` on the next rewind. Clear at Bind-snap TLS
   enter/exit; skip Lean `attach_current_live_snap` (ff_continuation already
   carries arm_rewind jump_snap).

## Attack landed (production = LAST_SNAP hygiene + diagnosis)

| Fix | Where |
|-----|--------|
| Clear LAST_SNAP at Bind-snap TLS enter/exit | `boundary.rs` `with_bind_snap_tls` |
| Skip Lean `attach_current_live_snap` on rewind | `vm.rs` |
| JUMP_DIG logs tip_sloads on code_hash refuse | `boundary.rs` `try_apply_pending_pc_resume` |
| Keep tip≡FF overlap + steps_cap; silent ResumePath; SoftWait Soft~0 | unchanged |
| Nested apply / defer-until-match | **falsified (hang)** — OFF |
| First-frame-only capture / multi-attach | **falsified (wall↑ / aj=0)** — OFF |
| Iter28 Lean test | `tests/specfence.rs` |

### Measured / rejected this iter

| Trial | Result |
|-------|--------|
| First-frame-only capture + multi-attach (28a) | 597 N5 **18.1↑**; aj=0 — **falsified** |
| First-frame-only + deepest-one (28b) | 597 N5 **13.2**; aj=0 — no aj win |
| Nested capture + prefer first_frame select (28c) | 597 N5 **13.2**; aj=0 |
| Restore tip_compact select (28d) | 597 N5 **14.6** (machine noise) |
| Defer PENDING until nested frame_init match (28e) | dig `apply_ok`; **hung 597** — **falsified** |
| LAST_SNAP clear + skip Lean attach (28) | SoftWait Soft=0; 599/097 aj>0; wall ≈ Iter27 rerun |

## Multi-block table

SoftWait Soft=**0**. Machine load this session: Iter27 tip remeasure N5 597 **14.1**
(vs original Iter27 writeup 12.1) — treat wall as noise-matched plateau.

### Primary production N=5 (`abc-iter28`)
| Block | SF wall med | OCC med | SoftWait Soft | aj | bsnap | notes |
|------:|------------:|--------:|--------------:|--:|------:|-------|
| **597** | **13.7** | 5.2 | **0** | 0 | ~110 | min 13.0; ≈ Iter27-rerun 14.1 |
| **599** | **21.5** | 10.2 | **0** | **9** | ~100 | 599-safe; aj>0 |
| **097** | **12.7** | 5.9 | **0** | **1** | ~60 | safe |
| **598** | **2.2** | 1.2 | **0** | 0 | ~0 | quiet OK |

### Secondary N=10 (`abc-iter28-n10`)
| Block | SF wall med | SoftWait | aj | notes |
|------:|------------:|---------:|--:|-------|
| **597** | **14.3** | **0** | 0 | min 12.4; plateau |
| **599** | **22.9** | **0** | **9** | safe |
| **097** | **12.5** | **0** | **1** | |
| **598** | **2.3** | **0** | 0 | |

Tests: `cargo test -p pevm --lib` + `--test specfence` (serial) green.

## Iter 28 diagnosis (required answers)

### 1. detect / avoid / resolve — which hole now?
**Resolve still primary for <10 / 597 aj.** Detect OK. Avoid: SoftWait Soft=0;
no BO park. Named hole: **597 tip identity is nested callee bytecode**, not
tx.to first frame; hang-free nested apply still open (defer-until-match hung).

### 2. Region / Fence / intra / inter
| Axis | Verdict |
|------|---------|
| **Region** | tip≡FF overlap kept; first-frame ≠ storage-hot ℓ on router morph |
| **Fence** | SoftWait Soft dormant; abs-jump refuse on code_hash mismatch |
| **Intra** | LAST_SNAP TLS hygiene; ResumePath SNAP deferred; stock SSTORE |
| **Inter** | Quiet 598 OK; Storm 597 plateau; 599 aj specializes |

### 3. vs Iter27 / plateau
- Production: tip≡FF overlap + steps_cap + LAST_SNAP clear + skip Lean attach.
- SoftWait Soft **0**. 597 aj still 0. Wall noise-matched to Iter27-rerun.
- SUCCESS: **SoftWait Soft=0** + **diagnosis complete** (597 aj / <12.1 unmet
  on this load; nested apply falsified).

### 4. Cause for Iter 29 (named)
1. **Hang-free nested Bind consume ≠ frame_init defer** — e.g. apply only after
   natural CALL enters matching code_hash *without* keeping PENDING across
   unrelated frames; or mid-tx journal-depth-matched apply with hard timeout;
   or select/credit tips only when `target_address` of tip matches fail-loc
   contract (not tx.to); **not** unbounded nested apply (hung Iter27/28e); or
2. **Schedule ≤12.1→<10** on quieter samples — **not** fra≥16, yield deepen,
   SoftWait Soft, BO park, sticky-absorb, mass SNAP, tip_sloads Validated skip,
   or first-frame-only capture.
3. Keep silent-default ResumePath; FF-only tip≡FF; SoftWait Soft~0.

## Artifacts
- Prod: `lab/results/abc-iter28-sf-occ.json`, `abc-iter28-n10-sf-occ.json`
- Flip: `abc-iter28-flip.json`, `abc-iter28-n10-flip.json`
- Logs: `abc-iter28.run.log`, `abc-iter28-n10.run.log`
- Dig/falsified: `abc-iter28a/b/c/d/e*.json`, `abc-iter28*-dig*.run.log`,
  `abc-iter27-rerun.run.log` (load baseline)

## Code touched
- `specfence/boundary.rs` — LAST_SNAP clear; JUMP_DIG tip_sloads on refuse
- `vm.rs` — skip Lean attach_current_live_snap
- `specfence/mod.rs` — Iter28 blurb
- `tests/specfence.rs` — Iter28 Lean
- `lab/notes/specfence-abc-unified-protocol.md` — Iter28 log
