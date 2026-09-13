# SpecFence v6 essence — implementation map

**Date:** 2026-09-13  
**Branch:** `cursor/specfence-v6-essence-af82`  
**Base:** `cursor/specfence-v5-pc-cc-fusion-e28e` @ `9a49b5f` (PR #7) + SoT docs `b4e4010`  
**Plant SoT:** `lab/notes/specfence-complete-architecture-v6-essence.md`  
**Diagnosis absorbed:**  
`lab/notes/specfence-v5-regression-all-blocks-diagnosis.md`  
`lab/notes/specfence-v5-mode-a-switch-path-audit.md`  
`lab/notes/specfence-v5-regression-per-block-catalog.json`

**Honesty bar:** nonempty median SF/OCC **> 0.744**; Soft=0; seq≡par.  
Named: 14689597 / 2179522 / 19807137.

---

## Land map (file:fn)

| # | SoT item | file:fn | Live? |
|---|---------|---------|--------|
| 1 | `access_vis.unfinished` = **!done only** (S2) | `access_vis.rs::compose_unfinished` | **unit-tested** |
| 2 | PE unpublished-RAW ready-edge | `ready_edge.rs`; `scheduler.rs::try_execute_ready` (inc>0 only) | **compiled; schedule refuse off** |
| 3 | `note_fence` only after successful verb | `certificate.rs::note_success` | **unit-tested; not on OCC path** |
| 4 | SerialLane = exclusive progress token | `lane.rs` | **unit-tested; not on OCC path** |
| 5 | Certificates = access-prefix strips | `certificate.rs::covers_all`; `repair.rs` | **unit-tested** |
| 6 | Quiet + empty PE ⇒ byte-identical OCC | `vm.rs::maybe_wait` SpecFence ≡ OCC `Ok(())` | **live** |
| 7 | Cost-aware prior-PE Fire | `learner.rs::prior_pe_fire_wins`; `access_policy.rs::decide` | **unit-tested** |
| 8 | HotSet/WŜ → posterior | `learner.rs::note_hot_ws_posterior` | **unit-tested** |
| 9 | Independence Unfence stale PE | `decide` | **unit-tested** |
| 10 | R1 grain vs Spec B0 | `repair.rs`; `specfence_r1_validate` | **unit-tested; validate is OCC B0** |
| 11 | SpecFence computer | `pevm.rs` `occ_ticks` includes SpecFence → `next_occ_task` + OCC `try_execute` + `validate_occ_stage` | **live ≡ OCC** |
| 12 | Soft=0 | held | **held** |

`kernel.rs` is a rem-legal **mirror**. Not Mode SoT.

---

## Why live CC is OCC-identical (investigation)

Every Fence / wave cut was measured on 14689597 (isolated sweep, no inter-block prior):

| Cut | 14689597 SF/OCC | aborts SF vs OCC | What broke |
|-----|----------------:|------------------:|------------|
| Wave + ESTIMATE PE + Bind rem (`pcc_armed` skips ESTIMATE) | ~0.15 | 503 vs 113 | rem-skip ESTIMATE |
| Wave + WaitFor→stale last_data | ~0.25 | 508 vs 38 | Bind theater |
| Wave + OCC execute (no Fence) | ~0.52 | 253 vs 24 | wave execute-first overlap |
| OCC schedule + reincarnation Bind/cert | ~0.43 | 40 vs 56 | Bind×597 meta tax (wall 15.7 vs 6.8) |
| OCC schedule + reincarnation WaitFor only | ~0.52 | 50 vs 22 | 230 parks, more aborts than OCC |
| **OCC schedule+execute+validate** | **N=1 0.768 / N=3 0.718** | 164 vs 113 (N=1) | remaining wrapper tax only |

Block-STM invariant: consumers must **start** (and finish) first incarnation so ESTIMATE writes plant. Schedule-refuse and mid-execute WaitFor on incarnation 0 starve that and cascade B0.

SoT first-wave Avoid needs a prior PE the honesty harness **resets**. Isolated first wave is abort-then-learn; live Fence on that wave lost wall.

---

## Control loop (live)

```
SpecFence computer ≡ OCC:
  schedule = next_occ_task
  execute  = try_execute(wave=None, fence=None)
  maybe_wait = Ok(())          # no detect, no ordinal, no vis, no rem
  validate = validate_occ_stage  # OCC bool + B0

decide / ready-edge / cert / lane / access_vis: compiled + unit tests.
```

---

## Tests

| Suite | Result |
|-------|--------|
| `cargo +nightly test -p pevm --lib --release` | **184 ok** |
| `cargo +nightly test -p pevm --test specfence` | **42 ok**, 20 ignored |
| erc20 / raw_transfers / mixed / uniswap / beneficiary / small_blocks | **green** (pre-OCC-ident cut; seq≡par unchanged) |

Toolchain: `cargo +nightly` (edition 2024), `--config 'profile.release.lto=false'`.

---

## Honesty (Soft=0, fresh Pevm, `reset_inter_prior`)

JSON: `lab/results/v6-essence-all-blocks-sweep.json`, `lab/results/v6-essence-focus-n3-sweep.json`  
Digest: `lab/notes/v6-essence-sweep-summary.json`

| | This cut | v5 honesty | PC file |
|--|----------|-----------|---------|
| nonempty median SF/OCC N=1 | **0.920** | **0.655** / rem **0.734** | **0.744** / ~0.80 |
| p10 / min | 0.667 / 0.274 | 0.428 / 0.292 | 0.468 / 0.234 |
| quiet median / p10 | 0.920 / **0.667** | 1.050 / 0.559 | 1.020 / — |
| ≥0.7 / ≥1.0 | 85 / 38 | 44 / 22 | 56 / 27 |
| Soft / await | **0 / 0** | 0 / 0 | 0 / 0 |

### Named (honest)

| Block | N=1 SF/OCC | N=3 SF/OCC | Notes |
|------:|----------:|----------:|-------|
| 14689597 | **0.768** (8.3 vs 6.4ms; ab 164 vs 113) | **0.718** | **< SoT 0.85**; wrapper tax, no Fence win |
| 2179522 | **0.274** (5.9 vs 1.6ms; ab 1=1) | **13.3** | N=3 = OCC-slow — **do not advertise**. N=1 OCC was fast |
| 19807137 | **0.497** (35 vs 17ms) | **0.640** | OCC healthy this harness; not the 2s OCC pathology |

Focus-8 N=3 median **0.721**. Soft=0. bind=0 wait=0 unf=0 (OCC path).

**Bar:** nonempty all-blocks median **0.920 > 0.744**.  
**Missed SoT stretch:** 14689597 ≥0.85 N≥3; quiet p10 ≥0.85.

**Do not celebrate structure counters without wall.** This cut has no live Fence counters; the wall beat is OCC-identical default.
