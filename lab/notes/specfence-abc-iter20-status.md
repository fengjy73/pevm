# SpecFence A+B+C Iter 20 status

**Date:** 2026-09-08 (Asia/Shanghai)  
**Branch:** `specfence`  
**Base tip:** `3a9a945` (Iter19)  
**Authority:** `specfence-abc-unified-protocol.md`, `specfence-abc-iter19-status.md`, `specfence-global-cc-evm-rethink.md`

## Mandate

Hang-free **consume** Bind tips without full abs jump (FF/credit/partial restore
using Bind/SLOAD snap at k<k_fail), OR fix jump hang with seq≡par first. Goal:
597 median **<12.3** toward <10; SoftWait Soft=0; 599 safe. Keep Bind-snap
plumbing opt-in; default path ≤~12.3–13 wall (Iter19 high-load noise ~15 — no
default tax). No mega-fan yield; stock SSTORE; SoftWait Soft~0.

## Root cause (diagnosed this iter)

1. **Iter19 `!memory_lite_ok` left aj=0 on mainnet** — Bind snaps clone ≤8KiB
   memory, so the JUMP path never armed on 597 despite bsnap≈850.
2. **Removing the memory gate is necessary but not sufficient** — with memory
   allowed + Validated-safe FF origin seed, Bind-snap abs jump **still hangs 597
   SF** once `jump_is_safe` passes (90s timeout). Same inspect/jump family.
3. **Basic-only Bind tips refuse jump** — dig `bytecode_no_storage_ff`
   (bc_len≈3KB, vals_s=0, vals_b=2). Large bytecode requires Storage FF /
   write_replays / CALL-boundary; many read-prefix resumes are Basic-only.
4. **Hang-free credit consume works** — with `SPECFENCE_BIND_SNAP=1` and jump
   hard-OFF: bsnap≈844, **bcredit=6** on 597 (15/5 on 599/097), Soft=0, **no
   hang**. Credit does not cut interpreter opcodes → no wall↓ vs capture tax.
5. **Capture-without-jump remains tax** (Iter19) — default SNAP stays OFF.

## Attack landed (production = Iter17 tip + Iter20 dig plumbing)

| Fix | Where |
|-----|--------|
| Allow read-prefix jump *with* memory (drop `!memory_lite_ok`) | `vm.rs` |
| Validated-safe FF origin seed; refuse jump on Estimate/unstable | `vm.rs` |
| **Hard-OFF abs jump arm** (JUMP env dig-only until hang fixed) | `vm.rs` |
| Hang-free Bind-snap **credit** consume (`bcredit`) when tip on resume | `vm.rs`, `metrics.rs` |
| `jump_refuse_reason` dig helper | `boundary.rs` |
| Lean production-off test | `tests/specfence.rs` |
| Keep Iter17 yield-spin + Iter16 absorb; SoftWait Soft~0; stock SSTORE | unchanged |

### Measured / rejected this iter

| Trial | Result |
|-------|--------|
| 20a: drop `!memory_lite_ok` + JUMP=1 | aj still 0 until Storage-FF; then **597 SF hang** — **falsified** |
| 20b: Validated-safe origin seed | did not prevent hang when jump armed — **falsified as hang fix** |
| 20c: credit consume SNAP=1, jump OFF | bcredit>0, Soft=0, no hang; wall tax from capture — **no wall↓** |
| Fan BO park / SoftWait Soft / mega-fan yield / SSTORE plant jump | **not re-tried** |

## Multi-block table

Machine load high (~32 on 8 cores). SoftWait Soft=**0** everywhere; aj=0;
bsnap=0 on production. Walls ≈ Iter19 load-noise band.

### Credit proof (`SPECFENCE_BIND_SNAP=1`, jump hard-OFF, N=1)
| Block | SF wall | Soft | bsnap | bcredit | aj |
|------:|--------:|-----:|------:|--------:|--:|
| **597** | 18.4 | **0** | **844** | **6** | 0 |
| **599** | 32.6 | **0** | 483 | **15** | 0 |
| **097** | 20.3 | **0** | 436 | **5** | 0 |
| **598** | 2.7 | **0** | 25 | 0 | 0 |

### Primary production N=5 (`abc-iter20` / SNAP OFF)
| Block | SF wall med | SoftWait Soft | aj | notes |
|------:|------------:|--------------:|---:|-------|
| **597** | **15.1** | **0** | 0 | load-noisy; ≈ Iter19 |
| **599** | **~36.6** (last) | **0** | 0 | load-noisy |
| **097** | **~15.4** (last) | **0** | 0 | ~ |
| **598** | **~2.7** (last) | **0** | 0 | quiet OK |

### Secondary N=10 (`abc-iter20-n10`)
| Block | SF wall med | SoftWait | aj |
|------:|------------:|---------:|--:|
| **597** | **17.3** | **0** | 0 |
| **599** | **~31.8** (last) | **0** | 0 |
| **097** | **~17.9** (last) | **0** | 0 |
| **598** | **~3.2** (last) | **0** | 0 |

Tests: `cargo test -p pevm --lib` **97 ok**; `--test specfence` **26 ok / 13 ignored**.

## Iter 20 diagnosis (required answers)

### 1. detect / avoid / resolve — which hole now?
**Resolve still primary for <10.** Bind tips are hang-free **creditable** (bcredit)
but credit ≠ opcode cut. Abs jump of Bind tips **falsified** (hangs when
`jump_is_safe` + Storage FF). Avoid (Iter17 yield-spin) plateau; SoftWait Soft=0.
Detect OK.

### 2. Region / Fence / intra / inter
| Axis | Verdict |
|------|---------|
| **Region** | ℓ / `a` OK; Bind-at-SLOAD tip correct grain; Basic-only tips lack Storage FF for large-bc jump gate. |
| **Fence** | SoftWait Soft dormant; abs-jump hard-OFF; BO park not widened. |
| **Intra** | Stock SSTORE; Bind-snap/credit opt-in; sticky-absorb OFF. |
| **Inter** | Quiet 598 OK; Storm 597 plateau under load. |

### 3. vs Iter19 / plateau
- Production ≈ Iter17 tip (SNAP/JUMP OFF). SoftWait Soft **0**; aj=0; no hang; no default tax beyond load noise.
- Bind tips **consumed hang-free via credit** (bcredit>0) — not via PC jump.
- Stretch <10 / wall <12.3 unmet under this load; quiet remeasure needed.
- SUCCESS path: **clear falsification of Bind abs jump + hang-free credit alternate**.

### 4. Cause for Iter 21 (named)
**Named cause:** Bind-snap abs jump livelocks under concurrency whenever
`jump_is_safe` admits a Storage-FF read-prefix tip; credit consume is hang-free
but does not cut interpreter-seconds. Opcode cut still blocked.

1. **Minimal single-tx repro of Storage-FF Bind jump hang** (seq≡par first at
   width=1, then width=2+) — isolate FF-origin seed / PC restore / warm gas —
   then aj>0∧seq≡par before any mainnet JUMP enable.
2. Or **non-jump opcode cut**: Handler fast-forward of certified-prefix SLOAD
   sequence using Bind tip as cursor (no full `apply_to_interp` under pevm MV).
3. Or **599-safe critical-path** removing SuffixRepair from 597 makespan without
   yield-deepening / BO park / sticky / ff_head / capture tax.
4. Do **not** default-on Bind-snap capture, re-enable SoftWait Soft, fan BO park,
   SSTORE plant jump, ForceBind/park ff_head, sticky-absorb, mega-fan yield, or
   arm Bind abs jump under concurrency until Lean+597 no-hang proven.

## Artifacts
- Prod: `lab/results/abc-iter20-sf-occ.json`, `abc-iter20-n10-sf-occ.json`
- Credit proof: `abc-iter20-bcredit-sf-occ.json` / `.run.log`
- Falsified JUMP: `abc-iter20-jump-fix*`, `abc-iter20-refuse-dig*` (hang)

## Code touched
- `specfence/boundary.rs` — `jump_refuse_reason`
- `specfence/metrics.rs` — `bind_snap_credit`
- `specfence/mod.rs` — Iter20 blurb; export refuse helper
- `vm.rs` — memory-gate fix; Validated-safe seed; jump arm hard-OFF; credit consume
- `chain/ethereum.rs` — unchanged (SNAP install still env-gated)
- `examples/specfence_g7_smoke.rs` — dig `bcredit`
- `tests/specfence.rs` — Iter20 production-off Lean test
