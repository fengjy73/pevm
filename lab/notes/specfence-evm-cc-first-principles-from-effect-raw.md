# SpecFence — first principles from effect-level RAW traces

**Date:** 2026-09-06 (Asia/Shanghai)  
**Status:** DESIGN — instrumentation + analysis only (no full CC rewrite)  
**Evidence:** `lab/results/effect-raw-deep.*` (Db-hook) + `lab/results/effect-raw-journal-stream.*` (Inspector)  
**Checkout:** branch `specfence` (journal FineGrain mode)  
**Supersedes for control-law intent:** Adaptive Learning Architecture v2 HotSet-as-feature framing where it conflicts; see §7 pointer  
**Keeps:** sequential ≡ parallel TCB; production `finegrain_deep` / `finegrain_journal` / inspect **off** by default

---

## 0. What we measured (method)

1. **Db-hook deep** (`set_finegrain_deep(true)`): `RawEffectEdge` on cold `VmDb::{storage,basic,code_hash}` MV observes.
2. **Journal stream** (`set_finegrain_journal(true)`): opt-in `inspect_run`; Inspector `step` logs every **SLOAD/SSTORE/BALANCE/EXT*/SELFBALANCE** with:
   - `consumer_tx`, `gas_used_so_far` (= tx_gas_limit − remaining), opcode steps
   - on read: producer from live `last_writer` map (+ Db Basic/CodeHash still hooked)
   - on write: **live** SSTORE ordinal (not finalize `write_set` index)
3. **Depth:**
   - `depth_frac_effects` = first_program_cross_k / total_journal_or_db reads
   - `depth_frac_gas` = gas_used_at_first_program_cross / tx_gas_limit (**preferred for EV**)
4. **Preferred stream:** OCC@1 (serial-forced G*); OCC@8 optional for discovery timing.
5. **Blocks (journal recollect):** 14689597, 14689599, 19606598, 19606599, 19469096, 19469097.

Production default: both flags **off** → `Handler::run`, zero overhead.

---

## 1. EVM billing unit vs CC object vs discovery

| Layer | Unit | Role |
|-------|------|------|
| **Billing / work** | Interpreter **gas** (opcode steps as secondary) | What redo and wait *cost* |
| **CC object** | Versions on **locations** (`MemoryLocation` → MV Data/Estimate) | What must be ordered |
| **Discovery** | First time a consumer **observes** a prior producer version (or ESTIMATE) | When EV of Wait/Bind/abort becomes knowable |
| **Enforcement** | Bind / WaitHard / SpecRead / validate / ESTIMATE / reexec | How policy acts after discovery |

Final-RW HotSet thresholds see only the **committed container** of locations per tx. They cannot see mid-tx repeats, **gas** discovery depth, or producer readiness at that instant.

---

## 2. Trace findings

### 2.1 RAW counts vs user table (14689597)

| Source | Count | Notes |
|--------|------:|-------|
| User ground truth (producer-effect → consumer-read) | **≈3802** | Mid-tx effect stream |
| Prior FineGrain final-RW RAW | **449** | Unique (p,c,ℓ) from final sets |
| Db-hook deep (OCC@1, excl. beneficiary/lazy) | **593** (551 prog / 42 hand) | Cold Db observe ≈1 per pcl |
| **Journal stream (OCC@1, excl. lazy)** | **647** (605 prog / 42 hand) | SLOAD stream + Basic Db; mean_effects/pcl≈**1.09** |

**Honest before/after undercount:** journal/Inspector closed only a **small** gap (593→647, ~+9%). Still **~5.9× below** user ≈3802. Dominant remaining causes (not journal-cache alone):

1. **Repeat RAW on same (p,c,ℓ) is rare** on this block (max repeats≈2; mean/pcl≈1.09) — journal-cached SLOAD repeats do fire, but seldom against a prior producer on the same slot.
2. User table may use a **different effect identity** (global serial effect log, WAW+RAW, account-touch producers, alternate location hashing).
3. Live SSTORE ordinals help producer_k fidelity but do not multiply consumer edges without multi-read RAW.

Do **not** claim the undercount is closed. Instrumentation is now at the right layer for gas depth; count gap needs definitional alignment with the user’s producer-effect grammar.

### 2.2 First program cross-tx depth — **gas flips the story**

| Block | effect depth p50 | **gas depth p50** | frac gas depth < 1% | frac effect depth < 1% |
|-------|-----------------:|------------------:|--------------------:|-----------------------:|
| **14689597** | **0.86** | **0.106** | **0.00** | 0.00 |
| 19606599 | 0.36 | **0.232** | 0.00 | 0.00 |
| 19469097 | 0.80 | **0.344** | 0.00 | 0.00 |
| 19606598 | 0.36 | **0.318** | 0.00 | 0.00 |
| 19469096 | 0.83 | **0.344** | 0.00 | 0.00 |

On 14689597, DB/journal-**effect** depth sat at ≈6/7 (preamble basics then first storage RAW). **Gas** depth p50≈**0.11** (p10≈0.10, p90≈0.16; min≈0.063). So discovery is **late in effect ordinals** but **early in billed work**.

**≪1% gas depth:** **does not appear** in this sample (no consumer with gas depth < 0.01). Early-abort EV is better than the Db-effect proxy suggested, but not “abort at 1% gas”.

### 2.3 M-A vs M-D proxies (gas-based work)

With gas depth preferred in `estimate_ma_md`:

| Block | consumers w/ prog cross | M-A redo | M-D redo (∝1−d_gas) | **redo_saved** | **wait_added** |
|-------|------------------------:|---------:|--------------------:|---------------:|---------------:|
| 14689597 | 475 | 475 | ~418 | **~57** | **~108** |
| 19606599 | — | — | — | **~24** | **~7** |
| 19469097 | — | — | — | **~50** | **~6** |

Interpretation revision vs Db-only note:

- Under **effect** d≈0.86, abort-at-discovery looked almost free on “saved prefix” but expensive on residual suffix — wait still dominated fan-out.
- Under **gas** d≈0.11, **EarlyAbort / SpecRead residual redo (1−d)≈0.89** is still large; Wait/Bind on the hot program ℓ remains the fan-out lever. Gas-early discovery **raises** the option value of **Bind-when-ready** and selective early abort vs the Db-proxy story, but does **not** make HotSet thresholds sufficient.
- Wait_added still large on 597 (fan-out morphology).

### 2.4 Effect DAG vs final-RW (journal OCC@1)

| Block | effect program path | final-RW longest | max program fan-out (tx) | journal RAW |
|-------|--------------------:|-----------------:|-------------------------:|------------:|
| 14689597 | 27 | 29 | **448** | 647 |
| 19606599 | 26 | 57 | 14 | 555 |
| 19469097 | 47 | 47 | 6 | 400 |
| 19469096 | 132 | 132 | 12 | 232 |

597 remains **star/fan-out**; 19469096 remains **long chain**.

---

## 3. What true adaptive means

**Online per-edge EV** at discovery time, using:

1. **depth-into-tx `d` in gas** (effect ordinal is a weak proxy — now measured),
2. **producer readiness** (Data published / ESTIMATE / residual WŜ / lag),
3. **program vs handler class** at the effect,
4. **cross-block morphology prior** (contiguous segments A/B/C).

Not: retune `H_w`/`H_a` until a gate barely moves.

---

## 4. Control law sketch (formulas)

For edge \(e=(p\to c,\ell)\) discovered at consumer gas-depth \(d\in[0,1]\):

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

**Revised with gas d≈0.11 on 597:** EarlyAbort sunk cost \(d\cdot\hat w_c\) is small (~10% gas), but residual redo \(W_{\mathrm{redo}}(d)\) remains ~90% unless Bind/Wait prevents stale. Prefer **Bind if Data ready**, else **Wait** on hot program fan-out ℓ; SpecRead only when \(P(\mathrm{stale})\) low or wait tax dominates. Escalate EarlyAbort when stale known **and** gas \(d\) small **and** residual writers are ESTIMATE-heavy.

**Morphology switch:** fan-out storm → Wait/Bind first program writers; long WAW chain → schedule/steal over HotLocal chatter.

---

## 5. Intra-block learning vs inter-block prior

**Intra-block:** update per-ℓ / per-edge posteriors as effects appear; decisions edge-local; never sticky block-wide Wait bit.

**Inter-block (contiguous):** carry `{program_frac, handler_frac, expected_fanout, expected_program_path, abort_rate, gas_depth_prior}`. Decay on morphology flip.

---

## 6. Instrumentation status

| Item | Status |
|------|--------|
| True mid-tx **gas** at first program cross | **Done** (journal stream) |
| Inspector SLOAD/SSTORE (+ BALANCE/EXT*) | **Done** (opt-in `set_finegrain_journal`) |
| Live write ordinal at SSTORE | **Done** |
| Close count gap vs user ≈3802 | **Not closed** (~647); needs definitional alignment |
| Handler attribution beyond Basic/CodeHash | Partial (tx-env / precompile still open) |
| Validated G\* vs speculative OCC@8 | OCC@1 preferred; OCC@8 collected for timing |

Research: `Pevm::set_finegrain_journal(true)` (implies deep + inspect). Production default unchanged.

---

## 7. Redesign implications (updated)

1. Final-RW RAW (449) ≪ Db-deep (593) ≪ user (≈3802); journal (647) only slightly above Db-deep — **count undercount is not primarily journal-cache on 597**.
2. **Gas depth p50≈0.11 vs effect depth p50≈0.86 on 597** — control law must use **gas**, not effect ordinal.
3. No ≪1% gas discovery in sample; EarlyAbort is “early-ish” not “immediate”.
4. Fan-out 597: Wait/Bind hot ℓ still primary; gas-early discovery improves Bind-when-ready EV, not HotSet thresholds.
5. M-A vs M-D with gas: redo_saved shrinks vs effect-proxy fantasy; wait_added still large on fan-out.
6. Program vs handler split remains (599 journal: 439 prog / 116 hand filtered).
7. Contiguous priors should carry **gas_depth_prior** + morphology.
8. Do **not** full SpecFence CC rewrite yet — next is definitional RAW alignment + wire gas-d into choose_action sketch only.
9. Architecture v2/v3 edge-local EV thesis holds; this note **constrains** depths with measured gas.
10. Flag-off path: seq≡par TCB unchanged (inspect only when journal/research inspect on).

---

## 8. Pointer / supersession

- **Supersedes:** readings of Adaptive Learning Architecture v2 that treat HotSet thresholds as the primary control law; Db-effect depth conclusions in the prior revision of this note.
- **Does not replace:** TCB seq≡par; LeanOCC default; location as concurrency object.
- **Architecture sketch:** see `specfence-cc-architecture-v3.md` § gas-depth addendum (Wait vs early-abort EV).
- **Next (not this task):** align RAW grammar with user table; optional `choose_action` prototype using `depth_frac_gas` — no full plant rewrite.
