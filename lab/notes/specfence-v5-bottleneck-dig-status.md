# SpecFence V5 — bottleneck dig status

**Date:** 2026-09-07 (Asia/Shanghai)  
**Branch:** `specfence`  
**Parent tip:** `582ea20` (V5-P3 no-graduate)  
**Protocol:** `lab/notes/specfence-v5-bottleneck-dig-plan.md` §3  
**Iron law:** `lab/notes/specfence-first-principles-bottleneck.md`

## Goal

Evidence-backed makespan decomposition on **14689597** @8 (spot 599/097/598). SoftWait scarce; no inspect graduate; no Wait ladders.

## Artifacts

| File | Role |
|------|------|
| `lab/results/v5-bottleneck-lean-sf-occ.json` | Quiet Lean G7 dig dump |
| `lab/results/v5-bottleneck-nosoft-sf-occ.json` | SoftWait forced-off A/B (`SPECFENCE_DISABLE_SOFTWAIT=1`) |
| `lab/results/v5-bottleneck-ab-summary.json` | 597 key numbers |
| `lab/results/v5-bottleneck-lean.run.log` / `*-nosoft.run.log` | Console |

Dig counters added (Lean-default, hang-free): `force_bind_reabort`, `soft_wait_wake_ok`, `soft_wait_wake_reabort`.  
`evm_ns_*` Instant **not** added — counter + wall split is unambiguous.

---

## Q1–Q4 table (597 Lean @8)

| Q | Answer | Evidence |
|---|--------|----------|
| **Q1** Where are interpreter-seconds? | **Abort-reexec dominates SF waste.** First-run ≤ n_tx; reexec_entries = evm − n_tx. Research rewind credit = 0 on Lean abort (`resume_count=0`). SoftWait idle secondary (arms 41). | SF `evm_entries=2416` (reexec **1852**) vs OCC `1486` (reexec **922**); `full_restart=378=occ_aborts`; `rewind_to_cp` only SoftWait-wake journal FF (24), not abort mid-tx |
| **Q2** SoftWait useful vs harmful? | **Scarce; weakly useful; not the SF/OCC hole.** Useful wake rate ~37%. Disabling SoftWait → SoftWait=0, SF/OCC **flat/worse**. | Lean SoftWait **41 ≪ 428**; wake ok/reabort **13/22**; nosoft SoftWait **0**, SF/OCC **0.137** vs lean **0.153** |
| **Q3** Bind / ForceBind quality? | **ForceBind is MV/π labeling, not CPU savings.** ~49% of aborts re-abort while force_bind armed. `partial_retry ≈ aborts`. | `force_bind_reabort=184` / `occ_aborts=378`; `partial_retry_count=395`; `bind_hits=2305` vs `spec_read=10436` (Bind share ~18%) |
| **Q4** Gap vs OCC makespan? | SF wall ≈ **6.6×** OCC; aborts ≈ **4.7×**; evm entries ≈ **1.6×**. Meta/fatter read path still hurts when SF evm ≤ OCC (nosoft). | Lean wall 36.1 vs 5.5 ms; nosoft wall 28.1 vs 3.8 ms with SF `evm=1294` **&lt;** OCC `1559` |

### SoftWait A/B (597)

| Arm | SoftWait | SF/OCC | aborts SF/OCC | evm SF/OCC | wall SF/OCC (ms) | force_bind_reabort |
|-----|--------:|-------:|--------------:|-----------:|-----------------:|-------------------:|
| Lean default | **41** | 0.153 | 378 / 80 | 2416 / 1486 | 36.1 / 5.5 | 184 |
| SoftWait off | **0** | 0.137 | 228 / 77 | 1294 / 1559 | 28.1 / 3.8 | 110 |

SoftWait stays ≪428 on default. Turning it off does **not** close the OCC gap → SoftWait is not the primary bottleneck.

### Spot cores (Lean SoftWait / SF/OCC / aborts SF÷OCC)

| Block | SoftWait | SF/OCC | aborts SF/OCC | evm SF/OCC |
|------:|--------:|-------:|--------------:|-----------:|
| 14689597 | 41 | 0.153 | 378/80 | 2416/1486 |
| 19606599 | 11 | 0.261 | 84/84 | 650/661 |
| 19469097 | 55 | 0.340 | 76/112 | 567/716 |
| 19606598 | 2 | 0.316 | 6/7 | 123/122 |

Mean SF/OCC lean ≈ **0.27** (noise vs P3 band).

---

## SF vs OCC waste breakdown (597 Lean)

```text
OCC:  T ≈ T_crit + abort_reexec(80 aborts, 922 reexec entries)           wall ~5.5ms
SF:   T ≈ T_crit + abort_reexec(378 aborts, 1852 reexec entries)
           + SoftWait/EarlyAbort park idle (park_count=1474, steal=80)
           + meta on hot path (Bayes/HotSet/Fence/π per location)
           + ForceBind head reexec (no mid-tx skip)                      wall ~36ms
```

| Bucket | SF | OCC | Note |
|--------|---:|----:|------|
| First-run EVM (proxy n_tx) | 564 | 564 | Same block |
| Abort reexec entries | 1852 | 922 | SF pays more restarts |
| Validation aborts | 378 | 80 | SF storms worse on 597 |
| Mid-tx resume credit | 0 | n/a | Lean abort clears RewindTo |
| SoftWait arms | 41 | 0 | Scarce |
| ForceBind then reabort | 184 | 0 | Labels ≠ fewer EVM runs |
| Ready-steal on park | 80 | 0 | ≪ park_count → false idle |

---

## Root cause ranking (why SF loses)

1. **R1 — Repair grain ≠ cost grain (dominant on Lean abort path)**  
   Semantic PartialRetry / ForceBind ≈ abort count; interpreter still restarts from tx head (`full_restart = occ_aborts`, `resume_count = 0`). **`force_bind_reabort / aborts ≈ 0.49`** proves Bind prefix often fails again after paying full EVM. Matches first-principles R1.

2. **R3 — Meta / fat hot-path tax (dominant when entry counts are close)**  
   Nosoft A/B: SF can have **fewer** `evm_entries` than OCC and still lose ~7× wall → decision path + fencing constants tax every location access beyond abort count alone.

3. **R4 — Discovery already cheap for OCC; SF aborts more on 597**  
   Abort ratio SF/OCC ≈ **4.7×** on the dig target. SpecFence’s admission/filter does not reduce first-pass wrong work enough to offset overhead; often increases incarnations.

4. **R2 — Wait/park converts parallelism into pipeline (secondary)**  
   SoftWait scarce and not the SF/OCC driver (A/B flat). Remaining **EarlyAbort / Blocking parks** (`wait_park_count` ≫ `ready_steal_on_wait`) still leave false idle on the critical path.

5. **Bind hit rate looks healthy but does not cut interpreter-seconds**  
   Thousands of `bind_hits` coexist with ForceBind reaborts and head reexec — MV origin quality ≠ CPU savings under Lean plant.

---

## Next actions (iron law only)

Concrete; **no** WaitHard ladders / SoftWait storms / inspect graduate:

1. **Cut abort interpreter-seconds on a Lean-safe subset** — hang-free mid-tx resume / EffectBoundary+journal-FF abort arm only where dig shows ForceBind reabort clusters and a proof path that does not hang 597 (research inspect stays opt-in). Success metric: `evm_entries` and wall down vs OCC, not SoftWait count.

2. **OCC-matching lean engagement under low contention** — disable SpecFence meta on the absolute hottest read path until a contention tripwire (adaptive engagement that actually removes Bayes/Fence/π from cold txs). Success: quiet blocks (598-class) SF wall ≈ OCC wall.

3. **Park → steal first** — when SoftWait/EarlyAbort parks, guarantee ready independent txs run (`ready_steal_on_wait` ≪ `wait_park_count` today). Success: lower wall without raising SoftWait arms.

4. **Do not** chase SoftWait thresholds, Boolean Wait ladders, or default inspect — dig shows SoftWait≪428 and inspect already hangs (V5-P3).

---

## Code / dig instrumentation

- `SPECFENCE_DISABLE_SOFTWAIT=1` — π SpecRead-only; never SoftWait-arm (WaitHard remap + Bind unfinished-writer → SpecRead).
- Metrics: `force_bind_reabort`, `soft_wait_wake_{ok,reabort}` (SoftWait-park attributed only).
- G7 smoke: dig rows (`wall_ms`, `n_tx`, `reexec_entries`, ForceBind/SoftWait wake); `SPECFENCE_G7_TAG=…` output naming.

## Tests

```text
cargo test -p pevm --lib
cargo test -p pevm --test specfence
```

Both green after dig changes.

## Forbidden (unchanged)

Fanout→WaitHard, Boolean Wait ladders, Heat sticky Wait, default-on inspect.
