# SpecFence A+B+C Iter 29 status

**Date:** 2026-09-08 (Asia/Shanghai)  
**Branch:** `specfence`  
**Base tip:** `3196f9c` (Iter28)  
**Authority:** `specfence-abc-unified-protocol.md`, `specfence-abc-iter28-status.md`, `specfence-global-cc-evm-rethink.md`

## Mandate

Hang-free nested Bind consume ≠ frame_init defer, **or** schedule ≤12.1→<10.
SoftWait Soft=0; no nested apply hang (Iter27/28); no first-frame-only wall↑;
no fra≥16 / yield deepen / SoftWait Soft / BO park / mass SNAP / Validated skip.
Keep tip≡FF + steps_cap + LAST_SNAP clear.

## Root cause (diagnosed this iter)

1. **Iter28e hang vector = PENDING kept across frame_init** — `apply_defer`
   spam on every nested init while PENDING stayed armed. That is *not* the same
   as applying once after a natural CALL hash match with PENDING cleared.
2. **Lean Handler only tried apply after first frame_init** — nested tips need a
   post-`frame_init` hook in `run_exec_loop` (and Inspector init) to consume.
3. **Default-on nested abs apply → Lean seq≠par under concurrency** — dig mainnet
   hang-free with `SPECFENCE_NESTED_BIND=1` (`nested_stash→nested_match→apply_ok`),
   but Lean fixtures flaked seq≠par when default-on. Production stays apply OFF.
4. **597 aj still blocked at arm gates** — `origin_unsafe` / Validated prefix /
   `steps_over` (not code_hash alone). Nested apply dig still shows **aj=0 on 597**.
5. **Richer tip≡FF select (invert tip_compact) wall↑** — 597 N5 **17.4** — falsified;
   keep Iter27 prefer-fewer tip_compact.

## Attack landed (production = nested credit + opt-in nested apply)

| Fix | Where |
|-----|--------|
| Hang-free nested stash (PENDING cleared on code_hash mismatch) | `boundary.rs` try_apply |
| Natural CALL consume after nested `frame_init` (budget 32) | `tx_runner.rs` + Inspector init |
| Depth/jdepth bypass only under `NESTED_ALLOW_JDEPTH` | `boundary.rs` |
| Production: **credit** nested Bind tips on code_hash refuse | `boundary.rs` try_apply |
| Opt-in nested abs apply: `SPECFENCE_NESTED_BIND=1` (default OFF) | `nested_bind_consume_enabled` |
| Keep tip≡FF overlap + steps_cap + LAST_SNAP clear; SoftWait Soft~0 | unchanged |
| Iter29 Lean test | `tests/specfence.rs` |

### Measured / rejected this iter

| Trial | Result |
|-------|--------|
| Nested apply default-on (29a) | dig hang-free `apply_ok`; Lean **seq≠par flake** — **falsified for default** |
| Richer tip≡FF tip_compact (29a) | 597 N5 **17.4↑** — **falsified** |
| Nested apply OFF + tip_compact Iter27 (29b) | 597 N5 **13.9**; Soft=0 |
| Production credit-on-refuse + opt-in apply (29) | SoftWait Soft=0; 599/097 aj>0; tests green |

## Multi-block table

SoftWait Soft=**0**. Wall noise-matched to Iter28 plateau (≤12.1 unmet this load).

### Primary production N=5 (`abc-iter29`)
| Block | SF wall med | OCC med | SoftWait Soft | aj | bcredit | notes |
|------:|------------:|--------:|--------------:|--:|--------:|-------|
| **597** | **14.7** | 3.6 | **0** | 0 | ~29 | min 12.2; credit fires |
| **599** | **21.7** | 9.1 | **0** | **6** | ~2 | 599-safe; aj>0 |
| **097** | **13.5** | 5.4 | **0** | **1** | ~1 | safe |
| **598** | **2.7** | 1.2 | **0** | 0 | ~0 | quiet OK |

### Secondary N=10 (`abc-iter29-n10`)
| Block | SF wall med | SoftWait | aj | notes |
|------:|------------:|---------:|--:|-------|
| **597** | **13.9** | **0** | 0 | min 13.2; plateau |
| **599** | **24.5** | **0** | **7** | safe |
| **097** | **13.2** | **0** | 0–2 | |
| **598** | **2.3** | **0** | 0 | |

### Dig: hang-free nested apply (`SPECFENCE_NESTED_BIND=1`)
`abc-iter29-nested-dig`: `nested_stash→nested_match→apply_depth_bypass→apply_ok`;
EXIT=0; SoftWait Soft=0; **no hang** (≠ Iter28e PENDING defer).

Tests: `cargo test -p pevm --lib` + `--test specfence` (serial) green (3×).

## Iter 29 diagnosis (required answers)

### 1. detect / avoid / resolve — which hole now?
**Resolve still primary for <10 / 597 aj.** Detect OK. Avoid: SoftWait Soft=0.
Named hole: **597 tip arms blocked by Validated/origin_unsafe/steps_over**; nested
abs apply is hang-free when opt-in but **not Lean-safe default**; production
credits nested tips on code_hash refuse.

### 2. Region / Fence / intra / inter
| Axis | Verdict |
|------|---------|
| **Region** | tip≡FF + steps_cap kept; nested tip identity via code_hash |
| **Fence** | SoftWait Soft dormant; nested apply opt-in only |
| **Intra** | LAST_SNAP clear; ResumePath; credit-on-refuse nested tips |
| **Inter** | Quiet 598 OK; Storm 597 plateau; 599 aj specializes |

### 3. vs Iter28 / plateau
- Production: tip≡FF + steps_cap + LAST_SNAP + **nested credit** + opt-in nested apply.
- SoftWait Soft **0**. Hang-free nested consume **proven** (`NESTED_BIND=1`).
- SUCCESS: **SoftWait Soft=0** + **hang-free nested consume (opt-in dig)** +
  **diagnosis complete** (≤12.1 / 597 aj unmet on this load).

### 4. Cause for Iter 30 (named)
1. **Make nested abs apply Lean seq≡par** (narrow gates: tip_sloads addr ≡
   `target_address`, depth≤2, tip≡FF only) so `SPECFENCE_NESTED_BIND` can default
   on — **not** PENDING frame_init defer (hung Iter28e); or
2. **597 arm gates** — reduce origin_unsafe / steps_over without Validated skip /
   fra≥16 / yield deepen / SoftWait Soft / BO park / mass SNAP; or
3. **Schedule ≤12.1→<10** on quieter samples — same bans.
4. Keep silent-default ResumePath; FF-only tip≡FF; SoftWait Soft~0; LAST_SNAP clear.

## Artifacts
- Prod: `lab/results/abc-iter29-sf-occ.json`, `abc-iter29-n10-sf-occ.json`
- Flip: `abc-iter29-flip.json`, `abc-iter29-n10-flip.json`
- Logs: `abc-iter29.run.log`, `abc-iter29-n10.run.log`
- Dig/falsified: `abc-iter29a-dig.*`, `abc-iter29b.*`, `abc-iter29-nested-dig.*`

## Code touched
- `specfence/boundary.rs` — nested stash/consume; credit-on-refuse; depth bypass
- `tx_runner.rs` — post-nested-frame_init consume hook
- `specfence/mod.rs` — Iter29 blurb + exports
- `tests/specfence.rs` — Iter29 Lean
- `lab/notes/specfence-abc-unified-protocol.md` — Iter29 log
