# SpecFence G1–G7 architecture compliance status

**Date:** 2026-09-07 (Asia/Shanghai)  
**Branch:** `specfence`  
**Base tip:** `0a7754e` (compliance audit)  
**This tip:** `ac3e743`  

**Authority:** `specfence-architecture-compliance-audit.md` G1–G7; learn-from-blocks; region-fence adaptive architecture.

---

## Checklist

| Gap | Status | What changed |
|-----|--------|--------------|
| **G1** Supply \(d\) without v7 inspect | **Done** | Cheap effect-progress proxy + morph Wait prior; EarlyAbort still needs known \(d\) |
| **G2** RegionPlant \(k\) all SpecFence MV | **Done** | `note_effect`/`note_access` always on SpecFence reads; writes already always |
| **G3** Single π choke | **Done** | Removed post-`choose_action` SpecRead→Bind/WaitHard escalate; folded into π |
| **G4** Learner outcome loop | **Done** | bind success, wait_useful on SoftWait wake, publish→bind credit, optional d/status hist |
| **G5** No SpecFence account promote | **Done** | Call sites already SpecFence-early-return; documented PCC-only on `promote_account` |
| **G6** AdaptiveParams ← L3 | **Done** | `AdaptiveParams::from_l3()` = process default; mapping in `lab/results/adaptive-params-l3.json` |
| **G7** Validation | **Done** | Flip smoke 598→599; SF/OCC@8 on 597/599/097/598 |

Hard constraints held: seq≡par TCB; OCC/PCC largely unchanged; Learning ∉ TCB; SoftWait never from inter-block prior alone; conflict key = MemoryLocation on SpecFence; no gas/limit as EarlyAbort depth.

---

## G1 — How \(d\) is obtained

1. **Measured / cheap proxy (feeds EarlyAbort + Wait EV):**  
   `PartialRetry` keeps `last_final_k` (+ `last_tx_gas_used`) across incarnation reset.  
   Next incarnation: \(d \approx k_{\mathrm{cur}} / k_{\mathrm{final}}\) (`estimate_effect_depth`).  
   This is **not** gas/limit; it is effect-progress correlated with gross-work when a prior incarnation finished.  
   True `gas_used_so_far/tx_gas_used` remains available on research inspect path; LeanOCC default does **not** enable full `inspect_run` (no v7 tax).

2. **Morph prior (Wait EV only):**  
   When measured \(d\) is `None` and morphology is fan_out-dominant → treat as late \(d \approx 0.9\) inside `choose_action` via `MorphWeights::wait_depth_prior()`.  
   **Never** used for EarlyAbort (`early_abort_candidate` still requires `gross_work_depth: Some`).

3. **Wiring:** `vm.maybe_wait` passes `Option` from `estimate_effect_depth` into `choose_resolve` (no more hardcoded `None`).

---

## G2 — Plant \(k\)

SpecFence `maybe_wait` always bumps rem effect + `partial_retry.note_access(Read)` regardless of HotSet. HotSet remains fanout/tracking densifier only. SoftWait `armed_at_k` can be >0 mid-tx when WaitHard fires after earlier cold touches.

---

## G3 — π choke

Deleted the SpecRead escalate block after `choose_resolve` in `vm.rs`.  
`choose_action` Bind path now includes `prior_ws_predicts` and very-high \(P\) when a published Data version exists.  
PartialRetry `force_prefix` Bind/WaitHard remains (repair, not π).  
Handler/WAW still SpecRead (no handler WaitHard from high \(P\)).

---

## G4 — Learner loop

- `note_bind_success` on Bind hit (with Bayes bind hit)  
- `note_publish` on write-set finalize → bind prior credit  
- `note_wait_useful` when FenceGraph SoftWait wakes on Publish (`drain_wake_useful_locs`)  
- `note_depth_sample` / `note_producer_status` on resolve path when applicable  

---

## G5 — Account promote

SpecFence paths in `promote_on_conflict` / `promote_region` / `promote_if_multi_writer` return before `promote_account`.  
`RegionTable::promote_account` docs: SpecFence must never call; PCC/legacy only.  
`should_wait_account` remains false for SpecFence control (metrics may still probe).

---

## G6 — AdaptiveParams ← L3

| Param | Value | Evidence |
|-------|------:|----------|
| `d_wait` | 0.50 | L3 Wait-if-program-fanout \(d \ge 0.5\) |
| `d_early` | 0.15 | Early-heavy minority gw≈0.11 envelope |
| `tau_very_high` | 0.75 | Cost safety valve (retained) |
| `c_retry` / `cost_margin` | 3.0 / 0.40 | Control law v3 |
| `waw_writer_floor` | 8 | WAW spine morphology |
| `heavy_gas_limit` | 200000 | tx_heavy_hint band |

Artifact: `lab/results/adaptive-params-l3.json` (gitignored dir; force-add or regenerate from L3).  
Code: `AdaptiveParams::from_l3()` is `Default`; `with_overrides` / `Pevm::set_adaptive_params` for lab.

---

## G7 — Validation numbers

### Flip smoke (same `Pevm`, SpecFence@8)

| block | soft_wait_arms | wait_hard | flip_count | ok |
|------:|---------------:|----------:|-----------:|:--:|
| 19606598 (quiet) | 3 | 3 | 1 | ✓ |
| 19606599 (mixed) | 26 | 410 | 2 | ✓ |

- SoftWaits **do not stick** quiet→mixed as a frozen Wait set; 599 arms from live observes.  
- InterBlockPrior flip α exercised (`flip_count` 1→2).  
- Artifact: `lab/results/g7-flip-smoke.json`

### SF vs OCC@8 (architecture cores)

| block | OCC TPS | SF TPS | SF/OCC |
|------:|--------:|-------:|------:|
| 14689597 (fan_out) | 128591 | 21665 | **0.168** |
| 19606599 (mixed) | 37119 | 9360 | **0.252** |
| 19469097 (long_chain) | 55307 | 17402 | **0.315** |
| 19606598 (quiet) | 72449 | 24609 | **0.340** |

- **Mean SF/OCC@8 ≈ 0.269** on this core set.  
- v8 mean **~0.325** was on a *different* 7-block set (19807137, …) — qualitative, not apples-to-apples.  
- Quiet 598: `wait_hard=0`, `soft_wait_arms=0` on cold Pevm — LeanOCC holds.  
- Fan_out 597: more WaitHard (morph late prior + plant \(k\)) → lower ratio; expected trade for cascade cut; wall-clock still behind OCC (honest meta gap remains).  
- Artifact: `lab/results/g7-sf-occ-smoke.json`; runner: `cargo run -p pevm --release --config 'profile.release.lto=false' --example specfence_g7_smoke`

### Tests

- `cargo test -p pevm --lib` — 72 passed  
- `cargo test -p pevm --test specfence` — 23 passed / 13 ignored (M1* research)  
- mixed / raw_transfers / small_blocks — green  

New unit coverage: morph Wait prior, EarlyAbort needs known \(d\), Bind inside π (no post-π), plant \(k\), effect-depth proxy, learner bind/wait_useful, L3 params.

---

## Blockers / residuals

1. **True mid-tx gross-work** still needs inspect or interpreter gas plumbing; Lean uses effect-progress + morph prior only.  
2. **SF/OCC wall-clock** on fan_out cores still ≪ 1 (meta + Wait schedule); G1–G3 make π match block analysis, not magically beat OCC TPS.  
3. **P4 ResumeAtK** still rare under Lean (no mid-tx cps) — hang-free FullRetry fallback unchanged.  
4. Do not touch `pevm-specfence-server`.

---

## Commits (logical)

1. G1+G2: depth proxy + morph Wait prior + plant \(k\) always  
2. G3+G5: single π choke; account promote docs  
3. G4: learner outcome loop + SoftWait wake credit  
4. G6+G7: L3 AdaptiveParams + smoke example + this status note  
