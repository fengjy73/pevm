# SpecFence v9.4 full land — implementation map

**Date:** 2026-09-14  
**Branch:** `cursor/specfence-v94-full-land-46be`  
**Base tip:** `7bbb726` (`cursor/specfence-v8-pc-cc-computer-f6cf`)  
**SoT:** v9.4 file-SRP + v9.3 unified pevm spine + v9.1 call-flow / bars  
**Posture:** one PR, Soft=0, no P0/P1/P2, **no** `pc/`/`cc/`/`bayes/` ownership dirs.

---

## 0. What landed (code)

### S−1 — one pevm spine (v9.3)

| Before | After |
|--------|--------|
| Worker: `Occ ∨ plant_is_occ(empty PE ∨ quiet_fence_off)` → `next_occ_task` else `next_sf_task` | **SpecFence always** `next_sf_task` (empty extras ≡ OCC walk). `Occ` mode remains a **separate** computer |
| Execute: `plant_is_occ` → `try_execute` without wave/fence | SpecFence always wave/fence handles (cold = no-op meta) |
| Access gate: `plant_is_occ` early `Ok` including quiet-off | `specfence_cost_class_spec` = **empty PE only** (Mode(a)=Spec). Quiet-off no longer flips the computer |
| `quiet_lone_pe_keeps_occ_computer` | Replaced: PE-on stays on SpecFence spine; quiet still holds **verbs** (2179522) |

`specfence_plant_is_occ` is a deprecated alias of cost-class Spec (empty PE). It is **not** used as a schedule/validate retreat.

### S0 — file SRP (v9.4)

| Action | File |
|--------|------|
| **SPLIT** Fence policy out of `vm` | `specfence/fence_act.rs` (`FenceAct::{PinHold,DoneUnfenced,ReadyCanary}`) |
| **SPLIT** wave product surface | `specfence/wave.rs` **owns** `WaveParkTable` + `ParkKind`/`PinHold` (~530 LOC). SoftWait Soft + SuffixRepair remain in `rem.rs` (quarantined; Soft=0) |
| **SPLIT** feeder ≠ decide | `specfence/feeder.rs` (PE seed / abort observe → Bayes) |
| **NEW** admit_seed | `specfence/admit.rs` (begin_block + abort strengthen) |
| **DELETE** | `specfence/mode.rs` (3-LOC reexport) |
| **DELETE hot export** | `choose_edge_action`, `choose_action`, `bayes::{decide,should_wait_hard}` are `#[cfg(test)]` museums |
| **MERGE** kernel SoT | `CertificateTable::{rem_legal,may_resolve,repair_armed,begin_block}`; strips survive resume. `kernel.rs` is **`#[cfg(test)]` only** — not on Ctx, not allocated |
| **QUARANTINE** | `boundary` / `finegrain` remain compiled for inspect/lab opt-in; dual π bodies do **not** compile on the product path |

**Not a success criterion:** `pc/`/`cc/`/`bayes/` folders (banned). None created.

### Mechanism cuts (v9.1 on unified spine)

| # | Cut | Land |
|---|-----|------|
| 1 | Bayes→admit_seed | `admit_seed_begin_block`: known-star PE even on quiet morph; OrderedAdmit hinted accounts with ≥16 txs when fan/star |
| 2 | ProducerStage + refuse | unchanged API; abort path uses `admit_seed_on_abort` |
| 3 | PinWithoutThrow | `ParkKind::PinHold`; WaitFor pending park is PinHold; **no** steal-convert on PinHold |
| 4 | decide←Bayes | `decide_queried` + `BayesMap::query_access`; known-star opens WaitFor on quiet morph |
| 5 | R1 live | `validate_specfence`: snap/FF `identity_stable_match` **or** incarnation-strict `prior_read_value_stable`; selective fenced subset; `record_r1_win` |
| 6 | Cert survival | `begin_execute` no longer wipes locs on `inc==0`; `begin_block` is the only wipe |
| 7 | SerialLane | uses `act_serial_lane` (Done ≠ Bind count) |
| 8 | Telemetry | `waitfor_pin`, `waitfor_aborting`, `bind_after_done`, `r1_win`, `r1_attempt`, `schedule_refuse` |
| 9 | Quiet = Spec cost class | empty PE → Mode(a)=Spec on **same** `next_sf` / `validate_specfence` symbols |

**Bind-after-Done:** WaitFor/lane hitting a Done writer **certs** for R1 but **does not** `record_edge_bind` / `note_bind_success` (was the 442/473 theater).

**access_vis:** first ReadyEdge insert removed unless producer already predicted (admit_seed first).

---

## 1. Tests (this land)

| Suite | Result |
|-------|--------|
| `cargo test -p pevm --lib` | **202 passed** (incl. refuse inc==0, PinHold, decide EV, admit ≥16) |
| `--test specfence` | **42 passed**, 20 ignored |
| `--test raw_transfers` / `small_blocks` / `mixed` / `beneficiary` / `erc20` | **all passed** (re-run after wave extract + kernel merge + dual-π gate) |
| `--test uniswap` | **passed** (re-run after SRP close) |
| SoftWait Soft | **0** (asserted in specfence tests) |
| seq≡par | **held** on mocked clusters (one R1-overbind seq≠par caught and fixed: identity-without-value-proof) |

Ethereum/mainnet snapshot sweeps **not re-run** in this environment (no honest TPS JSON this PR).

---

## 2. Product bars (honesty)

| Bar | Status this PR |
|-----|----------------|
| Soft=0 | **held** (tests) |
| seq≡par | **held** (mocked + specfence) |
| Dual-computer tax | **killed in code** (no `plant_is_occ` → `next_occ_task` / `validate_occ_stage` on SpecFence) |
| nonempty median ≥0.95 | **not measured** — do not claim |
| 14689597 ≥0.90 @8 N≥3 | **not measured** — do not claim crush of 0.362 with JSON |
| quiet p10 ≥0.90 | **not measured** |
| R1 win ≥50% cert fan_out | **path live**; rate **not measured** |
| Bind-after-Done <10% | **count path fixed** (Done≠Bind); share **not measured** |

Tip honesty at base (`bb67ff7`): median **0.728** / fan **0.362**. This land is the call-order plant those numbers demanded. Wall-clock crush requires a named Soft=0 sweep JSON on the new binary.

---

## 3. File map

```
crates/pevm/src/pevm.rs          # unified next/execute/validate; admit_seed; PinHold park
crates/pevm/src/vm.rs            # thin gate → decide_queried + fence_act; PinHold park kind
crates/pevm/src/specfence/
  fence_act.rs                   # Fence verb policy (out of vm)
  admit.rs                       # begin_block / abort seed
  feeder.rs                      # PE/Bayes observe (learner feeder ≠ decide)
  wave.rs                        # WaveParkTable + PinHold (physical extract from rem)
  access_policy.rs               # ONE live decide ← Bayes
  bayes.rs                       # query_access / is_cold ports; Boolean π cfg(test)
  certificate.rs                 # strip survival + merged rem-legal SoT
  kernel.rs                      # cfg(test) museum only
  repair.rs                      # R1Selective grain
  executor.rs                    # cost_class_spec; R1 snap; no OCC retreat
  computer.rs                    # next_sf only (SpecFence spine)
  rem.rs                         # SuffixRepair + SoftWait Soft quarantine
  edge.rs / resolve.rs           # Detect/EV museums; choose_* cfg(test)
  mode.rs                        # DELETED
```

No `specfence/pc/`, `specfence/cc/`, `specfence/bayes/` directories.

---

## 4. Falsifiers still open (post-land)

- Mainnet all-blocks Soft=0 JSON not attached — **cannot** claim B1/B5.
- Dual π **bodies** remain as `#[cfg(test)]` museums (`edge`/`resolve`/`bayes` Boolean). Hot-path compile + export removed.
- `learner.rs` is still a megaclass (PE + morph + tax). Feeder is split; decide does **not** live there.
- SoftWait Soft arms still compile inside `rem` (product Soft=0; not default Avoid).
- OrderedAdmit (≥16 hinted txs) now seeds even on quiet-biased empty InterPrior; floor=2 when fan/prior/stars. Sweep must confirm 14689597 edges before satellite Execute.

---

## 5. Essence

One pevm spine, file-SRP, Bayes→admit→decide→PinHold→R1 **duties landed** (refuse fires, PinHold ≠ Aborting, decide consumes EV, R1b wired, admit seeds stars without prior gate). Soft=0, seq≡par tests green. Product TPS bars need a **new** Soft=0 sweep — this PR does not invent 0.95 or claim a crush of 0.685.

---

## Soft=0 honesty sweep (tip `3687da6`)

**When:** 2026-09-14 15:36 CST · artifacts: `lab/notes/v9.4-full-land-sweep-summary.json`, `lab/notes/specfence-v9.4-sweep-honesty.md`.

| | Base `bb67ff7` | This tip |
|--|---------------:|---------:|
| nonempty median SF/OCC N=1@8 | 0.728 | **0.6853** |
| quiet p10 | — | **0.5124** |
| 14689597 N=3@8 | 0.362 | **0.3482** |
| Soft | 0 | **0** (held) |
| WaitFor / Bind (N=1 agg) | — | 2151 / 2208 |
| R1 win/attempt | — | 6/1636 |
| Bind-after-Done | — | 205 |

Product bars still **not** met. Do not claim crush from call-order land alone.

---

## 6. SoT-gap close (successor of `b9903f2`)

**Date:** 2026-09-14  
**Posture:** finish named MISSING/PARTIAL duties — **no redesign**, no P0/P1/P2, Soft=0.

| Duty | Land |
|------|------|
| schedule-first Avoid | `try_execute_ready` refuses known consumers on **inc==0**; `record_schedule_refuse_n` from `next_sf_task` |
| ProducerStage refuse | first wave respects ReadyEdge while `w` Executing; Ready/Validated still canary |
| PinWithoutThrow | `add_pin_hold` (status stays Executing); wake `set_pin_ready` same incarnation; pevm PinHold arm does **not** `add_dependency` |
| BlockingOther not default | `fence_act::estimate_park_kind`; PE-known ESTIMATE / `fence_wait_for` → PinHold |
| decide←Bayes | `ev_pin_beats_abort` / `depth_frac` are the WaitFor spine; OR-bool is no-query adapter only |
| R1 live | `query_validate` + `repair_grain`; R1a value-stable; R1b `apply_suffix_repair` when EV/covers |
| admit_seed | ≥16-tx hints seed on quiet-biased empty InterPrior; floor=2 when fan/prior/stars |
| mid-tx bleed | `note_unpublished_raw` refresh-only unless predicted producer |
| file-SRP | ESTIMATE park-kind + Bind-rare EV in `fence_act`; `vm::fence_wait_for` museum; no `pc/cc/bayes` dirs |
| Bind rare | `bind_ev_from_query`: `ev_bind_beats_b0` only — `known_star` is pin/WaitFor, not Bind EV |
| true-k | admit plants Basic(addr) PE at k≈6 for hinted stars; `note_abort_access` skips any-k when class already seeded |

**Honesty:** tip sweep median **0.6853** / fan **0.3482** / R1 **6/1636** / refuse **0** is the *pre-close* wall at `3687da6`. This package lands the remaining PARTIAL rows. Product bars (median ≥0.95, fan ≥0.90, R1 ≥50%) stay **unclaimed** until a new Soft=0 JSON. Soft=0 held in tests.
