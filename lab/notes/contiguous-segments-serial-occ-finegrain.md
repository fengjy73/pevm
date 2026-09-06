# Contiguous segments — serial + OCC fine-grain (RAW program vs handler)

**Date:** 2026-09-06 (Asia/Shanghai)  
**Checkout:** `/workspace/specfence` branch `specfence`  
**RPC:** Alchemy ETH RPC from `~/.config/ethereum` (via `pevm-fetch`)  
**Tooling:** `cargo run -p pevm --release --config 'profile.release.lto=false' --example specfence_contiguous_segments_analysis`  
**Artifacts:** `lab/results/contiguous-segments-finegrain.{json,csv}` + deep `…-b{14689597,19606599,19469097}.json`  
**Constraints:** production inspect tax **off**; `finegrain_trace` opt-in on OCC only; seq≡par unchanged.

---

## Method

1. **Serial** + **OCC@1** + **OCC@8** wall time / TPS / abort / `evm_entries`.
2. **FineGrainCollector** final RW → effective DAG (exclude beneficiary + `basic_lazy`).
3. **RAW edges** `(producer_tx, consumer_tx, location, kind)` with class:
   - **program** = `storage` / `code_hash` / `selfdestruct`
   - **handler** = `basic` / unknown (account-basic paths)
4. **Proxies** (no inspect / no gas-depth): `producer_lag = c−p`, `cross_tx_read_frac`, program-RAW longest path. True early-read gas depth needs research inspect flag — not enabled here.
5. Low-gas neighbors (`gas_used < 4M`) force parallel OCC via header `gas_used` bump in the **example only** so FineGrainCollector still runs.

### Fetched blocks (n_tx)

| Segment | Blocks | Core n_tx check |
|---------|--------|-----------------|
| **A** | 14689595–99 | **14689597 = 564** ✓ |
| **B** | 19606597–600 | **19606599 = 367** ✓ |
| **C** | 19469096–99 | **19469097 = 336** ✓ |

---

## Headline table (all blocks)

| seg | block | n_tx | OCC@8 TPS | abort | reexec | longest | RAW | prog | hand | prog_chain | mean_lag |
|-----|-------|------|-----------|-------|--------|---------|-----|------|------|------------|----------|
| A | 14689595 | 23 | 17k | 0.35 | 0.36 | 5 | 10 | 10 | 0 | 2 | 11.7 |
| A | 14689596 | 31 | 19k | 0.52 | 0.57 | 10 | 1 | 1 | 0 | 2 | 1.0 |
| A | **14689597** | **564** | **121k** | **0.11** | **0.65** | **29** | **449** | **449** | **0** | **2** | **270** |
| A | 14689598 | 111 | 51k | 0.06 | 0.15 | 8 | 7 | 6 | 1 | 2 | 25.3 |
| A | 14689599 | 43 | 81k | 0.00 | 0.02 | 2 | 0 | 0 | 0 | 0 | — |
| B | 19606597 | 124 | 16k | 0.36 | 0.65 | 32 | 11 | 1 | 10 | 2 | 5.0 |
| B | **19606598** | **91** | **63k** | **0.11** | **0.24** | **6** | **3** | **0** | **3** | **1** | **31** |
| B | **19606599** | **367** | **26k** | **0.24** | **0.46** | **57** | **42** | **22** | **20** | **2** | **79** |
| B | 19606600 | 237 | 29k | 0.25 | 0.97 | 47 | 26 | 8 | 18 | 2 | 15.3 |
| C | **19469096** | **250** | **78k** | **0.48** | **0.65** | **132** | **6** | **2** | **4** | **2** | **63** |
| C | **19469097** | **336** | **55k** | **0.44** | **0.50** | **47** | **28** | **16** | **12** | **4** | **83** |
| C | 19469098 | 268 | 74k | 0.25 | 0.38 | 26 | 5 | 2 | 3 | 2 | 35.2 |
| C | 19469099 | 257 | 51k | 0.23 | 0.38 | 29 | 14 | 6 | 8 | 2 | 27.3 |

Effective DAG: WAW often dominates longest-chain (esp. 19469096). Program-RAW longest path stays short (≤4) — morphology is often **fan-out / multi-writer**, not a single deep program path.

---

## Research Q1 — 14689597: when deps appear / early discovery redo savings

**Morphology:** Pure **program** storm. 449/449 RAW are `storage`. Top multi-writer: one storage location with **26 writers (tx 0…38) and 474 readers**. Conflict component **475**. Effective longest chain **29** but program-RAW path length only **2** → star/fan-out from early writers, not a deep pipeline.

**When deps appear (schedule proxies):**

| Proxy | Value |
|-------|-------|
| First program-RAW consumer `tx_idx` | **39** (schedule frac **0.069**) |
| That consumer `min_producer_lag` | **1** (reads immediately prior writer) |
| That consumer final incarnation | **7** (heavy redo) |
| Program consumers in first 10% of txs | only **3** |
| Mean / median / p90 producer lag | **270 / 267 / 472** |
| `frac_txs_with_program_raw` | **0.80** |
| Incarnation hist | 96@0, then mass at 1–3; max_inc=8; **468/564** reincarnated |

**Early discovery redo savings (inference):** Deps are **discoverable as soon as early writers (0…38) publish** — the fan-out hits ~80% of txs. OCC@8 still gets high TPS (~121k) because steal keeps cores busy, but **`reexec_frac≈0.65`** (`evm_entries=1603` vs `n_tx=564`) is pure interpreter waste. A learner that **Binds/Waits on program RAW once first early writers land** (instead of SpecRead until abort×N) targets exactly this waste. Fixed `H_w=8` would eventually HotSet the location, but only after 8 writers — here writers 0…7 already create stale reads for later consumers; **edge-local EV from first RAW observation** beats a dead writer-count threshold.

**Neighbors:** 14689595–96 tiny blocks with sparse RAW; 14689598 collapses to 7 RAW / abort 0.06; 14689599 essentially independent. Contiguous prior must **not** sticky-bit “hot” from 597 into 598–599.

---

## Research Q2 — 19606599: Cancun program vs handler vs schedule; contrast 19606598

### Core 19606599

| Slice | Count / note |
|-------|----------------|
| RAW total | 42 |
| Program (`storage`) | **22** — mean lag **140**, med **183** (long-horizon) |
| Handler (`basic`) | **20** — mean lag **13**, med **4** (short) |
| Effective longest | **57** (WAW+RAW mix) |
| Hot locations | basic **54 writers / 74 readers**; storage chains 7-wide |
| OCC@8 | abort **0.24**, reexec **0.46**, max_inc **9**, TPS ~26k |

**Interpretation:** Cancun block mixes (1) **program storage RAW** with long producer lag (Bind/Wait candidates on critical path) and (2) **handler/basic chatter** with short lag (often cheaper to SpecRead+cheap abort than Wait). Schedule cost shows up as multi-writer **basic** mass + moderate abort — not a single storage star like 14689597. Adaptive policy must **split class**: program → EV Wait/Bind; handler → lean SpecRead unless abort@ℓ spikes.

### Contrast 19606598 (both-redo-and-wait improve)

| | 19606598 | 19606599 |
|--|----------|----------|
| n_tx | 91 | 367 |
| indep_frac | **0.80** | 0.64 |
| RAW | **3** (all handler) | 42 (22 prog / 20 hand) |
| program RAW | **0** | 22 |
| abort@8 | **0.11** | 0.24 |
| reexec | **0.24** | 0.46 |
| longest | **6** | 57 |
| OCC@8 TPS | **63k** | 26k |

598 is **wide/lean**: almost no program deps. Both “always SpecRead (redo)” and “occasional Wait” are cheap because there is little true RAW critical path — hence **both redo and wait improve** relative to a heavy plant. 599 suddenly adds program mass + handler mass; a sticky HotSet / conflict bit carried from 597→598 would tax the quiet block, and a cold start on 599 without contiguous morphology prior misses the program/handler split.

Transition 597→598: abort **down** (−0.24), RAW down — morphology flip. 598→599: abort **up**, +22 program RAW — prior from 598 alone under-prepares 599; need **segment-level** program-vs-handler prior, not last-block sticky bit.

---

## Research Q3 — 19469097: long chains vs redo; contrast 19469096

### Core 19469097

| Metric | Value |
|--------|-------|
| longest effective chain | **47** |
| **program-RAW path** | **4** (max in this dataset) |
| RAW | 28 (16 prog / 12 hand) |
| abort / reexec | **0.44 / 0.50** |
| Top MW | storage **47 writers** (tx 43…142); storage 38-wide; basic 33w/46r |
| First prog consumer | tx **2** (frac 0.006), lag 1 |

Longer **program** path (4) + many reincarnations (124 txs) → redo cost is real, but still WAW-heavy. Program edges are the Bind/Wait leverage; handler short-lag edges less so.

### Contrast 19469096 (long path / fewer txs)

| | 19469096 | 19469097 |
|--|----------|----------|
| n_tx | **250** | 336 |
| longest chain | **132** | 47 |
| RAW | **6** | 28 |
| program RAW | 2 | 16 |
| WAW | **172** | 183 |
| abort@8 | **0.48** | 0.44 |
| reexec | **0.65** | 0.50 |
| max_conflict_component | **132** | 51 |

**19469096** is a **WAW-serialized** block: huge longest chain / component with **almost no RAW**. Waiting on reads does little — the critical path is write–write ordering. OCC still posts high TPS (~78k) via ESTIMATE cascades, but SpecFence Wait/Bind aimed at RAW cannot shorten a WAW spine. **19469097** has shorter chain but **more program RAW** → better target for edge-local Bind; redo fraction still ~0.5.

Lesson for v2: longest-chain alone is a bad control signal; **RAW class + program path** matter. H_w on any multi-writer location would HotSet WAW-only basics/storage and invite WaitHard with no Bind payoff.

---

## Per-segment summary

| Seg | mean n_tx | mean RAW | mean prog/hand | mean longest | max prog_chain | Notable transitions |
|-----|-----------|----------|----------------|--------------|----------------|---------------------|
| A | 154 | 93 | 93 / 0 | 11 | 2 | 596→597: +533 tx, +448 program RAW; 597→598 collapse |
| B | 205 | 21 | 8 / 13 | 36 | 2 | 597→598 quiet; 598→599 program appears |
| C | 278 | 13 | 7 / 7 | 59 | **4** | 696→697: longest −85 but RAW +22; WAW→RAW mix shift |

---

## Implications for Adaptive Learning Architecture v2

1. **Reject H_w/H_a as primary control** — 14689597 needs action at writer 1…, not after 8; 19469096 multi-writer WAW would trip HotSet with no RAW Wait EV.
2. **Per-edge EV** using class, lag, chain depth — program long-lag ≠ handler short-lag.
3. **Contiguous morphology prior** across A/B/C — carry program/handler mix, not a sticky block conflict bit (598 vs 599).
4. **Early discovery** on fan-out blocks (597): Bind/Wait as soon as early program writers publish → cut `reexec_frac`.
5. Keep inspect **off** by default; collection hooks (this PR) feed the learner offline first.

See `lab/notes/specfence-adaptive-learning-architecture-v2.md`.
