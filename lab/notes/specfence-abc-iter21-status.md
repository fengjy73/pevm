# SpecFence A+B+C Iter 21 status

**Date:** 2026-09-08 (Asia/Shanghai)  
**Branch:** `specfence`  
**Base tip:** `eb0f17f` (Iter20)  
**Authority:** `specfence-abc-unified-protocol.md`, `specfence-abc-iter20-status.md`, `specfence-global-cc-evm-rethink.md`

## Mandate

Minimal Storage-FF Bind jump hang repro — fix hang with seq≡par first on
fixtures, then cautious enable; OR non-jump opcode cut / 599-safe critical-path.
Goal: 597 median **<12** toward <10 without hang; SoftWait Soft=0; SNAP/JUMP
default OFF; no capture-without-jump tax; keep Iter16–17; prefer quiet wall ≤~12.3.

## Root cause (diagnosed this iter)

1. **Bind abs jump applies but is seq≠par on ERC-20** — with `SPECFENCE_BIND_SNAP=1`
   + `SPECFENCE_BIND_SNAP_JUMP=1`, Lean ERC-20 cluster gets **aj≥1** and
   **committed state ≠ sequential** (aj=2, skipped=11, resume=68 in dig). Restore
   of PC/stack/memory/FF origins under pevm MV is not ≡ cold re-exec.
2. **aj metric was blind on Lean Bind path** — `record_pc_resume` only counted via
   PLANT TLS; Handler `run_exec_loop` Bind jumps had no PLANT → **aj=0 while jump
   applied**, masking seq≠par as “mysterious” wrong state / hang family.
3. **Matching MvMemory FF origins were seeded without Validated** — unfinished
   Data Bind seed under concurrency is the SoftWait/InconsistentRead livelock
   family (597 hang). Iter21 requires Validated on matching origins too.
4. **Stale PENDING_RESUME on worker TLS** — `with_bind_snap_tls` now clears
   resume state at enter/exit.
5. **SNAP-only is hang-free + seq≡par** (bsnap>0, aj=0) — capture itself is fine;
   JUMP consume is the broken piece.
6. **Tiny SLOAD fixture at width=1** is hang-free but aj=0 (no RAW SuffixRepair
   under pevm commit order without concurrent abort).

## Attack landed (production = Iter17 tip + Iter21 dig plumbing)

| Fix | Where |
|-----|--------|
| Env-gated JUMP arm (`suffix_jump = suffix_jump_would`) dig-only | `vm.rs` |
| Validated-all matching MvMemory origins before seed | `vm.rs` |
| call_depth≤1 Bind-jump gate | `vm.rs` |
| Clear stale PENDING_RESUME in `with_bind_snap_tls` | `boundary.rs` |
| Record aj/pc_resume via BIND_SNAP TLS when PLANT absent | `boundary.rs` |
| PENDING_FF_ORIGIN_SEEDS plumbing (deferred-seed helper) | `boundary.rs` |
| Lean digs: width1 / width2 / ERC-20 JUMP falsify / SNAP-only / production-off | `tests/specfence.rs` |
| Keep Iter16 absorb + Iter17 yield-spin; SoftWait Soft~0; SNAP/JUMP OFF | unchanged |

### Measured / rejected this iter

| Trial | Result |
|-------|--------|
| 21a: width=1 SNAP+JUMP tiny SLOAD | hang-free seq≡par; **aj=0** (no resume) — fixture limit |
| 21b: ERC-20 SNAP-only | bsnap>0, resume>0, Soft=0, **seq≡par** — capture OK |
| 21c: ERC-20 SNAP+JUMP | **aj>0 ∧ seq≠par** — **falsified** (keep JUMP OFF) |
| 21d: Validated-all + depth clear + aj metric fix | diagnosis complete; not enough for seq≡par |
| Fan BO park / SoftWait Soft / SSTORE plant / default SNAP | **not re-tried** |

## Multi-block table

Quiet machine (load ~2 after killing stale hung digs). SoftWait Soft=**0**;
aj=0; bsnap=0 on production.

### Primary production N=5 (`abc-iter21`)
| Block | SF wall med | OCC med | SoftWait Soft | aj | notes |
|------:|------------:|--------:|--------------:|---:|-------|
| **597** | **13.4** | 3.8 | **0** | 0 | min 12.5; ≤ Iter20 load-noise 15.1 |
| **599** | **20.3** | 9.2 | **0** | 0 | ↓ vs Iter20 ~36 load-noise |
| **097** | **12.3** | 6.1 | **0** | 0 | quiet OK |
| **598** | **2.3** | 1.1 | **0** | 0 | quiet OK |

### Secondary N=10 (`abc-iter21-n10`)
| Block | SF wall med | SoftWait | aj |
|------:|------------:|---------:|--:|
| **597** | **13.6** | **0** | 0 |
| **599** | **20.5** | **0** | 0 |
| **097** | **13.4** | **0** | 0 |
| **598** | **2.1** | **0** | 0 |

Tests: `cargo test -p pevm --lib` **97 ok**; `--test specfence` **27 ok / 17 ignored**.

## Iter 21 diagnosis (required answers)

### 1. detect / avoid / resolve — which hole now?
**Resolve still primary for <10.** Bind jump hang/seq≠par **understood**: apply can
fire (aj>0 after metric fix) but restore ≢ sequential on ERC-20 under pevm MV;
597 concurrency hang is the same family (poisoned origins / wrong mid-tx state →
InconsistentRead/Blocking livelock). Avoid (Iter17) plateau; SoftWait Soft=0.
Detect OK. Credit consume remains hang-free but not an opcode cut.

### 2. Region / Fence / intra / inter
| Axis | Verdict |
|------|---------|
| **Region** | ℓ / `a` OK; Bind-at-SLOAD tip correct; depth≤1 gate needed for top-level apply. |
| **Fence** | SoftWait Soft dormant; abs-jump dig-only OFF in prod; BO not widened. |
| **Intra** | Stock SSTORE; SNAP/JUMP OFF; sticky-absorb OFF. |
| **Inter** | Quiet 598 OK; Storm 597 ~13.4 quiet (plateau). |

### 3. vs Iter20 / plateau
- Production ≈ Iter17 tip (SNAP/JUMP OFF). SoftWait Soft **0**; aj=0; no hang; no default tax.
- Quiet 597 N5 **13.4** / N10 **13.6** (restored vs Iter20 load-noise ~15–17; near Iter16 12.3 band, stretch <12 unmet).
- SUCCESS path: **jump hang/seq≠par understood with minimal ERC-20 repro (aj>0∧seq≠par)**; Soft=0; diagnosis complete.

### 4. Cause for Iter 22 (named)
**Named cause:** Bind-snap abs jump restore is not pevm-MV-sequential on real
contracts (ERC-20 aj>0⇒seq≠par); opcode cut still blocked. Credit ≠ wall↓.

1. **Correct Bind-jump restore under pevm MV** — journal warm/cold, nested
   call_depth apply at matching frame, FF origin install only after successful
   `apply_to_interp`, stack/memory tip ≡ cold — prove ERC-20 aj>0∧seq≡par at
   width≥2 before any mainnet JUMP enable.
2. Or **non-jump opcode cut**: Handler fast-forward of certified-prefix SLOAD
   using Bind tip as cursor without full `absolute_jump` (e.g. try_ff-only
   short-circuit that does not rewrite PC).
3. Or **599-safe critical-path** removing SuffixRepair from 597 makespan without
   yield-deepening / BO park / sticky / ff_head / capture tax (quiet wall already
   ~13.4; need schedule that hits ≤12.3→<10).
4. Do **not** default-on Bind-snap, SoftWait Soft, fan BO park, SSTORE plant jump,
   ForceBind/park ff_head, sticky-absorb, mega-fan yield, or arm Bind abs jump
   under concurrency until ERC-20+597 aj>0∧seq≡par∧no-hang proven.

## Artifacts
- Prod: `lab/results/abc-iter21-sf-occ.json`, `abc-iter21-n10-sf-occ.json`
- Digs: ERC-20 JUMP seq≠par (aj≥1); SNAP-only seq≡par; width1 hang-free aj=0

## Code touched
- `specfence/boundary.rs` — TLS clear; aj via BIND_SNAP; FF seed helpers
- `specfence/mod.rs` — Iter21 blurb; export seed helpers
- `vm.rs` — env JUMP arm; Validated-all origins; depth≤1 gate
- `tests/specfence.rs` — Iter21 production-off + ignored digs
