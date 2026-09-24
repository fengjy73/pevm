# SpecFence A+B+C Iter 24 status

**Date:** 2026-09-08 (Asia/Shanghai)  
**Branch:** `specfence`  
**Base tip:** `1397355` (Iter23)  
**Authority:** `specfence-abc-unified-protocol.md`, `specfence-abc-iter23-status.md`, `specfence-global-cc-evm-rethink.md`

## Mandate

Cautious Bind-jump enable with wall proof (no SNAP tax), or deepen 599-safe
schedule toward <10. SoftWait Soft=0; refuse-if-stale kept; dig proved
aj>0∧fail=0 + 597 SNAP+JUMP no hang. Capture only on SuffixRepair resume path /
opt-in storm — **not every Handler run**.

## Root cause (diagnosed this iter)

1. **Mass SNAP tax is real on load** (Iter23 jump597 597 med ~27 with bsnap~1100)
   but **ResumePath capture ≈ Off wall** — no mass-path tax when TLS only on
   SuffixRepair resume / force_bind / needs_live_capture.
2. **Bind-jump enable works under concurrency** — 599 N10 recorded **aj=1** with
   refuse-if-stale; SoftWait Soft=0; no hang on N=5/10 cores.
3. **597 still aj≈0** — resume-captured tips mostly refused or credit-only
   (`bcredit`>0); discovery Mass tips also refuse on 597 → opcode cut does not
   yet move 597 makespan. Wall 597 N5 **13.0** (toward ≤12.3; stretch <10 unmet).
4. **fra≥16 deepen falsified** — 599 med/p90↑ vs fan≥32/64 keep.

## Attack landed (production enable = `SPECFENCE_BIND_SNAP=resume` + JUMP + Iter17/23; default Off)

| Fix | Where |
|-----|--------|
| `BindSnapMode::{Off,ResumePath,Mass}` | `boundary.rs` |
| Default **Off**; production **`=resume`**; Mass=`=1`; Off=`=0` | `bind_snap_mode` |
| JUMP follows capture mode unless `SPECFENCE_BIND_SNAP_JUMP=0` | `bind_snap_jump_enabled` |
| TLS only on repair_capture when ResumePath | `vm.rs` `use_bind_snap` |
| Keep refuse-if-stale tip_sloads↔FF; Iter23 fra≥32; SoftWait Soft~0 | unchanged |
| fra≥16 deepen trial | **falsified** — reverted |

### Measured / rejected this iter

| Trial | Result |
|-------|--------|
| `SPECFENCE_BIND_SNAP=resume` + JUMP | 597 N5 **13.0** Soft=0; ≈ Off 14.2; bsnap~400; **599 N10 aj=1** — **chosen** |
| Mass SNAP+JUMP (`=1`) | bsnap~1080; 599 wall↑ vs ResumePath — keep dig-only |
| Force Off (`=0`) | Soft=0 aj=0 bsnap=0; 597 N5 14.2 — baseline |
| fra pre-yield fan≥16 | 599 med **23.4** / p90 32.7 — **falsified** |

## Multi-block table

SoftWait Soft=**0**.

### Primary production N=5 (`abc-iter24` ResumePath)
| Block | SF wall med | OCC med | SoftWait Soft | aj | bsnap | notes |
|------:|------------:|--------:|--------------:|--:|------:|-------|
| **597** | **13.0** | 3.9 | **0** | 0 | ~400 | min 12.1; toward ≤12.3 |
| **599** | **21.1** | 9.2 | **0** | 0 | ~300 | OK vs Iter23 |
| **097** | **12.8** | 6.5 | **0** | 0 | ~240 | OK |
| **598** | **2.1** | 1.1 | **0** | 0 | ~10 | quiet OK |

### Secondary N=10 (`abc-iter24-n10`)
| Block | SF wall med | SoftWait | aj | notes |
|------:|------------:|---------:|--:|-------|
| **597** | **13.5** | **0** | 0 | |
| **599** | **21.9** | **0** | **1** | Bind-jump enabled fires |
| **097** | **15.4** | **0** | 0 | noisy |
| **598** | **2.4** | **0** | 0 | |

Tests: `cargo test -p pevm --lib` **97 ok**; `--test specfence` **31 ok / 20 ignored** (serial).

## Iter 24 diagnosis (required answers)

### 1. detect / avoid / resolve — which hole now?
**Resolve still primary for <10.** Bind-jump is **enabled** without mass SNAP tax
(ResumePath); aj fires on 599 but not 597 (refuse/credit). Avoid: SoftWait Soft=0;
fra≥32 kept; fra≥16 falsified. Detect OK.

### 2. Region / Fence / intra / inter
| Axis | Verdict |
|------|---------|
| **Region** | tip_sloads Bind grain; ResumePath = fence at repair incarnation |
| **Fence** | SoftWait Soft dormant; abs-jump production-on behind refuse-if-stale |
| **Intra** | Stock SSTORE; ResumePath SNAP; fra≥32 |
| **Inter** | Quiet 598 OK; Storm 597 **13.0** |

### 3. vs Iter23 / plateau
- Production enable: `SPECFENCE_BIND_SNAP=resume` SNAP+JUMP (default Off — Lean hang). SoftWait Soft **0**.
- Quiet 597 N5 **13.0** / N10 **13.5** (≈ Iter23 12.9/13.7; no mass tax vs Off).
- SUCCESS: **Bind-jump enabled** with wall proof (no mass SNAP tax) + Soft=0 +
  aj>0 on 599; clear falsification that enable alone reaches <10 / that fra≥16 helps.

### 4. Cause for Iter 25 (named)
1. **597-useful opcode cut** — discovery tips that pass refuse-if-stale without
   mass SNAP (e.g. single lite snap / storm-opt-in with hard bsnap budget), or
2. **New 599-safe schedule** toward ≤12.3→<10 — **not** fra≥16, yield deepen,
   SoftWait Soft, BO park, sticky-absorb, or default Mass SNAP.
3. Do **not** re-enable Mass SNAP by default; keep refuse-if-stale; keep SoftWait Soft~0.

## Artifacts
- Prod: `lab/results/abc-iter24-sf-occ.json`, `abc-iter24-n10-sf-occ.json`
- Off baseline: `abc-iter24-off-sf-occ.json`
- Falsified fra16: `abc-iter24-fra16-sf-occ.json`
- Mass dig: `abc-iter24-mass-sf-occ.json`

## Code touched
- `specfence/boundary.rs` — BindSnapMode; resume/mass/off; jump_enabled
- `vm.rs` — selective use_bind_snap; JUMP via bind_snap_jump_enabled
- `specfence/mod.rs` — Iter24 blurb + exports
- `chain/ethereum.rs` — install comment
- `pevm.rs` — fra≥16 trial reverted; keep ≥32
- `tests/specfence.rs` — Iter24 ResumePath + force-off; prior offs use `=0`
- `lab/notes/specfence-abc-unified-protocol.md` — Iter24 log
