# Block content anatomy + how SpecFence should adapt

**Date:** 2026-09-07 (Asia/Shanghai)  
**Status:** ANALYSIS + DESIGN (corrects “intra vs inter either/or”)  
**Evidence:** `lab/results/l1l2-b*.json`, `l1l2-summary.json`, `contiguous-segments-finegrain.json`, L3/L4 reports  
**Tip context:** methodology tip `0b4affc`

---

## 0. Correction: learning is dual-horizon, not either/or

L4 said **sticky t−1→t feature copy loses to a global prior** (err 236 vs 162). That rejects one *mechanism* (copy last block’s Wait/morph as hard policy). It does **not** reject **cross-block learning**.

Correct frame:

| Horizon | Role | Must exist? |
|---------|------|-------------|
| **Intra-block** | Online discovery → update edge EV, fan-out, class mix, abort@ℓ as the block runs | Yes (primary control) |
| **Inter-block** | Hierarchical / Bayesian **warm-start** of morphology + per-ℓ / class priors; **decay on flip** | Yes (prior, not gate) |

Both feed the **same** `choose_action(e, d, class, producer_status, prior)`. Inter never alone decides WaitHard; intra never starts from a blank slate if a prior exists.

---

## 1. What these blocks actually are (content, not just RAW counts)

### 1.1 Segment A around 14689597 — “ERC-20 fan-out storm”

| Block | n_tx | gas | Character (final-RW DAG + L1 journal) |
|------:|-----:|----:|----------------------------------------|
| 14689595 | 23 | 2.6M | Tiny; almost independent |
| 14689596 | 31 | 5.0M | Quiet RAW (1); WAW-heavy |
| **14689597** | **564** | **30.0M** | **Core storm** |
| 14689598 | 111 | 6.0M | Collapses back toward quiet |
| 14689599 | 43 | 1.6M | Near-empty conflict |

**14689597 content signature:**
- Gas modes dominated by **~40248–40284** (hundreds of txs) — classic light ERC-20 / token-transfer shape; plus ~32 plain `21000` transfers.
- Heavy minority: top gas ≈ **352k** (tx 3,5,23,24,28,30,31,36…); top 10% of txs still hold ~35% of gas.
- L1 journal: **647 RAW** (605 program / 42 handler), `max_program_fanout≈448`, `wave_width=434`, chain only **29** → **wide parallel wave + one (or few) hot storage writers**, not a deep pipeline.
- Hot storage loc (final-RW): writers≈26, readers≈474, component≈475 — one conflict clique swallows most of the block.
- Gross-work at first program cross is **bimodal**: ~25/60 sample early (`d≲0.2`) vs ~33/60 late (`d≈0.94`). Late mass = “almost done then touch hot slot”; early mass = heavy txs that hit dependency early → only they justify EarlyAbort.
- OCC@8: **90 aborts**, cascade p50≈**476** (nearly whole-block ESTIMATE poison), ~12% edges see **Estimate** at discovery, bind_frac≈0.87.

**Implication:** Adaptivity here = recognize **program fan-out storm** early, Bind when Data ready, Wait+park the late fan-out majority, EarlyAbort only the early-heavy minority — not block-wide HotSet.

### 1.2 Segment B around 19606599 — “quiet → mixed Cancun-like”

| Block | n_tx | gas | Notes |
|------:|-----:|----:|-------|
| 19606597 | 124 | 11.1M | Handler-dominated RAW (final-RW); WAW spine on basic |
| **19606598** | **91** | **5.2M** | **Quiet contrast** |
| **19606599** | **367** | **30.0M** | **Mixed / busy** |
| 19606600 | 237 | 22.1M | Still busy; handler-heavy mix |

**19606598:** L1 RAW only **44**, OCC@8 **bind_frac=1.0**, Estimate=0, chain=6, wave=80. Gas mostly `21000`.

**19606599:** L1 RAW **584** (439 prog / 145 hand ≈25% handler), fanout only **14**, chain **61**, wave **261**. Gas **heterogeneous** (119× `21000`, long tail to **1.43M**). Warm journal frac **0.44**; call_depth often 2–7 (DeFi nesting). OCC@8: more **Running** than Estimate at discovery; aborts=100, cascade p50≈187.

**Critical cross-block fact:** **598 → 599 is a morphology flip in one block** (quiet → mixed). Sticky “copy 598’s lean policy into 599” fails; but a **decayable prior** that says “segment B often has handler chatter + mixed depth” still helps warm-start 599 until intra evidence accumulates.

### 1.3 Segment C around 19469097 — “WAW spine then longer RAW chain”

| Block | n_tx | Character |
|------:|-----:|-----------|
| **19469096** | 250 | **WAW spine**: chain **132**, max_writers_on_loc **132**, final-RW RAW only 6 but journal RAW 232; gas mode **29714**×127 |
| **19469097** | 336 | Longer program chain (prog_chain=47), fanout 6, RAW 410; gas mix `21000`+`29714`+DeFi |
| 19469098–99 | 268–257 | Spine softens; still WAW-heavy with lazy inflation |

**096:** Almost all first-cross gw **≈0.925** (late) → Wait EV weak if you wait after 92% work; prefer **schedule/steal** on WAW, SpecRead on handler, don’t WaitHard every multi-writer basic.

**097:** Late discovery dominant (gw p50≈0.93) but more program RAW path — Bind-if-ready wins L3; Wait-if-fanout only mild (fanout small).

---

## 2. Cross-cutting content facts (all cores)

1. **Instance RAW ≠ final-RW RAW** (597: journal 647 vs final-RW ~449; 599: 584 vs 42) — learning must use **effect/journal grammar**, not HotSet of final writers alone.  
2. **Program vs handler** mix is a first-class morphology axis (597≈94% prog; 599≈75%; quiet often handler- or empty-dominated).  
3. **Producer readiness at OCC@8 discovery** is mostly Data (~87%) but **Estimate/Running tail (10–13%)** is exactly where Wait vs SpecRead matters.  
4. **Cascades are huge** on fan-out (p50 hundreds) — contagion cost dominates single-edge redo.  
5. **Adjacent blocks in A/B/C do not share one regime** — A spikes once; B flips quiet↔busy; C keeps a storage spine across 096–097. Cross-block signal is **regime class + shared hot ℓ**, not identical RAW counts.

---

## 3. How adaptive should adapt (unified control)

### 3.1 State the learner maintains

**Intra-block (online, high rate):**
- Per ℓ: writer set, reader fan-out so far, abort count, last producer status histogram  
- Per class (program/handler): running \(d\) histogram, bind success, wait timeout / steal success  
- Block morphology posterior: `fan_out | mixed | long_chain | waw_spine | quiet` (soft weights)  
- Contagion estimate: recent cascade sizes

**Inter-block (carry with decay):**
- Morphology Dirichlet / EMA over recent blocks (not last-block hard label)  
- Per-ℓ and bytecode-hint priors when the same storage keys / contracts recur (segment C spine loc)  
- Class mix + expected fan-out + expected \(d\) shape + typical abort rate  
- **Flip detector:** if early-block evidence KL-diverges from prior → raise decay (598→599)

### 3.2 Decision (same law every edge)

At discovery of \(e=(p\to c,\ell)\):

1. **Bind** if `producer_status=Data` (and optional prior agrees).  
2. Else if **program** and (intra fan-out high **or** prior fan-out high **or** \(d\) large): **WaitHard + park/steal**.  
3. Else if **handler** or **waw_spine** posterior high: **SpecRead** (schedule/steal ≫ WaitHard).  
4. **EarlyAbort** only if (heavy-tx posterior ∧ \(d\) small) — morphology minority on 597.  
5. HotSet / writer counts = **index to update dense stats**, never the sole Wait gate.

\(d\) when inspect off: use class/morphology prior for \(d\); when inspect research on, use measured gross-work.

### 3.3 What “adapt” means at each timescale

| Timescale | Adapts what | Trigger |
|-----------|-------------|---------|
| **Opcode / edge** | Bind / Wait / SpecRead / EarlyAbort | Each cross-tx observe |
| **Within block** | Morphology weights, fan-out, abort@ℓ, lean vs full meta | Every publish / abort / steal |
| **Block boundary** | Carry EMA priors; decay if flip; seed HotSet tracking for recurring ℓ | End of block → next |
| **Segment / epoch** | Recalibrate cost model (wait vs redo constants) offline from L3 lab | Lab / rare online |

### 3.4 What L3/L4 actually authorize

- L3: **Bind-if-ready + Wait-on-program-fanout** beat M-A on hot morphologies; quiet not hurt → v3 `choose_action` OK.  
- L4: **Do not** hard-copy t−1 features as policy. **Do** keep inter-block EMA + flip-aware decay + recurring-ℓ priors (especially segment C).

---

## 4. One-paragraph summary

These historical blocks are not “one conflict rate”: **597** is a near-full-block ERC-20 **storage fan-out** with bimodal discovery depth and catastrophic OCC cascades; **598→599** is a **quiet-to-mixed flip** with DeFi nesting and handler mix; **096→097** is a **WAW storage spine** evolving into a longer program chain. SpecFence adaptivity must therefore be **dual-horizon**: intra-block EV on every RAW edge, and inter-block decayable morphology/ℓ priors that warm-start but never replace discovery — so the system adapts the *same* Bind/Wait/SpecRead/EarlyAbort law to whichever regime the next block actually is.
