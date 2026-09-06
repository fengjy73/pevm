# SpecFence — first principles from effect-level RAW traces

**Date:** 2026-09-06 (Asia/Shanghai)  
**Status:** DESIGN — instrumentation + analysis only (no full CC rewrite)  
**Evidence:** `lab/results/effect-raw-journal-stream.*` (Inspector, tip after gross-work pass)  
**Checkout:** branch `specfence`  
**Keeps:** sequential ≡ parallel TCB; production `finegrain_deep` / `finegrain_journal` / inspect **off** by default

---

## 0. Method (what we measure)

1. **Db-hook deep** (`set_finegrain_deep(true)`): `RawEffectEdge` on cold `VmDb::{storage,basic,code_hash}` MV observes.
2. **Journal stream** (`set_finegrain_journal(true)`): opt-in `inspect_run`; Inspector `step` logs every **SLOAD/SSTORE/BALANCE/EXT*/SELFBALANCE** and live **valued-CALL / CREATE / SELFDESTRUCT** account writes:
   - **one edge per cross-tx read instance** (no `(p,c,ℓ)` HashSet dedupe)
   - `producer_effect_k` increments on **every** live write instance (not finalize-only)
   - producer resolve = **location last-writer** prior to the consumer
3. **Depth formulas (document):**
   - `depth_frac_effects` = first_program_cross_k / total_journal_reads
   - `depth_frac_gas` = gas_at_cross / **tx_gas_limit** (legacy; misleads when used ≪ limit)
   - **`depth_frac_gross_work` (preferred)** = gas_at_cross / **tx_gas_used**  
     where `gas_at_cross = tx_gas_limit − remaining` at the discovering opcode (plant TLS carries tx limit)
   - `depth_frac_opcode` = opcode_steps_at_cross / total_opcode_steps
4. **Preferred stream:** OCC@1 (serial-forced G*); OCC@8 optional for timing.
5. **Blocks:** 14689597, 14689599, 19606598, 19606599, 19469096, 19469097.

**Framing:** control-law inputs are **OUR** pevm/revm plant distributions. External RAW tables are definitional references only — **not** calibration targets.

Production default: both flags **off** → `Handler::run`, zero overhead.

---

## 1. EVM billing unit vs CC object vs discovery

| Layer | Unit | Role |
|-------|------|------|
| **Billing / gross-work** | Cumulative **gas used** in the tx (opcode steps secondary) | What redo and wait *cost* |
| **CC object** | Versions on **locations** (`MemoryLocation` → MV Data/Estimate) | What must be ordered |
| **Discovery** | First time a consumer **observes** a prior producer version (location last-writer) | When EV of Wait/Bind/abort becomes knowable |
| **Enforcement** | Bind / WaitHard / SpecRead / validate / ESTIMATE / reexec | How policy acts after discovery |

Final-RW HotSet thresholds see only the **committed container** of locations per tx. They cannot see mid-tx repeats, **gross-work** discovery depth, or producer readiness at that instant.

---

## 2. Trace findings (plant-observed)

### 2.1 RAW counts (OCC@1 journal, excl. beneficiary/lazy)

| Block | journal RAW | prog / hand | final-RW | unique pcl | mean/pcl | journal reads (cross/all) | acct-grain diag |
|-------|------------:|------------:|---------:|-----------:|---------:|--------------------------:|----------------:|
| **14689597** | **647** | 605 / 42 | 449 | 592 | 1.09 | 605 / 2497 | 1073 |
| 19606599 | 584 | 439 / 145 | 42 | — | 1.81 | 471 / 6009 | 909 |
| 19469097 | 410 | 341 / 69 | 28 | — | 1.56 | 355 / 3491 | 567 |
| 19469096 | 232 | 213 / 19 | 6 | — | 1.20 | 213 / 1460 | 311 |
| 19606598 | 44 | 32 / 12 | 3 | — | 1.63 | 32 / 833 | 83 |

**Instance semantics confirmed:** edges are not `(p,c,ℓ)`-deduped. Mean effects/pcl ≈ 1.09 on 597 is **empirical** (contracts rarely re-SLOAD the same foreign slot), not a collector HashSet. Max program fan-out on 597 remains **448** (star).

**Definitional notes (not targets):** other grammars may count account-grain observes (`sload_account_grain_cross`), WAW, LOG/KECCAK, warm journal reads without MV, or aborted-incarnation edges. Those inflate relative to **location last-writer RAW**; we keep primary edges location-correct and report diag counters separately.

### 2.2 First program-cross depth — **gross-work flips gas/limit**

| Block | effect p50 | gas/limit p50 | **gross-work p50** | opcode p50 | frac gw &lt; 1% | frac op &lt; 1% |
|-------|-----------:|--------------:|-------------------:|-----------:|---------------:|---------------:|
| **14689597** | 0.86 | **0.106** | **0.943** | **0.772** | **0.00** | **0.00** |
| 19606599 | 0.36 | 0.232 | **0.381** | 0.205 | 0.00 | 0.00 |
| 19469097 | 0.80 | 0.344 | **0.611** | 0.710 | 0.00 | 0.00 |
| 19469096 | 0.83 | 0.344 | **0.925** | 0.710 | 0.00 | 0.00 |
| 19606598 | 0.36 | 0.318 | **0.539** | 0.282 | 0.00 | 0.00 |

On **14689597**, morphology split:

- **~448 simple fan-out consumers** (7 journal reads, ~40k gas used of ~240k limit): first foreign SLOAD at ~38k gas → **gross-work ≈ 0.94**, opcode ≈ 0.77. Gas/limit ≈ 0.11 was an **artifact of oversized limits**, not early work.
- **~25 heavy consumers** (e.g. tx38-class): gross-work ≈ **0.11**, opcode ≈ **0.05**. Truly early in billed work / steps.

**≪1% gross-work or opcode depth:** **does not appear** in this plant sample (no consumer with gw or opcode depth &lt; 0.01). Min opcode depth ≈ 0.053 (heavy txs).

### 2.3 M-A vs M-D proxies (gross-work preferred)

With `estimate_ma_md` preferring `depth_frac_gross_work`:

| Block | consumers w/ prog cross | redo_saved | wait_added |
|-------|------------------------:|-----------:|-----------:|
| 14689597 | 475 | **~427** | **~108** |
| 19606599 | — | **~38** | **~7** |
| 19469097 | — | **~88** | **~6** |

Revision vs gas/limit-only note: on 597, high `redo_saved` under gross-work reflects the fan-out majority discovering **late** (d≈0.94) — abort-at-discovery has already sunk most billed work. That is the opposite of the gas/limit “early discovery” story.

### 2.4 Effect DAG vs final-RW (journal OCC@1)

| Block | effect program path | final-RW longest | max program fan-out | journal RAW |
|-------|--------------------:|-----------------:|--------------------:|------------:|
| 14689597 | 27 | 29 | **448** | 647 |
| 19606599 | 26 | 57 | 14 | 584 |
| 19469097 | 47 | 47 | 6 | 410 |
| 19469096 | 132 | 132 | 12 | 232 |

597 remains **star/fan-out**; 19469096 remains **long chain**.

---

## 3. What true adaptive means

**Online per-edge EV** at discovery time, using:

1. **depth-into-tx `d` in gross-work** (`gas_at_cross / tx_gas_used`; opcode fraction as secondary),
2. **producer readiness** (Data published / ESTIMATE / residual WŜ / lag),
3. **program vs handler class** at the effect,
4. **cross-block morphology prior** (contiguous segments A/B/C) — fan-out vs chain.

Not: retune `H_w`/`H_a` until a gate barely moves. Not: fit counts to an external table.

---

## 4. Control law sketch (formulas)

For edge \(e=(p\to c,\ell)\) discovered at consumer gross-work depth \(d\in[0,1]\):

\[
\begin{aligned}
W_{\mathrm{redo}}^{\mathrm{MA}} &= 1 \\
W_{\mathrm{redo}}(d) &= (1-d)\cdot \hat w_c \\
W_{\mathrm{wait}} &= \hat t_{\mathrm{ready}}(p,\ell)\cdot (1-\sigma) \\
\mathrm{EV}[\mathrm{Wait}] &= -W_{\mathrm{wait}} \\
\mathrm{EV}[\mathrm{SpecRead}] &= -\,P(\mathrm{stale}\mid e)\cdot W_{\mathrm{redo}}(d) \\
\mathrm{EV}[\mathrm{Bind}] &= 0 \quad \text{if Data}(p,\ell)\ \mathrm{published} \\
\mathrm{EV}[\mathrm{EarlyAbort}] &= -d\cdot\hat w_c - W_{\mathrm{redo}}(d')\quad\text{(only if stale already known)}
\end{aligned}
\]

**With plant gross-work on 597:**

- **Fan-out majority (d≈0.94):** EarlyAbort sunk cost is already ~94% of billed work; residual redo small. Prefer **Wait/Bind** on the hot program ℓ. Gas/limit-based EarlyAbort optimism is **withdrawn**.
- **Heavy minority (d≈0.11):** EarlyAbort / SpecRead residual still large unless Bind-when-ready; Wait/Bind remain the fan-out lever for the shared hot location.
- **≪1% class:** not observed → do not design the default law around abort-at-1%-work.

**Morphology switch:** fan-out storm → Wait/Bind first program writers; long WAW chain → schedule/steal over HotLocal chatter.

---

## 5. Intra-block learning vs inter-block prior

**Intra-block:** update per-ℓ / per-edge posteriors as effects appear; decisions edge-local; never sticky block-wide Wait bit.

**Inter-block (contiguous):** carry `{program_frac, handler_frac, expected_fanout, expected_program_path, abort_rate, gross_work_depth_prior, opcode_depth_prior}`. Decay on morphology flip.

---

## 6. Instrumentation status

| Item | Status |
|------|--------|
| Mid-tx gas at first program cross | **Done** (journal) |
| Gross-work depth = gas_at_cross / tx_gas_used | **Done** (preferred) |
| Opcode-step depth | **Done** |
| Inspector SLOAD/SSTORE (+ BALANCE/EXT*) | **Done** |
| Live write ordinal every SSTORE + valued CALL/CREATE/SELFDESTRUCT | **Done** |
| Instance edges (no pcl dedupe) | **Done** (empirical mean/pcl≈1.09) |
| Account-grain / other grammars | **Diag only** (`stream_diag`) — not primary RAW |
| Handler attribution beyond Basic/CodeHash | Partial (tx-env / precompile still open) |
| Production flag-off path | Unchanged |

Research: `Pevm::set_finegrain_journal(true)` (implies deep + inspect). Production default unchanged.

---

## 7. Redesign implications (updated)

1. Plant journal RAW on 597 is **647** location last-writer instances; mean/pcl≈1.09 is real repeat rarity, not dedupe.
2. **Gross-work p50≈0.94 vs gas/limit p50≈0.11 on 597** — control law **must** use gross-work (or opcode), not gas/limit.
3. No ≪1% gross-work/opcode discovery in sample; EarlyAbort is not “abort at 1% work” on this plant.
4. Fan-out 597: Wait/Bind hot ℓ primary; EarlyAbort EV is **weak** for the simple-consumer majority under true gross-work.
5. Heavy-tx minority still sees early gross-work (~0.11) / opcode (~0.05) — morphology-conditioned policy, not one scalar.
6. Program vs handler split remains material on B/C blocks.
7. Contiguous priors should carry **gross_work_depth_prior** + **opcode_depth_prior** + morphology.
8. Do **not** full SpecFence CC rewrite yet — next is wire gross-work `d` into `choose_action` sketch only.
9. Architecture v2/v3 edge-local EV thesis holds; this note **constrains** depths with measured gross-work.
10. Flag-off path: seq≡par TCB unchanged.

---

## 8. Pointer / supersession

- **Supersedes:** readings that treat gas/limit depth as “early discovery” on 597; any note that calibrates plant RAW to an external count table.
- **Does not replace:** TCB seq≡par; LeanOCC default; location as concurrency object.
- **Architecture sketch:** see `specfence-cc-architecture-v3.md` § gross-work addendum.
- **Next (not this task):** optional `choose_action` prototype using `depth_frac_gross_work` — no full plant rewrite.
