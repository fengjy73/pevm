# SpecFence methodology pipeline status

**Date:** 2026-09-07 (Asia/Shanghai)  
**Branch:** `specfence`  
**Authority:** `lab/notes/specfence-historical-block-instrumentation-methodology.md`

## Deliverables

| Item | Path / status |
|------|----------------|
| Method schema note | `lab/notes/specfence-measurement-method-schema.md` |
| Frozen `MeasurementMethod` in code | `crates/pevm/src/specfence/finegrain.rs` (`schema_version=l1l2-v1`, `warm_policy=emit_warm_true`) |
| L1 effect log + DAG summary | `FineGrainSnapshot.effect_log`, `l1_dag_summary()` |
| L2 unified OCC traces | `producer_status` / `ready_for_bind` / `warm` / `call_depth` on `RawEffectEdge` |
| L1/L2 collector | `cargo run -p pevm --release --config 'profile.release.lto=false' --example specfence_l1_l2_collect` |
| L3 offline EV lab | `lab/scripts/l3_offline_ev_lab.py` → `lab/results/l3-offline-ev.{json,md}` |
| L4 prior predictivity | `lab/scripts/l4_prior_predictivity.py` → `lab/results/l4-prior-predictivity.{json,md}` |
| Status (this file) | `lab/notes/specfence-methodology-pipeline-status.md` |

## F. Collection run

Priority blocks (journal OCC@1 + OCC@8), all OK:

| Block | Role | L1 RAW | L1 effect_log | OCC@8 RAW | morph (L1) |
|------:|------|-------:|--------------:|----------:|------------|
| 14689597 | core A | 647 | 3600 | 1987 | fan_out |
| 19606599 | core B | 584 | 7955 | ~1183–1550 | long_chain* |
| 19469097 | core C | 745† / 410 deeper | 4805 | 1187 | long_chain |
| 19606598 | quiet B | 44 | 1167 | 52 | quiet |
| 19469096 | quiet/spine C | 234 | 2128 | 586 | long_chain* |

\*Heuristic morphology label; plant notes still call 599 mixed / 096 waw_spine.  
†Instance count can exceed prior deeper summary depending on warm/instance emission; effective filtered RAW on OCC@1 matches prior ~410–647 band on cores.

Artifacts: `lab/results/l1l2-b{N}.json`, `lab/results/l1l2-summary.json`, log `lab/results/l1l2-collect.run.log`.  
Gap: segment B tip `19606600` not on disk (not required for priority list). No Alchemy re-fetch needed for listed cores/neighbors.

## L3 EV headline (wasteΔ vs M-A; negative = better)

| Morphology | Wait-if-program-fanout | Bind-if-ready | M-D (oracle-ish) |
|------------|-----------------------:|--------------:|-----------------:|
| fan_out (597) | **-382** | **-413** | -319 |
| long_chain (097) | **-88** | **-127** | -82 |
| mixed (599) | 0 | **-80** | -31 |
| waw_spine (096) | 0 | **-141** | -130 |
| quiet (598/599q) | 0 | **-6** | -3 |

**Gate:** Bind-if-ready wins on ≥1 hot morphology and does **not** degrade quiet. Wait-if-program-fanout wins on fan_out + long_chain, neutral on quiet.  
→ **`choose_action` AUTHORIZED and implemented (v3).**

## L4 prior predictivity

- Contiguous A/B/C feature vectors from `contiguous-segments-finegrain.json` (+ deeper overwrite when present).
- **Overall: NO** — mean L2(t−1→t) **236** vs global prior **162**; `tm1_better_frac≈0.44`.
- Conclusion: prefer **intra-block EV** over sticky inter-block priors (matches deeper-pass stance).

## G. choose_action decision — IMPLEMENTED

Wired control law v3 in `crates/pevm/src/specfence/resolve.rs` + `vm.rs::maybe_wait`:

1. **Bind** if producer Data / published version ready.  
2. **WaitHard** if program ∧ writer known ∧ not done ∧ (`fanout_hint` ∨ `d≥0.5` ∨ very-high P).  
3. **SpecRead** for handler / WAW-spine / no fanout (HotSet is **hint only**, not a hard gate).  
4. Production path: `Handler::run` default; gross-work `d` left `None` without inspect (fanout_hint substitutes).  
5. Beneficiary still skipped; account-grain not a Wait key.

Unit tests: `cargo test -p pevm --lib resolve::` — 8 passed.

## Constraints respected

- No pevm-specfence-server usage/modification.  
- seq≡par TCB; OCC/PCC largely unchanged.  
- Research flags only for journal/inspect.  
- No secrets committed; Alchemy unused this run (snapshots present).

## Follow-ups

- Optional SpecFence smoke/sweep on 597 + 598/599 after this tip.  
- Morphology heuristic vs plant labels (599/096).  
- Optional: plumb inspect-only gross-work depth into `PolicyCtx` behind `SPECFENCE_ENABLE_INSPECT`.
