# SpecFence — 10-block parallel upper bound (Soft=0)

**Date:** 2026-09-14 (Asia/Shanghai)  
**Tip:** `2cbd339` / `cursor/specfence-cc-glossary-302f` (glossary rename only; no plant edits)  
**Vocabulary:** [`specfence-cc-glossary.md`](specfence-cc-glossary.md) — wait-for dependency, partial abort, full abort + re-execute, optimistic read, ordered/pessimistic admit, dependency-aware admission (`refuse_admit`), RAW/WAR/WAW  
**Companion:** [`specfence-10block-parallel-upper-bound-catalog.json`](specfence-10block-parallel-upper-bound-catalog.json)  
**SoftWait Soft:** **0** on all Soft=0 process traces

---

## Method

Consensus fixes commit order. The **theoretical parallel upper bound** is the conflict DAG under that order:

1. Build final-RW **RAW + WAW** edges (beneficiary / `basic_lazy` excluded) via `analyze_dag` / finegrain OCC snapshot.
2. **Critical path** `L = longest_chain` (tx-hop length).
3. **Max independent width** `W = max_wave_width` (largest ready wave / antichain).
4. Equal-cost bound: `speedup_∞ = n_tx / L`, `speedup_@8 = min(8, n_tx/L, W)`.
5. `t_work` = sequential wall, or **OCC@1** when sequential is pathological (`serial > 5× OCC@1`, seen on 19807137).
6. `ideal_wall_@8 = t_work / speedup_@8`. Gap = measured OCC@8 / ideal.

Evidence: `lab/results/10block-parallel-upper-bound-finegrain.json`, Soft=0 N=1@8 `tps-stall-10block-soft0-n1.json`, process dumps, effect-raw-deeper on 14689597 / 19606599 / 19469097, contiguous finegrain on the same three.

WAR is rare in these final-RW snapshots (read-after-write dominates as RAW; write-after-write as WAW). Journal effect-raw adds mid-tx RAW instances and discovery depth.

### Blocks (explicit)

| # | Block | Role |
|--:|------:|------|
| 1 | 14689597 | fan_out (named) |
| 2 | 19807137 | spine (named) |
| 3 | 19606599 | named |
| 4 | 19469097 | named |
| 5 | 2179522 | quiet (named) |
| 6 | 6196166 | Soft=0 worst WAW spine (sf_occ≪1 in prior sweeps) |
| 7 | 8889776 | Soft=0 worst/mixed spine |
| 8 | 6137495 | Soft=0 worst WAW spine |
| 9 | 14396881 | quiet-morph but sf_occ≪1 (mixed) |
| 10 | 12047794 | quiet-parity |

---

## Headline bound table

| block | morph | n | L | W | RAW | WAW | bound@8 | ideal@8 ms | OCC@8 ms | OCC/ideal | Soft=0 SF ms | SF/OCC_fg |
|------:|-------|--:|--:|--:|----:|----:|--------:|-----------:|---------:|----------:|-------------:|----------:|
| 14689597 | RAW_fan_out | 564 | 29 | 434 | 449 | 145 | 8.00× | 0.48 | 5.64 | 11.77× | 25.369 | 0.22× |
| 19807137 | WAW_spine | 712 | 571 | 105 | 9 | 629 | 1.25× | 16.98 | 11.92 | 0.70× | 75.188 | 0.16× |
| 19606599 | mixed_RAW_WAW | 367 | 57 | 261 | 42 | 177 | 6.44× | 2.97 | 10.61 | 3.57× | 19.646 | 0.54× |
| 19469097 | mixed_RAW_WAW | 336 | 47 | 197 | 28 | 182 | 7.15× | 1.32 | 6.69 | 5.08× | 14.485 | 0.46× |
| 2179522 | near_independent | 222 | 2 | 221 | 0 | 1 | 8.00× | 0.04 | 1.07 | 29.93× | 1.505 | 0.71× |
| 6196166 | WAW_spine | 108 | 49 | 25 | 0 | 249 | 2.20× | 0.32 | 1.70 | 5.40× | 7.362 | 0.23× |
| 8889776 | mixed_RAW_WAW | 330 | 56 | 128 | 16 | 225 | 5.89× | 0.40 | 2.21 | 5.51× | 6.952 | 0.32× |
| 6137495 | WAW_spine | 60 | 33 | 28 | 0 | 32 | 1.82× | 0.60 | 1.17 | 1.95× | 3.313 | 0.35× |
| 14396881 | near_independent | 1346 | 5 | 1337 | 0 | 13 | 8.00× | 0.47 | 4.67 | 9.95× | 13.974 | 0.33× |
| 12047794 | near_independent | 232 | 1 | 232 | 0 | 0 | 8.00× | 0.60 | 4.26 | 7.10× | 5.965 | 0.71× |

Notes: **OCC/ideal > 1** means pure OCC leaves bound on the table. **OCC/ideal < 1** (19807137) means equal-cost hops over-estimate spine cost — OCC already beats the unit model by overlapping non-spine work; the *structural* bound remains `n/L ≈ 1.25×`. SF/OCC_fg uses finegrain OCC@8 (Soft=0 OCC@8 on 19807137 was pathological ~2685 ms — ignored for ratios).

---

## Block 14689597 (fan_out) — `RAW_fan_out`

### 1. Theoretical parallel upper bound

- **n_tx** = 564, gas = 30,028,257, DAG source = `finegrain_final_rw`.
- Conflict DAG: **RAW=449**, **WAW=145**, edges=594.
- Critical path **L=29**; max width **W=434**; indep_frac=0.099.
- Multi-writer locs=25; max writers on one loc=26; max conflict component=476.
- Bound speedup: **∞-cores 19.45×**, **@8 8.00×**.
- Work proxy `sequential` t_work=3.83 ms → ideal@8 **0.48 ms**.
- Measured: sequential=3.83 ms, OCC@1=7.44 ms, OCC@8=**5.64 ms** (0.68× vs work).
- Hottest loc: kind=`storage` writers=26 readers=474.

### 2. Why pure OCC falls short of the bound

- Dominant stage: **`optimistic_read_then_validate_fail_full_abort_reexecute_fanout`**.
- OCC@8 abort_rate=0.12411347517730496; occ_aborts=70; max_incarnation=8.
- OCC@8 / ideal@8 = **11.77×** (waste factor under equal-cost model).
- Morphology: **program storage RAW fan-out** — 26 early writers → ~474 readers on one slot; program-RAW path length only 2 while effective L=29 (star, not deep pipeline).
- Stage waste: consumers take **optimistic read** of unfinished / wrong version → validate → **full abort + re-execute** (contiguous reexec_frac≈0.65 historically; Soft=0 SF still full_abort_reexecute=226).
- Effect-raw: max_program_fanout=448; gross_work_depth_p50=0.943 → majority discover RAW **late** in billed work; abort-at-discovery has already sunk most gas.
- Idle is not the wall; **wrong work** (re-execute) is.

### 3. What must change at those sites

- Target stage/sites: `optimistic_read_then_validate_fail_full_abort_reexecute_fanout`.
- dependency_aware_admission: prefer-admit early writers before fan-out consumers
- wait_for_dependency on predicted-essential RAW once producer Executing (not optimistic_read)
- ordered_admit when producer published+EV
- partial_abort (rebind/rewind) when certified prefix exists; avoid full_abort_reexecute storms

### 4. SpecFence design to approach the bound

- Soft=0 counters: wait_for_dependency=367, wait_for_full_abort=16, refuse_admit=1221, partial_abort=1/1, full_abort_reexecute=226, park_resume_full_abort_reexecute=365, optimistic_read=4, ordered_admit=0.
- **Admit early writers first** (dependency-aware admission), then **wait_for_dependency** or **ordered_admit** on the fan-out RAW — do not optimistic_read the hot slot.
- When a certified prefix exists, **partial abort (rebind/rewind)**; stop converting WaitFor wakes into full_abort_reexecute.
- Late gross-work discovery (p50≈0.94) means Avoid must be **pre-access** (admission / predicted essential), not post-fail Detect.

---

## Block 19807137 (spine) — `WAW_spine`

### 1. Theoretical parallel upper bound

- **n_tx** = 712, gas = 29,981,386, DAG source = `finegrain_final_rw`.
- Conflict DAG: **RAW=9**, **WAW=629**, edges=638.
- Critical path **L=571**; max width **W=105**; indep_frac=0.131.
- Multi-writer locs=31; max writers on one loc=571; max conflict component=571.
- Bound speedup: **∞-cores 1.25×**, **@8 1.25×**.
- Work proxy `occ1` t_work=21.17 ms (sequential pathological) → ideal@8 **16.98 ms**.
- Measured: sequential=2769.55 ms, OCC@1=21.17 ms, OCC@8=**11.92 ms** (1.78× vs work).
- Hottest loc: kind=`storage` writers=571 readers=571.

### 2. Why pure OCC falls short of the bound

- Dominant stage: **`execute_validate_full_abort_reexecute_on_WAW_chain`**.
- OCC@8 abort_rate=0.8286516853932584; occ_aborts=590; max_incarnation=12.
- OCC@8 / ideal@8 = **0.70×** (waste factor under equal-cost model).
- Morphology: **WAW spine** — one storage location with **571 writers**; RAW only 9. Critical path L=571 ⇒ structural bound ≈1.25×.
- OCC@8 still overlaps non-spine txs (beats unit ideal), but abort_rate≈0.83 / max_inc≈12: validate storms along the spine.
- Soft=0 process: wait_for_dependency≈1337 with park_resume_full_abort_reexecute≈1324 — waits convert to **full abort + re-execute** instead of clean wake.
- Partial abort is active here (win≈505) — the one block where Resolve conversion is real — yet SF wall still ≫ finegrain OCC.

### 3. What must change at those sites

- Target stage/sites: `execute_validate_full_abort_reexecute_on_WAW_chain`.
- pessimistic_admit / ordered_admit (serial-lane) along multi-writer WAW locations
- dependency_aware_admission refuse_admit of later writers while prior writer Ready|Executing
- partial_abort for incidental RAW side-edges; do not full_abort_reexecute the whole spine
- wait_for_dependency only on unfinished prior writer — never SoftWait Soft

### 4. SpecFence design to approach the bound

- Soft=0 counters: wait_for_dependency=1337, wait_for_full_abort=4, refuse_admit=6961, partial_abort=505/503, full_abort_reexecute=232, park_resume_full_abort_reexecute=1324, optimistic_read=137, ordered_admit=0.
- Treat the multi-writer location as a **serial lane**: pessimistic/ordered admit in commit order; refuse_admit later writers while prior is Ready|Executing.
- Keep independent txs on optimistic_read so W≈105 is usable; only the spine is serialized (matches L-bound).
- Preserve partial_abort for side RAW; do not full_abort_reexecute the entire spine tx on incidental fails.

---

## Block 19606599 (mixed_cancun) — `mixed_RAW_WAW`

### 1. Theoretical parallel upper bound

- **n_tx** = 367, gas = 29,981,684, DAG source = `finegrain_final_rw`.
- Conflict DAG: **RAW=42**, **WAW=177**, edges=219.
- Critical path **L=57**; max width **W=261**; indep_frac=0.643.
- Multi-writer locs=87; max writers on one loc=54; max conflict component=79.
- Bound speedup: **∞-cores 6.44×**, **@8 6.44×**.
- Work proxy `sequential` t_work=19.14 ms → ideal@8 **2.97 ms**.
- Measured: sequential=19.14 ms, OCC@1=23.23 ms, OCC@8=**10.61 ms** (1.80× vs work).
- Hottest loc: kind=`basic` writers=54 readers=74.

### 2. Why pure OCC falls short of the bound

- Dominant stage: **`mixed_abort_reexecute_plus_wave_underfill`**.
- OCC@8 abort_rate=0.23433242506811988; occ_aborts=86; max_incarnation=9.
- OCC@8 / ideal@8 = **3.57×** (waste factor under equal-cost model).
- Mixed Cancun: program RAW (long lag) + handler/basic WAW chatter; L=57, indep≈0.64, bound≈6.4×; OCC only ≈1.8× work.
- Abort_rate≈0.23, max_inc≈9; cascade validations on early writers (contiguous).
- Effect-raw gw_p50=0.381; OCC@8 waitish producer frac=0.12627669452181986.
- Soft=0: refuse_admit≈16k (idle tax) while wait_for_dependency≈146.

### 3. What must change at those sites

- Target stage/sites: `mixed_abort_reexecute_plus_wave_underfill`.
- split policy: program RAW → wait_for_dependency/ordered_admit; handler chatter → optimistic_read unless abort spikes
- dependency_aware_admission on Ready|Executing producers without over-serializing independents
- partial_abort over full_abort_reexecute when strips cover

### 4. SpecFence design to approach the bound

- Soft=0 counters: wait_for_dependency=146, wait_for_full_abort=5, refuse_admit=16342, partial_abort=24/24, full_abort_reexecute=52, park_resume_full_abort_reexecute=80, optimistic_read=7, ordered_admit=0.
- Split PE class: program RAW → wait_for_dependency / ordered_admit; handler short-lag → optimistic_read.
- Dependency-aware admission for Ready|Executing producers without serializing the indep majority (indep_frac>0.5).
- Raise partial_abort / full_abort ratio; kill park_resume_full_abort_reexecute.

---

## Block 19469097 (mixed_waw) — `mixed_RAW_WAW`

### 1. Theoretical parallel upper bound

- **n_tx** = 336, gas = 22,901,234, DAG source = `finegrain_final_rw`.
- Conflict DAG: **RAW=28**, **WAW=182**, edges=210.
- Critical path **L=47**; max width **W=197**; indep_frac=0.542.
- Multi-writer locs=53; max writers on one loc=47; max conflict component=51.
- Bound speedup: **∞-cores 7.15×**, **@8 7.15×**.
- Work proxy `sequential` t_work=9.43 ms → ideal@8 **1.32 ms**.
- Measured: sequential=9.43 ms, OCC@1=12.27 ms, OCC@8=**6.69 ms** (1.41× vs work).
- Hottest loc: kind=`unknown` writers=0 readers=108.

### 2. Why pure OCC falls short of the bound

- Dominant stage: **`mixed_abort_reexecute_plus_wave_underfill`**.
- OCC@8 abort_rate=0.2767857142857143; occ_aborts=93; max_incarnation=6.
- OCC@8 / ideal@8 = **5.08×** (waste factor under equal-cost model).
- Mixed WAW spines (storage chains length 47) + program RAW; bound≈7.2×; OCC≈1.4×.
- Abort_rate≈0.28; top aborts are early writers with huge cascade_validations.
- Effect-raw: longest_effect_program_path=47; gw_p50=0.611.

### 3. What must change at those sites

- Target stage/sites: `mixed_abort_reexecute_plus_wave_underfill`.
- split policy: program RAW → wait_for_dependency/ordered_admit; handler chatter → optimistic_read unless abort spikes
- dependency_aware_admission on Ready|Executing producers without over-serializing independents
- partial_abort over full_abort_reexecute when strips cover

### 4. SpecFence design to approach the bound

- Soft=0 counters: wait_for_dependency=165, wait_for_full_abort=5, refuse_admit=8442, partial_abort=32/33, full_abort_reexecute=71, park_resume_full_abort_reexecute=141, optimistic_read=9, ordered_admit=0.
- Split PE class: program RAW → wait_for_dependency / ordered_admit; handler short-lag → optimistic_read.
- Dependency-aware admission for Ready|Executing producers without serializing the indep majority (indep_frac>0.5).
- Raise partial_abort / full_abort ratio; kill park_resume_full_abort_reexecute.

---

## Block 2179522 (quiet) — `near_independent`

### 1. Theoretical parallel upper bound

- **n_tx** = 222, gas = 4,698,004, DAG source = `finegrain_final_rw`.
- Conflict DAG: **RAW=0**, **WAW=1**, edges=1.
- Critical path **L=2**; max width **W=221**; indep_frac=0.991.
- Multi-writer locs=1; max writers on one loc=2; max conflict component=2.
- Bound speedup: **∞-cores 111.00×**, **@8 8.00×**.
- Work proxy `sequential` t_work=0.29 ms → ideal@8 **0.04 ms**.
- Measured: sequential=0.29 ms, OCC@1=1.12 ms, OCC@8=**1.07 ms** (0.27× vs work).
- Hottest loc: kind=`basic_lazy` writers=209 readers=9.

### 2. Why pure OCC falls short of the bound

- Dominant stage: **`scheduler_meta_overhead`**.
- OCC@8 abort_rate=0.0045045045045045045; occ_aborts=1; max_incarnation=1.
- OCC@8 / ideal@8 = **29.93×** (waste factor under equal-cost model).
- Near-independent (L=2, RAW=0, WAW=1). Bound@8 = 8× from width.
- OCC@8 **slower than sequential** in finegrain — pure **scheduler/meta overhead**, not conflict waste.
- Soft=0 SpecFence can beat a cold OCC@8 N=1 (reported sf_occ>1) but vs finegrain OCC still ≈0.7× — quiet parity is a meta problem.

### 3. What must change at those sites

- Target stage/sites: `scheduler_meta_overhead`.
- keep optimistic_read default; minimize refuse_admit idle tax
- avoid wait_for_dependency and ordered_admit theater on cold locations
- SpecFence should match OCC wall (meta overhead is the only gap)

### 4. SpecFence design to approach the bound

- Soft=0 counters: wait_for_dependency=0, wait_for_full_abort=0, refuse_admit=80, partial_abort=0/0, full_abort_reexecute=0, park_resume_full_abort_reexecute=0, optimistic_read=0, ordered_admit=0.
- Stay on **optimistic_read**; drive refuse_admit→0 and wait_for_dependency→0 on cold locs.
- Success metric: SF wall ≤ OCC wall (bound already ≈8× and conflict-free).

---

## Block 6196166 (soft0_worst_waw_spine) — `WAW_spine`

### 1. Theoretical parallel upper bound

- **n_tx** = 108, gas = 7,975,867, DAG source = `finegrain_final_rw`.
- Conflict DAG: **RAW=0**, **WAW=249**, edges=249.
- Critical path **L=49**; max width **W=25**; indep_frac=0.204.
- Multi-writer locs=9; max writers on one loc=49; max conflict component=49.
- Bound speedup: **∞-cores 2.20×**, **@8 2.20×**.
- Work proxy `sequential` t_work=0.70 ms → ideal@8 **0.32 ms**.
- Measured: sequential=0.70 ms, OCC@1=1.48 ms, OCC@8=**1.70 ms** (0.41× vs work).
- Hottest loc: kind=`storage` writers=49 readers=49.

### 2. Why pure OCC falls short of the bound

- Dominant stage: **`execute_validate_full_abort_reexecute_on_WAW_chain`**.
- OCC@8 abort_rate=0.7129629629629629; occ_aborts=77; max_incarnation=8.
- OCC@8 / ideal@8 = **5.40×** (waste factor under equal-cost model).
- WAW spine with RAW=0: L=49, max_writers=49. Bound only ≈2.20×.
- OCC abort_rate high; often OCC@8 ≥ sequential — re-execute eats the small available parallelism.
- Soft=0: wait_for_dependency parks convert heavily to park_resume_full_abort_reexecute.

### 3. What must change at those sites

- Target stage/sites: `execute_validate_full_abort_reexecute_on_WAW_chain`.
- pessimistic_admit / ordered_admit (serial-lane) along multi-writer WAW locations
- dependency_aware_admission refuse_admit of later writers while prior writer Ready|Executing
- partial_abort for incidental RAW side-edges; do not full_abort_reexecute the whole spine
- wait_for_dependency only on unfinished prior writer — never SoftWait Soft

### 4. SpecFence design to approach the bound

- Soft=0 counters: wait_for_dependency=152, wait_for_full_abort=0, refuse_admit=0, partial_abort=0/0, full_abort_reexecute=75, park_resume_full_abort_reexecute=152, optimistic_read=9, ordered_admit=0.
- Treat the multi-writer location as a **serial lane**: pessimistic/ordered admit in commit order; refuse_admit later writers while prior is Ready|Executing.
- Keep independent txs on optimistic_read so W≈105 is usable; only the spine is serialized (matches L-bound).
- Preserve partial_abort for side RAW; do not full_abort_reexecute the entire spine tx on incidental fails.

---

## Block 8889776 (soft0_worst_spine) — `mixed_RAW_WAW`

### 1. Theoretical parallel upper bound

- **n_tx** = 330, gas = 9,996,021, DAG source = `finegrain_final_rw`.
- Conflict DAG: **RAW=16**, **WAW=225**, edges=241.
- Critical path **L=56**; max width **W=128**; indep_frac=0.312.
- Multi-writer locs=50; max writers on one loc=56; max conflict component=59.
- Bound speedup: **∞-cores 5.89×**, **@8 5.89×**.
- Work proxy `sequential` t_work=2.36 ms → ideal@8 **0.40 ms**.
- Measured: sequential=2.36 ms, OCC@1=4.59 ms, OCC@8=**2.21 ms** (1.07× vs work).
- Hottest loc: kind=`storage` writers=56 readers=56.

### 2. Why pure OCC falls short of the bound

- Dominant stage: **`mixed_abort_reexecute_plus_wave_underfill`**.
- OCC@8 abort_rate=0.19696969696969696; occ_aborts=65; max_incarnation=5.
- OCC@8 / ideal@8 = **5.51×** (waste factor under equal-cost model).
- Mixed spine: L=56, W=128, RAW=16/WAW=225; bound≈5.9×; OCC≈1.1× work.
- Soft=0 partial_abort wins≈30 but SF still 0.25–0.32× OCC — wait/refuse path tax.

### 3. What must change at those sites

- Target stage/sites: `mixed_abort_reexecute_plus_wave_underfill`.
- split policy: program RAW → wait_for_dependency/ordered_admit; handler chatter → optimistic_read unless abort spikes
- dependency_aware_admission on Ready|Executing producers without over-serializing independents
- partial_abort over full_abort_reexecute when strips cover

### 4. SpecFence design to approach the bound

- Soft=0 counters: wait_for_dependency=86, wait_for_full_abort=7, refuse_admit=1344, partial_abort=30/31, full_abort_reexecute=18, park_resume_full_abort_reexecute=92, optimistic_read=8, ordered_admit=0.
- Split PE class: program RAW → wait_for_dependency / ordered_admit; handler short-lag → optimistic_read.
- Dependency-aware admission for Ready|Executing producers without serializing the indep majority (indep_frac>0.5).
- Raise partial_abort / full_abort ratio; kill park_resume_full_abort_reexecute.

---

## Block 6137495 (soft0_worst_waw_spine) — `WAW_spine`

### 1. Theoretical parallel upper bound

- **n_tx** = 60, gas = 7,994,690, DAG source = `finegrain_final_rw`.
- Conflict DAG: **RAW=0**, **WAW=32**, edges=32.
- Critical path **L=33**; max width **W=28**; indep_frac=0.450.
- Multi-writer locs=1; max writers on one loc=33; max conflict component=33.
- Bound speedup: **∞-cores 1.82×**, **@8 1.82×**.
- Work proxy `sequential` t_work=1.09 ms → ideal@8 **0.60 ms**.
- Measured: sequential=1.09 ms, OCC@1=2.02 ms, OCC@8=**1.17 ms** (0.93× vs work).
- Hottest loc: kind=`storage` writers=33 readers=33.

### 2. Why pure OCC falls short of the bound

- Dominant stage: **`execute_validate_full_abort_reexecute_on_WAW_chain`**.
- OCC@8 abort_rate=0.5333333333333333; occ_aborts=32; max_incarnation=6.
- OCC@8 / ideal@8 = **1.95×** (waste factor under equal-cost model).
- WAW spine with RAW=0: L=33, max_writers=33. Bound only ≈1.82×.
- OCC abort_rate high; often OCC@8 ≥ sequential — re-execute eats the small available parallelism.
- Soft=0: wait_for_dependency parks convert heavily to park_resume_full_abort_reexecute.

### 3. What must change at those sites

- Target stage/sites: `execute_validate_full_abort_reexecute_on_WAW_chain`.
- pessimistic_admit / ordered_admit (serial-lane) along multi-writer WAW locations
- dependency_aware_admission refuse_admit of later writers while prior writer Ready|Executing
- partial_abort for incidental RAW side-edges; do not full_abort_reexecute the whole spine
- wait_for_dependency only on unfinished prior writer — never SoftWait Soft

### 4. SpecFence design to approach the bound

- Soft=0 counters: wait_for_dependency=51, wait_for_full_abort=0, refuse_admit=0, partial_abort=21/21, full_abort_reexecute=10, park_resume_full_abort_reexecute=51, optimistic_read=4, ordered_admit=0.
- Treat the multi-writer location as a **serial lane**: pessimistic/ordered admit in commit order; refuse_admit later writers while prior is Ready|Executing.
- Keep independent txs on optimistic_read so W≈105 is usable; only the spine is serialized (matches L-bound).
- Preserve partial_abort for side RAW; do not full_abort_reexecute the entire spine tx on incidental fails.

---

## Block 14396881 (quiet_morph_sf_occ_low) — `near_independent`

### 1. Theoretical parallel upper bound

- **n_tx** = 1346, gas = 30,020,813, DAG source = `finegrain_final_rw`.
- Conflict DAG: **RAW=0**, **WAW=13**, edges=13.
- Critical path **L=5**; max width **W=1337**; indep_frac=0.989.
- Multi-writer locs=9; max writers on one loc=5; max conflict component=6.
- Bound speedup: **∞-cores 269.20×**, **@8 8.00×**.
- Work proxy `sequential` t_work=3.76 ms → ideal@8 **0.47 ms**.
- Measured: sequential=3.76 ms, OCC@1=9.51 ms, OCC@8=**4.67 ms** (0.80× vs work).
- Hottest loc: kind=`basic_lazy` writers=1197 readers=2.

### 2. Why pure OCC falls short of the bound

- Dominant stage: **`scheduler_meta_overhead`**.
- OCC@8 abort_rate=0.004457652303120356; occ_aborts=6; max_incarnation=1.
- OCC@8 / ideal@8 = **9.95×** (waste factor under equal-cost model).
- Quiet morph, n_tx=1346, L=5, indep≈0.99, bound=8× — **almost no RAW/WAW**.
- OCC≈serial (meta); Soft=0 SF ≈0.28–0.33× finegrain OCC: cold-path / refuse_admit / scheduling overhead on a large independent block — not a conflict bound problem.

### 3. What must change at those sites

- Target stage/sites: `scheduler_meta_overhead`.
- keep optimistic_read default; minimize refuse_admit idle tax
- avoid wait_for_dependency and ordered_admit theater on cold locations
- SpecFence should match OCC wall (meta overhead is the only gap)

### 4. SpecFence design to approach the bound

- Soft=0 counters: wait_for_dependency=1, wait_for_full_abort=0, refuse_admit=0, partial_abort=0/0, full_abort_reexecute=2, park_resume_full_abort_reexecute=0, optimistic_read=0, ordered_admit=0.
- Stay on **optimistic_read**; drive refuse_admit→0 and wait_for_dependency→0 on cold locs.
- Success metric: SF wall ≤ OCC wall (bound already ≈8× and conflict-free).

---

## Block 12047794 (quiet_parity) — `near_independent`

### 1. Theoretical parallel upper bound

- **n_tx** = 232, gas = 12,486,404, DAG source = `inferred`.
- Conflict DAG: **RAW=0**, **WAW=0**, edges=0.
- Critical path **L=1**; max width **W=232**; indep_frac=1.000.
- Multi-writer locs=0; max writers on one loc=0; max conflict component=1.
- Bound speedup: **∞-cores 232.00×**, **@8 8.00×**.
- Work proxy `sequential` t_work=4.80 ms → ideal@8 **0.60 ms**.
- Measured: sequential=4.80 ms, OCC@1=5.35 ms, OCC@8=**4.26 ms** (1.13× vs work).
- Note: finegrain snapshot absent (WARN sequential fallback); OCC@8 abort_rate=0 → treat as independent DAG

### 2. Why pure OCC falls short of the bound

- Dominant stage: **`scheduler_meta_overhead`**.
- OCC@8 abort_rate=0.0; occ_aborts=0; max_incarnation=0.
- OCC@8 / ideal@8 = **7.10×** (waste factor under equal-cost model).
- Quiet-parity control: OCC@8 abort_rate=0; inferred independent DAG; OCC≈sequential.
- Soft=0 SF/OCC≈0.81 — closest quiet match; residual is meta, not conflict.

### 3. What must change at those sites

- Target stage/sites: `scheduler_meta_overhead`.
- keep optimistic_read default; minimize refuse_admit idle tax
- avoid wait_for_dependency and ordered_admit theater on cold locations
- SpecFence should match OCC wall (meta overhead is the only gap)

### 4. SpecFence design to approach the bound

- Soft=0 counters: wait_for_dependency=0, wait_for_full_abort=0, refuse_admit=20, partial_abort=0/0, full_abort_reexecute=2, park_resume_full_abort_reexecute=0, optimistic_read=0, ordered_admit=0.
- Stay on **optimistic_read**; drive refuse_admit→0 and wait_for_dependency→0 on cold locs.
- Success metric: SF wall ≤ OCC wall (bound already ≈8× and conflict-free).

---

## Cross-block summary

### Top 5 stall patterns (bound → measured gap)

1. **RAW_fan_out_optimistic_read_waste** — blocks [14689597]
   Huge wave width but OCC gets ~0–1× serial: consumers optimistic_read hot storage, validate fails, full_abort_reexecute cascades. Bound ~8× unused.

2. **WAW_spine_serializes_makespan** — blocks [19807137, 6196166, 6137495]
   longest_chain ≈ spine writers; bound speedup only ~1.2–2.2×. OCC still pays abort/reexec on the chain instead of ordered_admit along WAW.

3. **mixed_RAW_WAW_underfilled_waves** — blocks [19606599, 19469097, 8889776]
   Bound 6–7× but OCC ~1–2×: abort_rate 0.2–0.4, max_inc high; handler vs program policy not split.

4. **wait_for_converts_to_full_abort_reexecute** — blocks [14689597, 19807137, 6196166]
   Soft=0: wait_for_dependency parks then park_resume_full_abort_reexecute ≈ parks; partial_abort rare on fan_out.

5. **quiet_meta_and_refuse_admit_tax** — blocks [14396881, 12047794, 2179522]
   DAG near-independent (bound ~8×); OCC often meta-slower than serial; SpecFence refuse_admit/meta can help (2179522) or hurt (14396881).

### TPS bottleneck essence

Across these ten blocks the parallel upper bound is set by the RAW/WAW conflict DAG under fixed commit order: fan-out blocks have short critical paths and huge wave width (bound ≈8×) but pure OCC burns that width on optimistic_read → validate → full_abort_reexecute; spine blocks are already nearly serial by WAW longest_chain (bound ≈1–2×) so TPS cannot be rescued by more cores—only by ordered_admit/dependency-aware admission along the spine. SpecFence Soft=0 approaches the bound only when wait_for_dependency and partial_abort replace late full_abort_reexecute without refuse_admit idle tax on independents.

---

## Invariants

- Soft=0 held on all Soft=0 process traces.
- No plant logic changes on this tip (rename only).
- seq≡par TCB unchanged.
- Upper bound is **structural** (DAG), not a claim that SpecFence currently meets it.
