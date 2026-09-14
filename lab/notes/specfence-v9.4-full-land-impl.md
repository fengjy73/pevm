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
| **SPLIT** wave product surface | `specfence/wave.rs` (re-exports `WaveParkTable`; SoftWait stays in `rem`, Soft=0) |
| **SPLIT** feeder ≠ decide | `specfence/feeder.rs` (PE seed / abort observe → Bayes) |
| **NEW** admit_seed | `specfence/admit.rs` (begin_block + abort strengthen) |
| **DELETE** | `specfence/mode.rs` (3-LOC reexport) |
| **DELETE hot export** | `choose_edge_action`, `choose_action` no longer `mod.rs` production π |
| **MERGE** kernel SoT | `CertificateTable::{rem_legal,may_resolve,repair_armed,begin_block}`; strips survive resume |
| **QUARANTINE** | `boundary` / `finegrain` / `edge`/`resolve`/`bayes` Boolean π remain compiled for tests/museum; not live decide |

**Not done (honest):** physical extract of 1k+ WavePark lines out of `rem.rs`; `pc/`/`cc/`/`bayes/` folders (banned as success). `kernel.rs` still allocated (Ctx field) as rem-legal mirror.

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
| `cargo test -p pevm --lib` | **191 passed** |
| `--test specfence` | **42 passed**, 20 ignored |
| `--test raw_transfers` / `small_blocks` / `mixed` / `beneficiary` / `erc20` | **all passed** |
| `--test uniswap` | **passed** |
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
  fence_act.rs                   # NEW — Fence verb policy
  admit.rs                       # NEW — begin_block / abort seed
  feeder.rs                      # NEW — PE/Bayes observe
  wave.rs                        # NEW — product WavePark surface
  access_policy.rs               # decide_queried ← Bayes
  bayes.rs                       # query_access / is_cold ports
  certificate.rs                 # strip survival + kernel merge APIs
  repair.rs                      # R1Selective grain
  executor.rs                    # cost_class_spec; R1 snap; no OCC retreat
  computer.rs                    # next_sf only (SpecFence spine)
  rem.rs                         # ParkKind::PinHold
  mode.rs                        # DELETED
```

---

## 4. Falsifiers still open (post-land)

- Mainnet all-blocks Soft=0 JSON not attached — **cannot** claim B1/B5.
- `rem.rs` still a god (wave code physically inside; `wave.rs` is the product name).
- `kernel.rs` still wired on Ctx (certificate is SoT for resolve).
- Dual π **bodies** still compile (`edge`/`resolve` tests). Hot-path export removed.
- OrderedAdmit (≥16 hinted txs) only when fan/star — first-block 14689597 with empty InterPrior still learns after first abort.

---

## 5. Essence

One pevm spine, file-SRP splits (not three folders), Bayes→admit→decide→PinHold→R1, Soft=0, seq≡par tests green. Product TPS bars need a sweep — this PR does not invent 0.95.
