# SpecFence v10 — RAW_fan_out + mixed_RAW_WAW impl map

**Date:** 2026-09-14  
**Branch:** `cursor/specfence-v10-raw-mixed-e645`  
**SoT:** [`specfence-complete-architecture-v10-raw-mixed.md`](specfence-complete-architecture-v10-raw-mixed.md)  
**Vocabulary:** [`specfence-cc-glossary.md`](specfence-cc-glossary.md)  
**Sweep:** `lab/results/v10-raw-mixed-52-n1.json` (52 ids, Soft=0, N=1 @8)  
**Fan honesty:** `lab/results/v10-14689597-n3.json` (N=3 @8)

One pevm spine. No `pc/` `cc/` `bayes/` dirs. Soft=0.

---

## File → duty (landed)

| File | v10 job |
|------|---------|
| `admit.rs` | Hint-fan ReadyEdges + Basic PE@k≈6 + `note_raw_producer`; InterPrior / `query_admit` / `for_each_admit_hit` storage PE. **No** Basic→Storage clone. |
| `ready_edge.rs` | Known-consumer bits; **idempotent** `defer` (no 31k spin-count). |
| `scheduler.rs` | Ready\|Executing `refuse_admit`; **wave-fill steal** of next independent **without** `fetch_max` past refused holes. |
| `computer.rs` | `next_sf_task`: ProducerStage then wave. |
| `access_policy.rs` | Sole decide. WaitFor on a **single unfinished** writer (Ready or Executing) when Bayes EV says so. OrderedAdmit rare. |
| `fence_act.rs` | `wait_for_resume_armed`; Ready→WaitForDependency; DoneOptimisticRead `cert=false`. |
| `bayes.rs` | `query_admit` / `query_access` (depth_frac high for one unfinished) / `query_validate`. Observe on abort (feeder). |
| `vm.rs` | Access gate: no storage PE clone from Basic. `pcc_wait_for_writer` + ESTIMATE park **only if** resume armed; else OCC `optimistic_read` / BlockingOther. |
| `wave.rs` | Resume intent **only** for WaitForDependency with `armed_at_k>0` (not BlockingOther). |
| `rem.rs` | Unchanged product bars: rem ResumeAtK at k≥8; empty/tiny prefix → 0 (caller does not park). |
| `executor.rs` | `query_validate` gates PartialAbortRewind EV; strip-cover still wins when armed. |

---

## Fake welds closed (v9.4 why-slower)

| Weld | Land |
|------|------|
| WaitFor park → rem FullAbortReexecute default | Park only if `armed_at_k>0`. BlockingOther wake does **not** plant rem resume intent. 52-set `park_resume_full_abort_reexecute` **= 0**. |
| Ready-refuse idle / 31k spin | Idempotent defer + wave-fill steal (no index jump). 52-set `refuse_admit` **183** (not 31k). |
| Storage true-k clone from Basic | Removed. Storage PE = InterPrior / Bayes / abort true-k only. |
| decide WaitFor only if `writer_executing` | Single unfinished writer + EV → WaitFor. |
| ReadyCanary of known edge | Kept (`act_wait_for` Ready → WaitForDependency). |
| Wave-fill `fetch_max` skip | Reverted after iter11 SIGSEGV / lazy-eval unreachable. Steal only. |

---

## Tests

- `cargo test -p pevm --release --test specfence -- --test-threads=1` → **42 passed**, 20 ignored (inspect/jump museums). Soft=0 seq≡par on product path.
- Lean evm: raw_transfers / mixed / beneficiary / small_blocks green.
- New units: wave-fill independent; Ready WaitFor; `query_admit`; park-only-if-armed; BlockingOther no resume intent; idempotent defer.

---

## Sweep vs prior (honest)

| Set | Metric | v10 | Prior to beat |
|-----|--------|-----|----------------|
| 52 curated N=1 @8 | median SF/OCC | **0.589** | 0.703 was **98-block** nonempty — **not same set** |
| 52 | ≥1.0 / ≥0.70 / <0.50 | 5 / 14 / 19 | — |
| 52 | Soft=0 | **held** | held |
| 52 | `partial_abort` | **59/59** | timely-Resolve 1574/1581 on 98 |
| 52 | `park_resume_full_abort_reexecute` | **0** | 14689597 timely-Resolve 214 |
| 52 | wait_for_dependency / wait_for_full_abort / refuse | 630 / 1295 / 183 | — |
| **14689597 N=3 @8** | SF/OCC | **0.262** | **0.449** (PR #11) — **miss** |
| 14689597 N=3 | abort SF / OCC | 189 / 50 | still > OCC |

Mean 5.23 on N=1 is OCC-slow noise (e.g. 19932703 OCC 2337 ms). Do not cite mean.

**14689597 still loses:** abort > OCC and wall ~3–4×. WaitFor volume is low (6–13); residual is ESTIMATE `wait_for_full_abort` + first-wave storage RAW that admit cannot see (`to` ≠ slot). Schedule-first refuse does not cover the fan. Independents stay `optimistic_read`.

---

## Not landed / still open

- Storage RAW consumers at begin_block without traces (router `to`).
- ESTIMATE BlockingOther volume still ≫ WaitForDependency (OCC abort class, plus park idle).
- Product bars 0.95 / fan 0.90 / useful_EVM — **miss**.
- Do not celebrate refuse↑ or wait_for_dependency↑.
