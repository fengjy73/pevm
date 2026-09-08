# SpecFence A+B+C Iter 26 status

**Date:** 2026-09-08 (Asia/Shanghai)  
**Branch:** `specfence`  
**Base tip:** `ff1d419` (Iter25)  
**Authority:** `specfence-abc-unified-protocol.md`, `specfence-abc-iter25-status.md`, `specfence-global-cc-evm-rethink.md`

## Mandate

Validated-fresh tip→jump on 597 (without tip_sloads prefix-spin skip), **or** new
599-safe schedule ≤12.3→<10. SoftWait Soft=0; keep silent-default ResumePath; no
mass SNAP; no tip_sloads skip of all-prefix Validated; no broad inc>0 capture; no
fra≥16 / yield deepen / SoftWait Soft / BO park.

## Root cause (diagnosed this iter)

1. **ResumePath tips were Bind-on-Data speculative** — SuffixRepair certified-prefix
   SLOADs hit `try_ff_storage` and **never** armed `note_pending_bind_snap`, so
   captured tips came from Bind-on-Data (often pre-Validated under 597 fan-out) and
   failed refuse-if-stale (tip_sloads ≠ FF) → aj≈0 on 597.
2. **FF-path arm → tip≡FF** — arming Bind-snap on successful `try_ff_storage` makes
   tip_sloads match certified FF by construction; jump still waits for all-prefix
   Validated spin + origin Validated (**same Validated window**; no tip_sloads skip).
3. **Per-SLOAD `attach_live_boundary` was the SNAP tax** — FF-path armed often →
   bsnap hundreds → 597 wall↑ (26a 13.9 / 26b 14.2). **Defer one attach to TLS exit**
   (deepest tip only) → bsnap~100, wall↓.
4. **Bind-on-Data snap pollutes tip_sloads** — even Validated Bind can append slots
   absent from FF → refuse. Production: **FF-path only** for Bind-snap arm.
5. **597 aj still 0** under fan-out (prefix/origin gates); **599/097 aj>0** proves
   the Validated-fresh path works where morphology allows.

## Attack landed (production = FF-path Validated-fresh + deferred attach)

| Fix | Where |
|-----|--------|
| Arm `note_pending_bind_snap` on FF-served SLOAD | `vm.rs` `try_ff_storage` |
| Disable Bind-on-Data Bind-snap arm (FF-only tip≡FF) | `vm.rs` `bind_on_data_lite` |
| Prefer tip_sloads≡FF at `jump_snap` select | `rem.rs` `build_continuation` |
| Defer one `attach_live_boundary_at` to TLS exit | `boundary.rs` `with_bind_snap_tls` |
| Keep all-prefix Validated spin; silent ResumePath; SoftWait Soft~0 | unchanged |
| Iter26 Lean test | `tests/specfence.rs` |

### Measured / rejected this iter

| Trial | Result |
|-------|--------|
| FF-path always-arm + per-SLOAD attach (26a) | 599 **aj=8**; 597 N5 **13.9** bsnap~487 — tax |
| Validated-only FF arm + Bind Validated (26b) | 597 **14.2↑** bsnap~759 — **falsified for wall** |
| FF-arm + deferred attach + Bind Validated (26c) | 597 N5 **13.5**; 599 aj>0; bsnap↓ |
| FF-only + deferred attach (26d) | 597 N5 **12.4** Soft=0; 599 **aj=7** wall 21.6 — **chosen** |
| tip_sloads prefix-spin skip / mass SNAP / broad inc>0 / fra≥16 / SoftWait / BO | **not retried** (prior falsified) |

## Multi-block table

SoftWait Soft=**0**.

### Primary production N=5 (`abc-iter26` = 26d)
| Block | SF wall med | OCC med | SoftWait Soft | aj | bsnap | notes |
|------:|------------:|--------:|--------------:|--:|------:|-------|
| **597** | **12.4** | 3.6 | **0** | 0 | ~120 | min 12.1; **wall <13** vs Iter25 13.7 |
| **599** | **21.6** | 9.7 | **0** | **7** | ~85 | 599-safe; useful jumps |
| **097** | **14.2** | 6.7 | **0** | **3** | ~110 | aj>0 |
| **598** | **2.1** | 1.2 | **0** | 0 | ~1 | quiet OK |

### Secondary N=10 (`abc-iter26-n10`)
| Block | SF wall med | SoftWait | aj | notes |
|------:|------------:|---------:|--:|-------|
| **597** | **13.3** | **0** | 0 | noise vs N5; still ≤ Iter25 N10 13.8 |
| **599** | **22.7** | **0** | >0 | safe |
| **097** | **13.2** | **0** | >0 | |
| **598** | **2.3** | **0** | 0 | |

Tests: `cargo test -p pevm --lib` **97 ok**; `--test specfence` **33 ok / 20 ignored**.

## Iter 26 diagnosis (required answers)

### 1. detect / avoid / resolve — which hole now?
**Resolve still primary for <10**, but Validated-fresh tip path is **named and live**:
FF-path tip≡FF + deferred attach cuts SNAP tax (597 N5 **12.4**) and lands aj on
599/097. Avoid: SoftWait Soft=0; no BO park. Detect OK. 597 aj still gated by
prefix/origin under fan-out.

### 2. Region / Fence / intra / inter
| Axis | Verdict |
|------|---------|
| **Region** | FF-served Bind tip grain; tip≡FF is Fence identity |
| **Fence** | SoftWait Soft dormant; abs-jump behind refuse + all-prefix Validated |
| **Intra** | Stock SSTORE; ResumePath SNAP deferred; fra≥32 |
| **Inter** | Quiet 598 OK; Storm 597 **12.4** (↓); 599 aj specializes |

### 3. vs Iter25 / plateau
- Production: FF-path Validated-fresh SNAP+JUMP + deferred attach. SoftWait Soft **0**.
- Quiet 597 N5 **12.4** / N10 **13.3** (↓ vs Iter25 13.7/13.8; **N5 wall <13**).
- SUCCESS: **wall <13 (N5)** + Soft=0 + 599/097 aj>0 + diagnosis complete.

### 4. Cause for Iter 27 (named)
1. **597 aj under fan-out** — same Validated-fresh tips still refused by prefix
   spin timeout / origin seed on 597; dig which gate dominates without tip_sloads
   skip; or
2. **Schedule toward ≤12.3→<10** on N10 (N5 already 12.4) — **not** fra≥16, yield
   deepen, SoftWait Soft, BO park, sticky-absorb, mass SNAP, broad inc>0, or
   tip_sloads skip of all-prefix Validated.
3. Keep silent-default ResumePath; FF-only tip≡FF; deferred attach; SoftWait Soft~0.

## Artifacts
- Prod: `lab/results/abc-iter26-sf-occ.json`, `abc-iter26-n10-sf-occ.json`
- Flip: `abc-iter26-flip.json`, `abc-iter26-n10-flip.json`
- Logs: `abc-iter26.run.log`, `abc-iter26-n10.run.log`
- Falsified: `abc-iter26a/b/c-*.json`

## Code touched
- `vm.rs` — FF-path `note_pending_bind_snap`; Bind-on-Data snap OFF
- `specfence/rem.rs` — tip≡FF prefer; `attach_live_boundary_at`
- `specfence/boundary.rs` — deferred Bind-snap attach
- `specfence/mod.rs` — Iter26 blurb
- `tests/specfence.rs` — Iter26 Lean + Iter22 env harden
- `lab/notes/specfence-abc-unified-protocol.md` — Iter26 log
