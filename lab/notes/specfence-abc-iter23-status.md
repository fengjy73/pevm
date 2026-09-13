# SpecFence A+B+C Iter 23 status

**Date:** 2026-09-08 (Asia/Shanghai)  
**Branch:** `specfence`  
**Base tip:** `c47c5dd` (Iter22)  
**Authority:** `specfence-abc-unified-protocol.md`, `specfence-abc-iter22-status.md`, `specfence-global-cc-evm-rethink.md`

## Mandate

Diff-first restore vs cold SuffixRepair to get stable aj>0∧seq≡par; OR non-jump
opcode cut / 599-safe schedule toward wall ≤12.3→<10. SoftWait Soft=0; JUMP/SNAP
OFF until dig fail=0 and 597 no-hang. Keep Iter16–17 production path.

## Root cause (diagnosed this iter)

1. **Diff-first on aj>0∧seq≠par:** parallel **reverts** (`status=false`) with
   **dgas=+661**, logs=0 — not pure gas-only. Token storage missing vs sequential
   success. SNAP-only (cold SuffixRepair) stays **seq≡par** 4/4.
2. **Stale Bind SLOAD values already consumed** — tip stack patch cannot fix
   `require(fromBalance >= amount)` / SUB results computed from earlier discovery
   Bind values that later diverge from certified FF under pevm MV.
3. **Refuse-if-stale gate** — when `tip_sloads` present, refuse abs jump unless
   every (addr,slot,value) equals certified FF; else cold SuffixRepair. Dig then
   **aj>0 ∧ fail=0** stably (3×16-run rounds). Empty `tip_sloads` still allowed
   for legacy M1f/Inspector snaps.
4. **597 SNAP+JUMP:** no hang (N=2 smoke); SoftWait Soft=0; aj≈0 on 597 (refuse
   gate); capture tax wall↑ → production SNAP/JUMP stay OFF.
5. **Non-jump wall:** Iter17 yield trim 64→32 **falsified** (599↑). High-fan
   (≥32) first-repair **pre-yield skip-park** kept — park_ms↓; 597 N5 **12.9**.

## Attack landed (production = Iter17 + Iter23 fra pre-yield; JUMP/SNAP OFF)

| Fix | Where |
|-----|--------|
| `tip_sloads` cumulative Bind SLOAD log | `boundary.rs` |
| Refuse Bind jump when tip_sloads ≠ FF | `try_arm_safe_absolute_jump_gated` |
| Apply-time stack↔FF reconcile (defense) | `try_apply_pending_pc_resume` |
| Journal warm prefer_tx = target else **min** | `boundary.rs` |
| High-fan (≥32) fra pre-yield skip-park | `pevm.rs` |
| Iter23 production-off + diff-first dig | `tests/specfence.rs` |
| Keep Iter16 absorb + Iter17 yield 64/32; SoftWait Soft~0; SNAP/JUMP OFF | unchanged |

### Measured / rejected this iter

| Trial | Result |
|-------|--------|
| Stack patch tip-only / cumulative without refuse | Still revert dgas=+661 — **insufficient** |
| Refuse when tip_sloads ≠ FF | Dig **aj>0∧fail=0** stable; 597 no-hang — **chosen (dig)** |
| Refuse when tip_sloads empty | Broke M1f unit test / aj=0 — **relaxed** |
| Yield trim 64/32→32/16 | 599 wall↑ — **falsified** |
| fra pre-yield fan≥32 | 597 N5 **12.9**; 599 OK — **kept** |

## Multi-block table

SoftWait Soft=**0**; aj=0; bsnap=0 on production.

### Primary production N=5 (`abc-iter23`)
| Block | SF wall med | OCC med | SoftWait Soft | aj | notes |
|------:|------------:|--------:|--------------:|---:|-------|
| **597** | **12.9** | 3.6 | **0** | 0 | min 12.5; ↓ vs Iter22 13.5; toward ≤12.3 |
| **599** | **22.5** | 9.9 | **0** | 0 | p90 noisy 31.4 |
| **097** | **12.0** | 6.2 | **0** | 0 | ↓ |
| **598** | **2.2** | 1.1 | **0** | 0 | quiet OK |

### Secondary N=10 (`abc-iter23-n10`)
| Block | SF wall med | SoftWait | aj |
|------:|------------:|---------:|--:|
| **597** | **13.7** | **0** | 0 |
| **599** | **19.7** | **0** | 0 |
| **097** | **13.9** | **0** | 0 |
| **598** | **2.2** | **0** | 0 |

Dig: ERC-20 SNAP+JUMP **STABLE** aj>0∧fail=0 (rounds: 11/11, 14/14, 2/2, 2/2).
597 SNAP+JUMP smoke: **no hang**; Soft=0; production stays OFF (capture tax).

Tests: `cargo test -p pevm --lib` **97 ok**; `--test specfence` **29 ok / 19 ignored**.

## Iter 23 diagnosis (required answers)

### 1. detect / avoid / resolve — which hole now?
**Resolve still primary for <10**, but Bind-jump restore hole is **named and gated**:
stale consumed SLOAD → refuse jump → cold SuffixRepair ≡ seq. Dig proves
aj>0∧seq≡par when tips match FF. Avoid: fra pre-yield trims park on high-fan;
SoftWait Soft=0. Detect OK. Production opcode cut still OFF (SNAP tax).

### 2. Region / Fence / intra / inter
| Axis | Verdict |
|------|---------|
| **Region** | tip_sloads Bind grain OK; refuse-if-stale is Fence at restore |
| **Fence** | SoftWait Soft dormant; abs-jump dig-safe OFF in prod |
| **Intra** | Stock SSTORE; SNAP/JUMP OFF; fra pre-yield on |
| **Inter** | Quiet 598 OK; Storm 597 **12.9** quiet |

### 3. vs Iter22 / plateau
- Production: Iter17 tip + fra pre-yield; SNAP/JUMP OFF. SoftWait Soft **0**.
- Quiet 597 N5 **12.9** / N10 **13.7** (↓ vs Iter22 13.5/13.1 band toward ≤12.3).
- SUCCESS: **stable jump dig** (aj>0∧fail=0) + Soft=0 + diagnosis complete;
  597 SNAP+JUMP no-hang; wall modest↓ via fra pre-yield.

### 4. Cause for Iter 24 (named)
1. **Cautious Bind-jump enable** under concurrency once wall with SNAP proven
   ≤12.3 without capture tax (or SNAP only on SuffixRepair resume incarnations).
2. Or **deepen 599-safe schedule** / opcode cut toward <10 without SoftWait Soft /
   BO park / yield deepen / sticky-absorb.
3. Do **not** default-on SNAP (capture tax), SoftWait Soft, fan BO park, SSTORE
   plant, or arm Bind abs jump without tip_sloads↔FF gate.

## Artifacts
- Prod: `lab/results/abc-iter23-sf-occ.json`, `abc-iter23-n10-sf-occ.json`
- Digs: `abc-iter23-jump597-sf-occ.json`; ERC-20 dig STABLE aj>0∧fail=0

## Code touched
- `specfence/boundary.rs` — tip_sloads; refuse-if-stale; prefer_tx min; apply reconcile
- `pevm.rs` — high-fan fra pre-yield skip-park
- `specfence/mod.rs` — Iter23 blurb
- `tests/specfence.rs` — Iter23 production-off + diff-first dig
- `lab/notes/specfence-abc-unified-protocol.md` — Iter23 log
