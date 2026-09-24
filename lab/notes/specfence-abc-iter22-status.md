# SpecFence A+B+C Iter 22 status

**Date:** 2026-09-08 (Asia/Shanghai)  
**Branch:** `specfence`  
**Base tip:** `b08d3e7` (Iter21)  
**Authority:** `specfence-abc-unified-protocol.md`, `specfence-abc-iter21-status.md`, `specfence-global-cc-evm-rethink.md`

## Mandate

Correct Bind-jump restore so ERC-20 fixtures get **aj>0 ∧ seq≡par**, then cautious
597; OR non-jump opcode cut / 599-safe schedule toward wall ≤12.3→<10.
SoftWait Soft=0; SNAP default OFF; do not arm JUMP under concurrency until
aj>0∧seq≡par on fixtures and no hang on 597. Keep Iter16–17 absorb/yield-spin.

## Root cause (diagnosed this iter)

1. **Iter21 aj>0∧seq≠par on ERC-20 is still real and flaky** — after restore
   plumbing, digs still see ~30–70% seq≠par when `SPECFENCE_BIND_SNAP_JUMP=1`
   (occasional SUCCESS). Validated-prefix serialize before jump **does not**
   eliminate mismatches → bug is in mid-tx restore ≡ cold re-exec, not only
   concurrent MV races.
2. **Pre-seed FF origins + failed apply** can poison read_set — Iter22 clears
   seeded origins when `!resume_was_applied`.
3. **Jumped-past SLOADs leave EIP-2929 slots cold** — without journal warm of
   FF Storage/Basic presents, later SLOAD/SSTORE gas ≠ sequential. Landed
   `apply_ff_read_presents` on PC apply (callee tx_id, not HashMap::next).
4. **Matching-origin seed lacked value check** — Iter22 requires MV Data still
   equals FF value even when incarnation matches.
5. **Memory truncation** (snap `memory_words>0` but bytes empty from 8KiB cap)
   refused for Bind jump.
6. **Bind snap `opcode_steps` used rem_k** (effect ordinal 2–5) while tip PC is
   hundreds deep — fixed to prefer `pc` as skip-credit proxy.
7. **SNAP-only remains hang-free + seq≡par**; production SNAP/JUMP stay OFF
   (capture tax / jump unsafe). SoftWait Soft=0.

## Attack landed (production = Iter17 tip + Iter22 dig plumbing)

| Fix | Where |
|-----|--------|
| FF journal warm on Bind jump apply (`PENDING_FF_READ_PRESENTS`) | `boundary.rs` |
| Prefer callee `transaction_id` for warm slots; journal.depth>1 refuse | `boundary.rs` |
| Matching-origin MV value check; clear seeded origins on failed apply | `vm.rs` |
| Memory-truncation + live tip (pc/gas/stack) gates | `vm.rs` |
| Validated-prefix yield-spin before dig JUMP (no BO park) | `vm.rs` |
| Bind snap `opcode_steps` prefer PC | `boundary.rs` |
| Iter22 production-off Lean test; ignored ERC-20 stability dig | `tests/specfence.rs` |
| Keep Iter16 absorb + Iter17 yield-spin; SoftWait Soft~0; SNAP/JUMP OFF | unchanged |

### Measured / rejected this iter

| Trial | Result |
|-------|--------|
| Journal warm + deferred/pre-seed + value check | ERC-20 still **flaky** aj>0∧seq≠par — **not enable** |
| Validated-all-lower-tx prefix spin before JUMP | Still flaky — **not sufficient** |
| Fan BO park / SoftWait Soft / SSTORE plant / default SNAP | **not re-tried** |
| Non-jump opcode cut / 599-safe schedule | **not landed** (jump dig primary) |

## Multi-block table

Quiet-ish machine. SoftWait Soft=**0**; aj=0; bsnap=0 on production.

### Primary production N=5 (`abc-iter22`)
| Block | SF wall med | OCC med | SoftWait Soft | aj | notes |
|------:|------------:|--------:|--------------:|---:|-------|
| **597** | **13.5** | 4.1 | **0** | 0 | min 12.5; ≈ Iter21 13.4 / plateau |
| **599** | **21.3** | 10.1 | **0** | 0 | ≈ Iter21 20.3 |
| **097** | **13.5** | 5.7 | **0** | 0 | ~ |
| **598** | **2.2** | 1.3 | **0** | 0 | quiet OK |

### Secondary N=10 (`abc-iter22-n10`)
| Block | SF wall med | SoftWait | aj |
|------:|------------:|---------:|--:|
| **597** | **13.1** | **0** | 0 |
| **599** | **20.2** | **0** | 0 |
| **097** | **13.9** | **0** | 0 |
| **598** | **2.2** | **0** | 0 |

Tests: `cargo test -p pevm --lib` **97 ok**; `--test specfence` **28 ok / 18 ignored**.

## Iter 22 diagnosis (required answers)

### 1. detect / avoid / resolve — which hole now?
**Resolve still primary for <10.** Bind-jump restore plumbing improved (warm,
origin hygiene, tip gates) but **ERC-20 under concurrency is not stably
seq≡par** even behind Validated prefix — do **not** arm JUMP. Avoid (Iter17)
plateau; SoftWait Soft=0. Detect OK. Credit ≠ wall↓.

### 2. Region / Fence / intra / inter
| Axis | Verdict |
|------|---------|
| **Region** | ℓ / `a` OK; Bind tip grain OK; rem_k≠opcode depth clarified. |
| **Fence** | SoftWait Soft dormant; abs-jump dig-only OFF in prod; BO not widened. |
| **Intra** | Stock SSTORE; SNAP/JUMP OFF; sticky-absorb OFF. |
| **Inter** | Quiet 598 OK; Storm 597 ~13.5 quiet (plateau). |

### 3. vs Iter21 / plateau
- Production ≈ Iter17 tip (SNAP/JUMP OFF). SoftWait Soft **0**; aj=0; no hang; no default tax.
- Quiet 597 N5 **13.5** / N10 **13.1** (≈ Iter21 13.4/13.6; stretch <12 / <10 unmet).
- SUCCESS path: **diagnosis complete** — restore hole named with repro still
  flaky after warm/prefix gates; Soft=0; tests green. Fixture **stable**
  aj>0∧seq≡par **not** achieved → JUMP stays OFF.

### 4. Cause for Iter 23 (named)
**Named cause:** Bind-snap abs jump still does not reproduce cold re-exec under
pevm MV on ERC-20 even when all lower txs are Validated — stack/memory/gas/PC
restore + FF warm is insufficient (likely incomplete journal side effects /
access-list / non-FF stack dependencies, or tip not ≡ certified-prefix end).

1. **Diff-first restore** — on aj>0∧seq≠par capture per-tx gas_used, log count,
   and first differing storage write; bisect which snap field diverges vs cold
   SuffixRepair with same FF seeds (no PC jump).
2. Or **non-jump opcode cut**: Handler fast-forward of certified-prefix SLOAD
   using Bind tip as cursor without `absolute_jump` (try_ff-only path that does
   not rewrite PC) — prove wall↓ on 597.
3. Or **599-safe critical-path** removing SuffixRepair from 597 makespan without
   yield-deepening / BO park / sticky / ff_head / capture tax (quiet wall ~13.5;
   need ≤12.3→<10).
4. Do **not** default-on Bind-snap, SoftWait Soft, fan BO park, SSTORE plant jump,
   ForceBind/park ff_head, sticky-absorb, mega-fan yield, or arm Bind abs jump
   under concurrency until ERC-20 dig is **stably** aj>0∧seq≡par (fail=0 over
   ≥16 runs) **and** 597 SNAP+JUMP no-hang.

## Artifacts
- Prod: `lab/results/abc-iter22-sf-occ.json`, `abc-iter22-n10-sf-occ.json`
- Digs: ERC-20 JUMP still flaky (success+fail); SNAP-only seq≡par retained

## Code touched
- `specfence/boundary.rs` — FF read presents warm; tx_id; depth gate; PC opcode_steps
- `vm.rs` — value check; seed clear-on-fail; tip/memory gates; Validated-prefix dig spin
- `specfence/mod.rs` — Iter22 blurb
- `tests/specfence.rs` — Iter22 production-off + ignored stability dig
