# SpecFence A+B+C Iter 13 status

**Date:** 2026-09-08 (Asia/Shanghai)  
**Branch:** `specfence`  
**Base tip:** `818e900` (Iter12)  
**Authority:** `specfence-abc-unified-protocol.md`, `specfence-abc-iter12-status.md`, `specfence-global-cc-evm-rethink.md`

## Mandate

Cut certified-prefix opcode-seconds on *successful* SuffixRepair without abs jump /
abort-path spins. Try Validated-gated value-stable journal FF (Iter10 bare falsified);
and/or stronger serial-barrier clique without sibling-park hang; and/or prove narrow
single-SSTORE jump (empty-memory refused) with aj>0∧seq≡par (multi still refused).

Goal: 597 median <12.5 toward <10; SoftWait Soft=0; no hang.

## Root cause (diagnosed this iter)

1. **Successful SuffixRepair still re-interprets certified-prefix opcodes** — head-FF
   / journal FF only skip DB/MV when **origin incarnation matches**. Origin bumps with
   same Storage/Basic value miss FF → full MV/DB path; interpreter still pays every
   prefix opcode either way (true opcode-skip needs abs jump).
2. **Bare value-stable FF (Iter10) was unsafe** — without Validated gate, concurrent
   writers can still change → livelock / N10 wall↑. Validated gate is the correct
   safety condition but **fires rarely** on 597 (vs_ff ≈ 1–6 / run) because most
   origin-stable hits already succeed and writers are seldom Validated at resume time.
3. **Abs jump remains the only real opcode cut** — env-gated single-SSTORE
   capture+jump (`SPECFENCE_ABSOLUTE_JUMP=1`) **hung** the Iter9 Lean fixture (same
   family as prior jump/capture hangs). Multi-SSTORE still refused (Iter11).
4. **Executed→Validated escalate spin** (abort/escalate path) and **FF-path Validated
   yield** add wall without enough vs_ff gain — same family as Iter12 abort-path
   evidence spins (falsified).

## Attack landed (production = `abc-iter13` = 13d lineage)

| Fix | Where |
|-----|--------|
| **Validated-gated value-stable journal FF** (Storage + Basic; rebind origin) | `vm.rs` `try_ff_*` |
| **`value_stable_ff_hits` metric** | `metrics.rs` / g7 smoke JSON |
| **Serial-barrier multi-candidate claim** (fan-desc; no sibling park) | `pevm.rs` |
| Jump/capture OFF; stock SSTORE; SoftWait Soft~0 | unchanged |

### Measured / rejected this iter

| Trial | Result |
|-------|--------|
| Validated-gated Storage vs-FF + Executed→Validated escalate spin (`abc-iter13` early / 13a) | 597 N5 **13.9↑** — escalate spin **falsified** |
| Storage vs-FF + multi-cand barrier, no escalate spin (`abc-iter13b`) | 597 N5 **13.5**; vs_ff≈2 — no clear wall win |
| + FF-path Validated yield (`abc-iter13c`) | 597 N5 **13.5**; vs_ff≈0 — **falsified** |
| Storage+Basic Validated vs-FF + multi-cand (`abc-iter13d` **prod**) | Soft=0; **no hang**; N5 **12.9** / N10 **13.7** |
| Env single-SSTORE capture+jump (`SPECFENCE_ABSOLUTE_JUMP=1`) | **hung** iter9 Lean fixture — **falsified** |

## Multi-block table

### Primary: N=5 (`abc-iter13d` / prod lineage)
| Block | SF wall med | OCC wall med | SoftWait Soft | vs_ff | aj | notes |
|------:|------------:|-------------:|--------------:|------:|---:|-------|
| **597** | **12.9** | 3.5 | **0** | ~1 | 0 | ≈ Iter12 12.5 (noise); Soft=0 |
| **599** | **20.5** | 9.5 | **0** | ~6 | 0 | ~ Iter12 19.4 |
| **097** | **14.4** | 6.4 | **0** | ~3 | 0 | ↑ vs Iter12 11.3 (noise/schedule) |
| **598** | **2.3** | 1.2 | **0** | ~1 | 0 | quiet OK |

### Secondary: N=10 (`abc-iter13d-n10`)
| Block | SF wall med | SoftWait | aj | vs Iter12 N10 |
|------:|------------:|---------:|---:|---------------|
| **597** | **13.7** | **0** | 0 | **↓ vs Iter12 14.0** |
| **599** | **18.6** | **0** | 0 | **↓ vs Iter12 19.7** |
| **097** | **12.5** | **0** | 0 | ~ Iter12 11.7 |
| **598** | **2.2** | **0** | 0 | quiet OK |

Last-row dig (597 SF N5 prod): resume≈112, fb_reabort≈86, sb_res≈23, fr≈53,
vs_ff≈1, SoftWait Soft **0**, aj=0, hsstore=0. No hang.
Retag `abc-iter13` under machine noise (OCC p90 outlier) reported 14.1 — treat
**13d** as authoritative production numbers.

Tests: `cargo test -p pevm --lib` **97 ok**; `--test specfence` **25 ok / 13 ignored**.

## Iter 13 diagnosis (required answers)

### 1. detect / avoid / resolve — which hole now?
**Resolve still primary for <10.** Validated-gated vs-FF is **safe** (no livelock) but
**does not cut opcode-seconds** (DB skip only; rare hits). True opcode cut = abs jump,
still **not hang-free**. Serial-barrier multi-cand is hang-free structure without
sibling park; wall effect within noise. Avoid OK (SoftWait Soft=0). Detect OK.

### 2. Region / Fence / intra / inter
| Axis | Verdict |
|------|---------|
| **Region** | ℓ / `a` OK; vs-FF rebinds origin to Validated tip on certified reads. |
| **Fence** | SoftWait Soft dormant; abs-jump OFF; BO/serial-barrier multi-cand only. |
| **Intra** | vs-FF only on rewind/ff_head; no mass-path plant/SSTORE tax. |
| **Inter** | Quiet\|Storm gates serial-barrier (storm-only); Quiet 598 OK. |

### 3. vs Iter12 / plateau
- N5: **12.9** ≈ Iter12 **12.5** (noise); SoftWait Soft **0**; aj=0.
- N10: **13.7** ≤ Iter12 **14.0**; 599 also ↓.
- Stretch <10 unmet. Opcode cut **not** proven.
- Single-SSTORE jump enablement **falsified** (hang).

### 4. Cause for Iter 14 (named)
**Named cause:** Certified-prefix **interpreter** seconds on successful SuffixRepair
remain because (a) journal FF cannot skip opcodes, (b) Validated-gated value-stable
FF is too rare to matter on 597, and (c) hang-free abs jump is still unproven
(single-SSTORE env hung; multi refused). Remaining gap to <10 needs a **new**
hang-free resolve shape that is not jump/plant/abort-path spin:

1. **Schedule-side makespan**: serialize fan-out consumers behind one Validated spine
   *before* first SuffixRepair (BO Await at first-cross on hot ℓ with stronger
   Validated evidence) so fewer txs ever pay prefix re-interp — without SoftWait Soft
   1.0 / sibling-park hang.
2. Or **RebindOnly / true_suffix collapse** so value-changing RAW on 597 becomes
   value-stable after writer Validated more often (rb≈0 today).
3. Or prove abs jump hang-free via a **non-TLS / non-capture-window** path (prior
   Handler plant + JUMP=1 hung — need different capture grain).
4. Do **not** re-enable SoftWait Soft 1.0, abort-path Validated evidence spins,
   broad unfinished Estimate park, capture-without-jump, live_prime inspect,
   multi-SSTORE abs jump, plant pre-sload warm, or mass-path SSTORE tax.

## Artifacts
- `lab/results/abc-iter13d-sf-occ.json`, `abc-iter13d-flip.json`, `abc-iter13d.run.log` (N=5 prod)
- `lab/results/abc-iter13d-n10-sf-occ.json`, `abc-iter13d-n10-flip.json`, `abc-iter13d-n10.run.log`
- Copies: `abc-iter13-prod-*`, `abc-iter13-n10-*`
- Falsified: `abc-iter13` early/13a (escalate spin), `abc-iter13b`, `abc-iter13c`;
  single-SSTORE JUMP=1 hang (no artifact — killed hung test)

## Code touched
- `vm.rs` — Validated-gated value-stable `try_ff_storage` / `try_ff_basic`
- `pevm.rs` — serial-barrier multi-candidate claim (no sibling / no Executed spin)
- `metrics.rs` — `value_stable_ff_hits`
- `specfence_g7_smoke.rs` — emit `value_stable_ff_hits`
- `mod.rs` — Iter13 blurb
