# SpecFence A+B+C Iter 27 status

**Date:** 2026-09-08 (Asia/Shanghai)  
**Branch:** `specfence`  
**Base tip:** `3d3052f` (Iter26)  
**Authority:** `specfence-abc-unified-protocol.md`, `specfence-abc-iter26-status.md`, `specfence-global-cc-evm-rethink.md`

## Mandate

597 aj under fan-out without tip_sloads prefix-spin skip, **or** N10 schedule ≤12.3→<10.
SoftWait Soft=0; keep tip≡FF Bind-snap (Iter26); no fra≥16 / yield deepen / SoftWait Soft /
BO park / mass SNAP.

## Root cause (diagnosed this iter)

1. **Credit≫arm on 597** — JUMP_DIG: depth always ≤1; mass refuse was `steps_over`
   (select maximized `opcode_steps` into >2048) and `bytecode_no_storage_ff` /
   refuse-if-stale when **cumulative** `tip_sloads` had extras absent from certified
   FF `values` (old all-match required every log entry).
2. **tip≡FF overlap** — ≥1 tip_sload matches FF Storage and none conflict → tip≡FF;
   missing FF entry OK; conflict still refuses. Unlocks arm path without prefix skip.
3. **steps_cap select** — prefer tip≡FF ∧ steps∈(0,cap] (cap 2048/128) then highest k;
   stop preferring max steps (was selecting `steps_over` tips).
4. **Deeper all-prefix Validated spin** (8192→32768) — same gate, more budget under
   fan-out; **not** tip_sloads skip.
5. **597 aj still 0** — when arm succeeds, first-frame `code_hash` often mismatches
   (Lean CALL_DEPTH stuck at 0; tips from nested CALLs). Nested apply-on-mismatch
   **hung 597** → production OFF. origin_unsafe / prefix_timeout also refuse some arms.
6. **Wall win without 597 aj** — overlap + steps_cap + deeper spin → 597 N5 **12.1** /
   N10 **12.5** (↓ vs Iter26 12.4 / 13.3); SoftWait Soft=0; 599/097 aj>0 safe.

## Attack landed (production = tip≡FF overlap + steps_cap + deeper prefix)

| Fix | Where |
|-----|--------|
| tip≡FF = ≥1 overlap, no conflict (extras OK) | `rem.rs` select; `boundary.rs` refuse-if-stale |
| Prefer tip≡FF ∧ steps≤cap (not max steps) | `rem.rs` `build_continuation` |
| tip_sload_ff counts as storage for large-bytecode / max_steps | `boundary.rs` `jump_is_safe` |
| All-prefix Validated spin 8192→32768 | `vm.rs` |
| Keep FF-only tip≡FF arm; silent ResumePath; SoftWait Soft~0 | unchanged |
| Nested apply / keep-pending on code_hash mismatch | **falsified (hang)** — OFF |
| Compact tip_sloads≤4 arm gate | measured then dropped (no aj win) |
| Iter27 Lean test | `tests/specfence.rs` |

### Measured / rejected this iter

| Trial | Result |
|-------|--------|
| tip≡FF overlap + steps_cap only (27a) | more try_arm; 597 still aj=0 (code_hash) |
| Nested apply + defer on hash mismatch (27b/c) | **hung 597** — **falsified** |
| Compact tip_sloads≤4 gate (27d) | 597 N5 **12.7**; aj=0 — no win vs 26 |
| Overlap + steps_cap + deeper prefix (27e) | 597 N5 **12.1** Soft=0; N10 **12.5** — **chosen** |
| tip_sloads prefix-spin skip / fra≥16 / SoftWait / BO / mass SNAP | **not retried** |

## Multi-block table

SoftWait Soft=**0**.

### Primary production N=5 (`abc-iter27` = 27e)
| Block | SF wall med | OCC med | SoftWait Soft | aj | bsnap | notes |
|------:|------------:|--------:|--------------:|--:|------:|-------|
| **597** | **12.1** | 4.3 | **0** | 0 | ~100 | min 11.3; **↓ vs Iter26 12.4** |
| **599** | **21.3** | 9.8 | **0** | **10** | ~100 | 599-safe; aj↑ |
| **097** | **11.5** | 5.3 | **0** | **2** | ~66 | wall↓ vs Iter26 14.2 |
| **598** | **2.1** | 1.3 | **0** | 0 | ~0 | quiet OK |

### Secondary N=10 (`abc-iter27-n10`)
| Block | SF wall med | SoftWait | aj | notes |
|------:|------------:|---------:|--:|-------|
| **597** | **12.5** | **0** | 0 | **↓ vs Iter26 N10 13.3**; toward ≤12.3 |
| **599** | **22.1** | **0** | >0 | safe |
| **097** | **12.0** | **0** | 0 | wall↓ vs Iter26 |
| **598** | **2.1** | **0** | 0 | |

Tests: `cargo test -p pevm --lib` + `--test specfence` (run at commit).

## Iter 27 diagnosis (required answers)

### 1. detect / avoid / resolve — which hole now?
**Resolve still primary for <10 / 597 aj.** Detect OK. Avoid: SoftWait Soft=0; no BO
park. Named progress: refuse/select no longer discard Validated-fresh tips for
cumulative tip_sloads extras or steps_over ranking; deeper Validated spin kept.
597 aj blocked by **first-frame code_hash** (nested tip / Lean CALL_DEPTH=0), not
by tip≡FF identity.

### 2. Region / Fence / intra / inter
| Axis | Verdict |
|------|---------|
| **Region** | tip≡FF overlap is Fence identity (extras ≠ conflict) |
| **Fence** | SoftWait Soft dormant; abs-jump behind refuse + all-prefix Validated |
| **Intra** | Stock SSTORE; ResumePath SNAP deferred; steps_cap select |
| **Inter** | Quiet 598 OK; Storm 597 **12.1** (↓); 599 aj specializes |

### 3. vs Iter26 / plateau
- Production: tip≡FF overlap + steps_cap select + deeper prefix spin. SoftWait Soft **0**.
- 597 N5 **12.1** / N10 **12.5** (↓ vs Iter26 12.4/13.3).
- SUCCESS: **597 wall↓** + Soft=0 + diagnosis complete (597 aj still 0).

### 4. Cause for Iter 28 (named)
1. **597 first-frame tip identity for aj>0** — hang-free nested apply (capped /
   hash-matched only) or capture/select tips whose code_hash matches tx.`to`
   first frame; keep all-prefix Validated (no tip_sloads skip); or
2. **Schedule ≤12.1→<10** on N5/N10 — **not** fra≥16, yield deepen, SoftWait Soft,
   BO park, sticky-absorb, mass SNAP, broad inc>0, or tip_sloads skip of all-prefix
   Validated; **not** unbounded nested apply (hung).
3. Keep silent-default ResumePath; FF-only tip≡FF; deferred attach; SoftWait Soft~0.

## Artifacts
- Prod: `lab/results/abc-iter27-sf-occ.json`, `abc-iter27-n10-sf-occ.json`
- Flip: `abc-iter27-flip.json`, `abc-iter27-n10-flip.json`
- Logs: `abc-iter27.run.log`, `abc-iter27-n10.run.log`
- Dig/falsified: `abc-iter27a/b/c/d-*.json`, `abc-iter27*-dig*.run.log`

## Code touched
- `specfence/rem.rs` — tip≡FF overlap; steps_cap jump_snap select
- `specfence/boundary.rs` — refuse-if-stale overlap; tip_sload_ff in jump_is_safe
- `vm.rs` — deeper all-prefix Validated spin; JUMP_DIG (env-gated)
- `specfence/mod.rs` — Iter27 blurb
- `tests/specfence.rs` — Iter27 Lean
- `lab/notes/specfence-abc-unified-protocol.md` — Iter27 log
