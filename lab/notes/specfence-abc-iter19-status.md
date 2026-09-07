# SpecFence A+B+C Iter 19 status

**Date:** 2026-09-08 (Asia/Shanghai)  
**Branch:** `specfence`  
**Base tip:** `d942d98` (Iter18)  
**Authority:** `specfence-abc-unified-protocol.md`, `specfence-abc-iter18-status.md`, `specfence-global-cc-evm-rethink.md`

## Mandate

Hang-free live snap at **certified-prefix end** (Bind/EffectBoundary before
`k_fail`), not post-SSTORE plant — so jump/resume can skip prefix opcodes. Or
599-safe critical-path schedule if snap path falsified. Goal: 597 median **<12.3**
toward <10; SoftWait Soft=0; 599 not regress; no hang; seq≡par. Keep Iter17
yield-spin + Iter16 absorb; stock SSTORE; no fan BO park; no FF widen.

## Root cause (diagnosed this iter)

1. **Capture grain was wrong for RAW-read fails (Iter18 confirmed)** — Handler
   SSTORE tips land at `k ≥ k_fail`. Need Bind/SLOAD-grain tips at certify time.
2. **Iter19 Bind/SLOAD snap plumbing works** — with `SPECFENCE_BIND_SNAP=1`,
   597 records **bsnap≈850** (live tips at Bind-on-Data after stock SLOAD). No
   plant TLS / WaitHard demote / SSTORE wrap. Stock SSTORE preserved.
3. **Absolute jump on Bind-snap hung** — `SPECFENCE_BIND_SNAP_JUMP=1` livelocked
   `specfence_iter9_handler_single_sstore_jump_seq_eq_par` (same family as Iter13
   JUMP=1 / inspect hangs). Production jump stays **OFF**.
4. **Capture-without-jump is pure wall tax** — default-on capture raised 597/599
   walls with aj=0. Default capture **OFF**.
5. **Mega-fan yield-spin (128/64 at fan≥16) falsified under load** — yield tax↑
   when CPUs saturated; reverted to Iter17 64+32.

## Attack landed (production = Iter17 tip + opt-in Bind-snap)

| Fix | Where |
|-----|--------|
| Handler SLOAD Bind-snap wrap + `with_bind_snap_tls` (no IN_INSPECT) | `boundary.rs`, `ethereum.rs` |
| `note_pending_bind_snap` from Bind-on-Data lite | `vm.rs` |
| Prefer Bind/read-boundary snaps in `build_continuation` (k < k_fail) | `rem.rs` |
| Read-prefix jump arm path (env-gated; production OFF) | `vm.rs` |
| Keep Iter17 yield-spin + Iter16 absorb; SoftWait Soft~0; stock SSTORE | unchanged |

### Measured / rejected this iter

| Trial | Result |
|-------|--------|
| 19a: Bind-snap capture default-on, jump OFF | bsnap↑ but 597/599 wall↑ — **falsified as production default** |
| 19b: `SPECFENCE_BIND_SNAP_JUMP=1` | hung Lean iter9 fixture — **falsified** |
| 19c: mega-fan yield 128/64 | wall↑ under load — **falsified** |
| Fan BO park / SoftWait Soft / SSTORE plant jump / FF widen | **not re-tried** |

## Multi-block table

Machine load was high (~25–28 on 8 cores). SoftWait Soft=**0** everywhere; aj=0;
bsnap=0 on production. Walls sit above quiet Iter18 band (noise).

### Capture proof (`SPECFENCE_BIND_SNAP=1`, N=1)
| Block | SF wall | Soft | bsnap | aj |
|------:|--------:|-----:|------:|--:|
| **597** | 14.5 | **0** | **852** | 0 |
| **599** | 32.2 | **0** | 499 | 0 |
| **598** | 3.1 | **0** | 17 | 0 |

### Primary production N=5 (`abc-iter19` / snap OFF)
| Block | SF wall med | SoftWait Soft | aj | notes |
|------:|------------:|--------------:|---:|-------|
| **597** | **14.9** | **0** | 0 | load-noisy; ≈ Iter17 family |
| **599** | **27.8** | **0** | 0 | load-noisy |
| **097** | **20.5** | **0** | 0 | ~ |
| **598** | **3.4** | **0** | 0 | quiet OK |

### Secondary N=10 (`abc-iter19-n10`)
| Block | SF wall med | SoftWait | aj |
|------:|------------:|---------:|--:|
| **597** | **16.3** | **0** | 0 |
| **599** | **27.0** | **0** | 0 |
| **097** | **18.2** | **0** | 0 |
| **598** | **2.5** | **0** | 0 |

Tests: `cargo test -p pevm --lib` **97 ok**; `--test specfence` **25 ok / 13 ignored**.

## Iter 19 diagnosis (required answers)

### 1. detect / avoid / resolve — which hole now?
**Resolve still primary for <10.** Bind-snap proves the right capture grain for
RAW-read fails (bsnap>0 at Bind/SLOAD, not post-SSTORE), but **hang-free absolute
jump remains unproven** (env jump hung). Avoid (Iter17 yield-spin) at plateau;
deeper yield falsified. SoftWait Soft=0. Detect OK.

### 2. Region / Fence / intra / inter
| Axis | Verdict |
|------|---------|
| **Region** | ℓ / `a` OK; Bind-at-SLOAD is the correct `a` for certified-prefix-end snap. |
| **Fence** | SoftWait Soft dormant; abs-jump OFF; BO park not widened. |
| **Intra** | Stock SSTORE; Bind-snap opt-in only; sticky-absorb OFF. |
| **Inter** | Quiet 598 OK; Storm 597 plateau under load. |

### 3. vs Iter18 / plateau
- Production runtime ≈ Iter17 tip (snap/jump OFF). SoftWait Soft **0**; aj=0; no hang.
- Prefix-end snap **proven capturable** (bsnap≈850) but **not usable** for jump yet.
- Stretch <10 / wall <12.3 unmet under this load; quiet remeasure needed.

### 4. Cause for Iter 20 (named)
**Named cause:** Certified-prefix-end Bind snaps exist, but applying them as
absolute PC resume still livelocks under concurrency — same inspect/jump family.
Opcode cut needs a **non-jump** consume of Bind tips, or a hang-free jump restore
that is seq≡par on Lean fixtures before mainnet enablement.

1. **Hang-free consume of Bind-snap without full abs jump** — e.g. gas/stack-only
   short-circuit, or CallOutcome/read-boundary resume that does not rewrite PC
   under pevm MV (prove seq≡par on iter9/iter11 fixtures first).
2. Or **fix why Bind-snap jump hangs** (depth/CALL_DEPTH? FF origin seed?
   circuit-breaker?) with a minimal single-tx repro — then aj>0∧seq≡par.
3. Or **599-safe critical-path** that removes SuffixRepair from 597 makespan
   without yield-deepening / BO park / sticky / ff_head (mega-fan yield falsified).
4. Do **not** default-on Bind-snap capture without jump, re-enable SoftWait Soft,
   fan BO park, SSTORE plant jump, ForceBind/park ff_head, sticky-absorb, or
   mega-fan yield under contention.

## Artifacts
- Prod: `lab/results/abc-iter19-sf-occ.json`, `abc-iter19-n10-sf-occ.json`
- Capture proof: `abc-iter19-bsnap-sf-occ.json` / `.run.log`
- N5×5 (mega-fan trial, reverted): `abc-iter19-r1`…`r5`

## Code touched
- `specfence/boundary.rs` — Bind-snap TLS, SLOAD wrap, env gates
- `specfence/rem.rs` — prefer Bind/read-boundary snaps for k < k_fail
- `specfence/metrics.rs` — `bind_snap_capture`
- `vm.rs` — note_pending_bind_snap; jump arm path (env OFF); Iter17 yield kept
- `chain/ethereum.rs` — install SLOAD Bind-snap when env on
- `examples/specfence_g7_smoke.rs` — dig `bsnap`
- `specfence/mod.rs` — Iter19 blurb
