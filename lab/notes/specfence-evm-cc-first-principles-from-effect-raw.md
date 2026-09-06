# SpecFence — first principles from effect-level RAW traces

**Date:** 2026-09-06 (Asia/Shanghai)  
**Status:** DESIGN — instrumentation + analysis only (no full CC rewrite)  
**Evidence:** `lab/results/effect-raw-deep.{json,csv}` + per-block deep JSON  
**Checkout:** branch `specfence` (deep FineGrain mode)  
**Supersedes for control-law intent:** Adaptive Learning Architecture v2 HotSet-as-feature framing where it conflicts; see §7 pointer  
**Keeps:** sequential ≡ parallel TCB; production `finegrain_deep` / inspect **off** by default

---

## 0. What we measured (method)

1. **Deep FineGrain mode** (opt-in): on every `VmDb::{storage,basic,code_hash}` call that observes `ReadOrigin::MvMemory(p)`, emit  
   `RawEffectEdge { producer_tx, producer_effect_k, consumer_tx, consumer_effect_k, location, class, incarnation }`.
2. **Write ordinal `k`:** index in producer’s finalize `write_set` (not live SSTORE stream — see §6 gap).
3. **Depth:** `depth_frac_effects = first_program_cross_k / total_db_effects` for that incarnation.
4. **Preferred stream:** OCC@1 (serial-forced parallel path) for clean G*; also OCC@8 for abort/incarnation correlation.
5. **Blocks:** contiguous segments A/B/C (13 blocks), deep-dive cores 14689597 / 19606599 / 19469097 (+ neighbors).

---

## 1. EVM billing unit vs CC object vs discovery

| Layer | Unit | Role |
|-------|------|------|
| **Billing / work** | Interpreter work (gas / opcode steps / DB effects as proxy) | What redo and wait *cost* |
| **CC object** | Versions on **locations** (`MemoryLocation` → MV Data/Estimate) | What must be ordered |
| **Discovery** | First time a consumer **observes** a prior producer version (or ESTIMATE) | When EV of Wait/Bind/abort becomes knowable |
| **Enforcement** | Bind / WaitHard / SpecRead / validate / ESTIMATE / reexec | How policy acts after discovery |

Final-RW HotSet thresholds see only the **committed container** of locations per tx. They cannot see:

- how many times a location was read mid-tx (journal may cache after first DB load),
- **when** in the consumer’s work the first cross-tx program read occurred,
- producer readiness at that instant,
- program vs handler at **effect** grain (only final location kind).

So HotSet `H_w`/`H_a` are **membership features**, not an EV control law.

---

## 2. Trace findings (constrained)

### 2.1 RAW counts vs user table (14689597)

| Source | Count | Notes |
|--------|------:|-------|
| User ground truth (producer-effect → consumer-read) | **≈3802** | Mid-tx effect stream |
| Prior FineGrain final-RW RAW | **449** | Unique (p,c,ℓ) from final sets |
| Deep effect-RAW (OCC@1, excl. beneficiary/lazy) | **593** (551 program / 42 handler) | One DB observe per (p,c,ℓ) typical |
| Unique (p,c,ℓ) among effect edges | **593** | `mean_effects/pcl ≈ 1.0` |

**Honest gap:** we are still **~6× below** user ≈3802. Dominant reason: **revm journal caches storage after the first `Database::storage` load** — subsequent SLOADs to the same slot do **not** re-enter pevm’s Db hook. User-level effect records count interpreter SLOADs / journal effects; we count **cold Db observations**. Additional gaps: `producer_effect_k` is finalize `write_set` order (not live SSTORE ordinal); no true mid-tx gas on the default path.

### 2.2 First program cross-tx depth (DB-effect fraction)

| Block | p10 | p50 | p90 | frac depth < 1% |
|-------|----:|----:|----:|----------------:|
| **14689597** | 0.86 | **0.86** | 0.86 | **0.00** |
| 19606599 | ~ | **0.59** | ~ | 0.00 |
| 19469097 | ~ | **0.78** | ~ | 0.00 |

On 14689597 the mass sits at **≈6/7**: EVM preamble (account basics / code) then first cross-tx **storage** observe. **No sample shows ≪1% on the DB-effect proxy.** Gas-fraction depth may still be small if preamble gas ≪ body gas — **that requires inspect mid-tx gas** (§6). Do not claim early-gas discovery from this proxy alone.

### 2.3 M-A vs M-D proxies (normalized consumer-work units)

Definitions used offline:

- **M-A:** always SpecRead; on conflict pay **full** consumer work `= 1` per consumer with a program cross.
- **M-D:** discovery at depth `d` → redo `∝ (1−d)`; wait proxy `∑ lag_norm·(1−steal)` over unique program pairs (`steal=0.5`).

| Block | consumers w/ prog cross | M-A redo | M-D redo | **redo_saved** | **wait_added** |
|-------|------------------------:|---------:|---------:|---------------:|---------------:|
| 14689597 | 476 | 476 | ~85 | **391** | **108** |
| 19606599 | — | — | — | **25.5** | **2.8** |
| 19469097 | — | — | — | **78.9** | **4.0** |

Interpretation: with **late** DB-effect `d≈0.86`, “early abort at discovery” still “saves” large `sum(d)` on paper only if one confuses saved-prefix with saved-suffix. **Operational EV for abort is `(1−d)·work`**, which is **small** when `d` is high. Wait/Bind on the **hot program location once early writers publish** is the lever that kills the 597 fan-out storm — not threshold HotSet after 8 writers.

### 2.4 Effect DAG vs final-RW

| Block | effect program path | final-RW longest | max program fan-out (tx) |
|-------|--------------------:|-----------------:|-------------------------:|
| 14689597 | 27 | 29 | **448** |
| 19606599 | 7 | 57 | 3 |
| 19469097 | 47 | 47 | 4 |
| 19469096 | 132 | 132 | 2 |

597 remains a **star/fan-out** morphology (fan-out ≫ path). 19469096/97 are **long chain** heavy. Control law must branch on morphology, not one HotSet bit.

---

## 3. What true adaptive means

**Online per-edge EV** at discovery time, using:

1. **depth-into-tx** `d` (prefer gas; DB-effect is a weak proxy),
2. **producer readiness** (Data published / ESTIMATE / residual WŜ / lag),
3. **program vs handler class** at the effect,
4. **cross-block morphology prior** (contiguous segments A/B/C).

Not: retune `H_w`/`H_a` until a gate barely moves.

---

## 4. Control law sketch (formulas)

For edge \(e=(p\to c,\ell)\) discovered at consumer depth \(d\in[0,1]\):

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

with steal credit \(\sigma\in[0,1]\), \(\hat w_c\) remaining interpreter work, \(\hat t_{\mathrm{ready}}\) predicted producer publish delay.

**Policy:** \(\arg\max\{\mathrm{EV}[\mathrm{Bind}],\mathrm{EV}[\mathrm{Wait}],\mathrm{EV}[\mathrm{SpecRead}]\}\); escalate EarlyAbort only when stale is known **and** \(d\) small in **gas**.

**Class prior:** program edges → higher Wait/Bind prior; handler/basic → SpecRead unless abort@ℓ spikes.

**Morphology switch:** if prior says fan-out storm (597-like), bias Wait/Bind on first program writers; if long WAW chain (19469096-like), bias schedule/steal over HotLocal chatter.

---

## 5. Intra-block learning vs inter-block prior

**Intra-block:** update per-ℓ / per-edge posteriors as effects appear; decisions are edge-local; never sticky block-wide Wait bit.

**Inter-block (contiguous):** carry compact prior `{program_frac, handler_frac, expected_fanout, expected_program_path, abort_rate}`. Warm-start priors at block begin; **decay on morphology flip** (598 lean ↔ 599 mixed). Segments A/B/C are the training substrate.

---

## 6. Instrumentation still missing

| Missing | Why it matters |
|---------|----------------|
| **True mid-tx gas** at first stale/cross read | DB-effect `d` ≠ gas `d`; early-abort EV needs gas |
| **Inspector SLOAD/SSTORE stream** | Journal-cached repeats invisible to `Database` hooks → gap vs ≈3802 |
| **Live write ordinal** at SSTORE (not finalize write_set) | Producer `k` alignment with user effect records |
| **Handler attribution** beyond Basic/CodeHash | tx-env / precompile touches |
| **Validated G\* only vs speculative observes** | OCC@8 mixes abort incarnations; filter policy for learning |

Research flag path: `Pevm::set_finegrain_deep(true)` (+ optional `SPECFENCE_ENABLE_INSPECT=1` later). Production default unchanged.

---

## 7. Redesign implications (8–12 bullets)

1. Final-RW RAW (449) understates effect dependency mass; even Db-level deep (593) ≪ user effect stream (≈3802) because of journal caching.
2. HotSet writer-count thresholds cannot encode discovery depth or per-edge EV.
3. 14689597 is fan-out (fan-out≈448), not a deep program pipeline — Wait/Bind the hot ℓ early.
4. DB-effect depth p50≈0.86 on 597 ⇒ abort-at-discovery saves little **suffix** unless gas depth is much earlier — measure gas next.
5. M-A vs M-D: Wait/Bind economics dominate fan-out storms; “early abort” needs early **gas** discovery.
6. Program vs handler must split policy (599: 105 prog / 113 hand effect edges).
7. Contiguous priors must track morphology flips (598 vs 599), not sticky conflict bits.
8. Effect-edge longest path can match final-RW chain on chain-heavy blocks (19469096) while fan-out blocks need fan-out features.
9. OCC@8 deep edges > OCC@1 (abort incarnations) — useful for correlating stale discovery incarnation, not as G\*.
10. Control law = online \(\arg\max EV\) over Bind/Wait/SpecRead with class + depth + readiness + prior.
11. Do **not** rewrite full SpecFence plant yet — close gas/inspect instrumentation gap first.
12. Architecture v2 remains valid on “edge-local EV” thesis; this note **constrains** it with measured depths and the Db-vs-interpreter gap.

---

## 8. Pointer / supersession

- **Supersedes:** any reading of Adaptive Learning Architecture v2 that treats HotSet thresholds as the primary control law.
- **Does not replace:** TCB seq≡par; LeanOCC default; location as concurrency object.
- **Next code (not this task):** inspect-backed SLOAD/SSTORE effect stream + mid-tx gas into `RawEffectEdge.gas_used_so_far`.
