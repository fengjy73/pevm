# SpecFence A+B+C Iter 30 status

**Date:** 2026-09-08 (Asia/Shanghai)  
**Branch:** `specfence`  
**Base tip:** `2d5249c` (Iter29)  
**Authority:** `specfence-abc-unified-protocol.md`, `specfence-abc-iter29-status.md`, `specfence-global-cc-evm-rethink.md`

## Mandate

Lean-safe nested apply default-on, **or** 597 arm gates (origin_unsafe/steps_over),
**or** schedule ≤12.1→<10.
SoftWait Soft=0; Nested apply dig OK but Lean default-on was seq≠par — fix Lean-safe
before default-on. No fra≥16 / yield deepen / SoftWait Soft / BO park / mass SNAP /
Validated skip. Keep tip≡FF + steps_cap + LAST_SNAP + nested stash dig.

## Root cause (diagnosed this iter)

1. **Iter29 default-on nested abs apply → Lean seq≠par** under concurrency —
   unrestricted depth bypass (≤8) + cumulative multi-addr `tip_sloads` patched onto
   the nested frame (stack↔FF reconcile across parent+callee slots).
2. **Lean-safe narrow gates restore seq≡par** — named Iter29→30 cause:
   - `tip_sloads` addr ≡ `target_address` (filter at consume; stash only homogeneous)
   - `call_depth ≤ 2` (depth bypass ≤8 → ≤2)
   - tip≡FF vs `PENDING_FF_READ_PRESENTS`
   Deep / multi-addr tips **credit** (hang-free), never PENDING frame_init defer.
3. **597 aj still blocked at arm gates** — dig: `steps_over` (steps≫2048) and
   `origin_unsafe` when would-be jump; nested Lean-safe apply fires on depth≤2
   homogeneous tips but **aj=0 on 597** under fan-out (599/097 aj>0).
4. **Broad stash (any tip_sloads) wall↑** — falsified vs homogeneous-only stash
   (keeps FF TLS across unmatched nested frames). Production = homogeneous stash.

## Attack landed (production = Lean-safe nested apply **default-on**)

| Fix | Where |
|-----|--------|
| `SPECFENCE_NESTED_BIND` default **ON** (opt-out `=0`) | `boundary.rs` |
| Lean-safe gates: addr≡target ∧ depth≤2 ∧ tip≡FF | `try_consume_nested_bind_resume` |
| Homogeneous-only stash; multi-addr → credit | `try_apply` code_hash mismatch |
| Depth bypass ≤8 → ≤2 | `try_apply_pending_pc_resume` |
| Keep tip≡FF + steps_cap + LAST_SNAP; SoftWait Soft~0 | unchanged |
| Iter30 Lean test (default-on seq≡par) | `tests/specfence.rs` |

### Measured / rejected this iter

| Trial | Result |
|-------|--------|
| Nested apply default-on + Lean-safe gates (homogeneous stash) | Lean **seq≡par** (3×); Soft=0; dig nested path hang-free |
| Broad stash (any tip_sloads) + filter-at-consume | dig `nested_match→apply_ok` depth=2; **wall↑** (~16) — **falsified for prod** |
| Homogeneous stash + filter + depth≤2 (production) | 597 N5 med **13.4** Soft=0 (↓ vs Iter29 14.7); min **12.1** |

## Multi-block table

SoftWait Soft=**0**. Primary SUCCESS = Lean-safe nested apply default-on (+ wall↓ vs Iter29).

### Primary production N=5 (`abc-iter30`)
| Block | SF wall med | OCC med | SoftWait Soft | aj | bcredit | notes |
|------:|------------:|--------:|--------------:|--:|--------:|-------|
| **597** | **13.4** | 3.6 | **0** | 0 | ~17 | min 12.1; Lean-safe default-on |
| **599** | **22.5** | 9.8 | **0** | **7** | ~1 | 599-safe; aj>0 |
| **097** | **13.6** | 5.8 | **0** | **2** | ~4 | safe |
| **598** | **2.1** | 1.3 | **0** | 0 | ~0 | quiet OK |

### Secondary N=10 (`abc-iter30-n10`)
| Block | SF wall med | SoftWait | aj | notes |
|------:|------------:|---------:|--:|-------|
| **597** | **13.2** | **0** | 0 | plateau; Soft=0 |
| **599** | **21.7** | **0** | **4** | safe |
| **097** | **13.7** | **0** | 2 | |
| **598** | **2.1** | **0** | 0 | |

### Dig: Lean-safe nested apply (default-on)
- Homogeneous path: stash→match→apply when depth≤2 ∧ tip≡FF; else credit.
- Broad-stash dig (`abc-iter30-dig2`): `nested_stash→nested_match tip_sloads=3->2 depth=2→apply_ok`; `nested_lean_refuse depth=6→credit`. EXIT=0; SoftWait Soft=0.

Tests: `cargo test -p pevm --lib` + `--test specfence` (serial) green; Iter30 Lean 3× green.

## Iter 30 diagnosis (required answers)

### 1. detect / avoid / resolve — which hole now?
**Resolve still primary for <10 / 597 aj.** Detect OK. Avoid: SoftWait Soft=0.
Named hole closed this iter: **Lean-safe nested abs apply default-on**. Remaining:
597 arm gates (`steps_over` / `origin_unsafe`) and schedule ≤12.1→<10.

### 2. Region / Fence / intra / inter
| Axis | Verdict |
|------|---------|
| **Region** | tip≡FF + steps_cap; nested tip identity via code_hash + addr≡target |
| **Fence** | SoftWait Soft dormant; nested apply Lean-gated default-on |
| **Intra** | LAST_SNAP clear; ResumePath; homogeneous stash + credit fallback |
| **Inter** | Quiet 598 OK; Storm 597 plateau; 599 aj specializes |

### 3. vs Iter29 / plateau
- Production: tip≡FF + steps_cap + LAST_SNAP + **Lean-safe nested apply default-on**.
- SoftWait Soft **0**. Lean seq≡par with default-on **proven**.
- 597 N5 **13.4** (↓ vs Iter29 14.7); stretch median ≤12.1 / <10 unmet (min hit 12.1).
- SUCCESS: **Lean-safe nested apply** + **SoftWait Soft=0** + **wall↓ vs Iter29** +
  **diagnosis complete**.

### 4. Cause for Iter 31 (named)
1. **597 arm gates** — reduce `origin_unsafe` / `steps_over` without Validated skip /
   fra≥16 / yield deepen / SoftWait Soft / BO park / mass SNAP (prefer shorter
   tip≡FF select / origin rebind hang-free); or
2. **Schedule ≤12.1→<10** on quieter samples — same bans; or
3. **Widen Lean-safe nested apply** without wall tax (e.g. depth≤2 multi-slot
   same-addr already covered; avoid broad multi-addr stash).
4. Keep Lean-safe nested default-on; tip≡FF; SoftWait Soft~0; LAST_SNAP clear.

## Artifacts
- Prod: `lab/results/abc-iter30-sf-occ.json`, `abc-iter30-n10-sf-occ.json`
- Flip: `abc-iter30-flip.json`, `abc-iter30-n10-flip.json`
- Logs: `abc-iter30.run.log`, `abc-iter30-n10.run.log`
- Dig: `abc-iter30-dig*.run.log`, `abc-iter30b.*` (broad-stash falsified)

## Code touched
- `specfence/boundary.rs` — Lean-safe nested default-on; gates; homogeneous stash
- `specfence/mod.rs` — Iter30 blurb
- `tests/specfence.rs` — Iter30 Lean + Iter29 pin NESTED_BIND=0
- `lab/notes/specfence-abc-unified-protocol.md` — Iter30 log
