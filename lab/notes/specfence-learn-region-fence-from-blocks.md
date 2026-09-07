# What to learn, how to learn, where region lives, how to place fences

**Date:** 2026-09-07 (Asia/Shanghai)  
**Status:** DESIGN derived from A/B/C block anatomy + L1/L2/L3  
**Complements:** `specfence-block-content-and-adaptive-howto.md`, REM Spec v1, control law v3  
**Evidence tip:** methodology `0b4affc` / howto `24094eb`

---

## 0. Three grains that must not be conflated

Block data forces a **three-layer** vocabulary. Mixing them is why account-HotSet and tx-wide Wait failed.

| Layer | Symbol | What it is | From the blocks |
|-------|--------|------------|-----------------|
| **Concurrency object** | \(\ell\) | `MemoryLocation` (Storage/Basic/CodeHash) | RAW/WAW edges live here; 597 hot storage ~26W/474R |
| **Region access (finest CC event)** | \(a=(t,k,\ell,m)\) | One journal R/W effect ordinal \(k\) inside tx \(t\) | L1 effect_log; first-cross \(k/\mathrm{effects}\) bimodal on 597 |
| **Fence / schedule task** | soft edge on \(\widehat G\) | Where π inserts Wait/Bind/park or EarlyAbort | Not “whole account”; not always whole tx |

**Region (最小并发控制单元) = \(a=(t,k,\ell,m)\)** for events and learning updates.  
**Conflict identity = \(\ell\)** for versions / Wait keys / HotSet index.  
**Park grain today = tx** (M2); **ideal fence site = consumer’s first unresolved access \(a\) on hot \(\ell\)** (and optionally EarlyAbort at that \(k\)).

**Explicit rejects from measurements:**
- Account-grain as Wait key → false Wait amplifier (deeper pass).  
- Final-RW-only HotSet as region → undercounts instance RAW (599: 42 final vs 584 journal).  
- Block-wide conflict bit as region → cannot express 598→599 flip or 597’s bimodal \(d\).

---

## 1. What to learn (feature inventory grounded in blocks)

### 1.1 Must learn (feeds `choose_action`)

| Feature | Grain | Why (block evidence) |
|---------|-------|----------------------|
| **class(\(\ell\))** program vs handler | \(\ell\) | 597≈94% prog Wait-worthy; 599≈25% handler → SpecRead |
| **fan-out(\(\ell\))** live readers of last writer | \(\ell\), online | 597 max_prog_fanout≈448; L3 Wait-if-fanout wins |
| **producer_status** at discovery | edge \(e\) | OCC@8: ~87% Data (Bind), 10–13% Estimate/Running (Wait vs Spec) |
| **gross-work \(d\)** at first cross | access \(a\) | 597 bimodal ~0.11 vs ~0.94; drives Wait vs EarlyAbort |
| **tx_work band** light vs heavy | \(t\) | 597: mass @40260 vs minority @352k |
| **P(abort∣\(\ell\))** / cascade size | \(\ell\), block | 597 cascade p50≈476 — contagion cost |
| **morphology posterior** | block | fan_out / mixed / waw_spine / quiet |
| **WAW/RAW ratio** | block / \(\ell\) | 096 ≈0.74 → schedule/steal ≫ WaitHard |
| **recurring \(\ell\)** across adjacent blocks | inter | C spine storage writers persist 096→097 |
| **bind success / wait useful** | edge outcome | closes Bayes loop |

### 1.2 Nice-to-learn (secondary)

- call_depth at cross (599 nesting 2–7)  
- lag \(c-p\) (597 mostly 1–2; 097 heavy tail)  
- warm/cold (definitional; don’t treat warm re-read as new dependency)  
- opcode-fraction \(d\) as secondary to gross-work

### 1.3 Do not learn as control keys

- gas/limit depth  
- account-level sticky Wait as primary  
- sticky copy of previous block’s Wait set (L4)

---

## 2. How to learn (dual-horizon update rules)

### 2.1 Intra-block (online, every observe / abort / publish)

```
on observe edge e=(p→c,ℓ) at access a:
  update fanout[ℓ], class mix, status hist[ℓ]
  update d_hist[class], d_hist[morph]
  action ← choose_action(... prior + online ...)
  if Bind/Wait/SpecRead outcome later known:
      Beta/EMA update P_conflict[ℓ], P_bind[ℓ], wait_useful[ℓ]
on abort(t) with fail locs L:
  for ℓ in L: abort[ℓ]++; cascade_ema ← size
  bump morph weight toward fan_out if |readers| large
on publish Data(p,ℓ):
  wake Waiters; credit Bind priors
```

Learning ∉ TCB: wrong Wait only costs time; validate still enforces seq≡par.

### 2.2 Inter-block (boundary, warm-start only)

```
end_block:
  morph_ema ← (1-α)·morph_ema + α·morph_hat
  for ℓ in touched_program: ℓ_prior[ℓ] ← EMA(fanout, abort, class)
  if KL(early_evidence, morph_ema) large: α ← α_flip  # 598→599
next_block_start:
  seed dense tracking for top prior-ℓ (HotSet = index)
  seed Bayes (α,β) from ℓ_prior / morph_ema
  do NOT promote RegionMode::Wait from prior alone
```

### 2.3 Offline (L3 lab / rare)

Recalibrate `D_WAIT`, `D_EARLY`, `C_RETRY`, COST_MARGIN from wasteΔ tables — not from hand thresholds.

---

## 3. Where the region unit should sit

### 3.1 Spec answer (unchanged intent, sharpened)

REM Spec v1 already: **region access \(a=(t,k,\ell,m)\)** with \(\ell=\) `MemoryLocation`.  
Code today partially regresses to:
- `RegionTable` sticky **location or account** Wait mode (too coarse / account harmful)  
- HotSet membership as Wait gate (v1; v3 demoted to hint — keep demoting)  
- M2 park at **tx** (necessary until mid-tx Wait is hang-free)

### 3.2 Target placement (from these blocks)

1. **Identity of conflict / versions:** \(\ell\) only (never account for Wait).  
2. **Event & learning atom:** \(a=(t,k,\ell,m)\) on every cold (and policy-tagged warm) journal R/W.  
3. **Fence attachment point:** soft dependency edge \(p \xrightarrow{\ell} c\) attached to the **consumer access \(a_c\)** that first observes unresolved \(\ell\) — i.e. fence *at first-cross*, not at tx start.  
4. **Repair/resume atom (PartialRetry):** checkpoint at `StorageWrite` / `CallEntry` nearest \(k\) of first-cross when \(d\) early; late light txs (597 \(k/\mathrm{effects}\approx0.86\), \(d\approx0.94\)) gain little from EarlyAbort — prefer Wait+park before that SLOAD or SpecRead+validate.  
5. **Wave scheduling grain:** ready set of **txs** (or future `(t,k)` continuations) constrained by soft Wait edges on \(\ell\).

### 3.3 Mapping morphologies → region emphasis

| Morphology | Dominant region pattern | Unit emphasis |
|------------|-------------------------|---------------|
| **597 fan_out** | One (few) storage \(\ell\), huge reader set | Dense stats on that \(\ell\); fences on many \(a_c\) |
| **599 mixed** | Many \(\ell\), handler+program, nested calls | Per-\(\ell\) Bayes; don’t global Wait |
| **096 waw_spine** | Same \(\ell\) rewritten 132×, few RAW | Region still \(\ell\), but **no Wait fence**; order/steal |
| **598 quiet** | Sparse \(a\) | LeanOCC; almost empty region table |

---

## 4. How to place fences (derived, not invented)

A **fence** here = a scheduling constraint π inserts: WaitHard (soft edge), Bind (hard once published), or EarlyAbort (cut incarnation). Validate remains the hard correctness fence.

### 4.1 Placement rules from measured first-cross

| Condition (from traces) | Fence | Where |
|-------------------------|-------|-------|
| `producer_status=Data` | **Bind** | At \(a_c\) — no wait |
| program \(\ell\) ∧ (fanout high ∨ \(d \ge D\_WAIT\)) ∧ producer not done | **WaitHard+park** | At \(a_c\) (first unresolved read of \(\ell\)); park **tx** until Data (M2); worker steals |
| handler \(\ell\) ∨ WAW/RAW high (096) | **No Wait fence** | SpecRead at \(a_c\); rely on validate + steal |
| heavy tx ∧ \(d \le D\_EARLY\) ∧ program (597 minority ~25) | **EarlyAbort fence** (when rem arm exists) | At that early \(a_c\) (\(k/\mathrm{effects}\sim0.15\), \(d\sim0.11\)); else temporary WaitHard |
| beneficiary / basic_lazy | **Never fence** | Exclude from \(\widehat G\) soft edges |
| quiet morph / cold \(\ell\) | **No fence** | SpecRead / LeanOCC |

### 4.2 Block-specific fence plans (examples)

**14689597:**  
- Identify hot storage \(\ell^*\) as soon as fan-out climbs.  
- For light ~40260 consumers: fence **WaitHard at late SLOAD** of \(\ell^*\) (\(d\approx0.94\)) — saves cascade, little sunk waste if Bind soon.  
- For heavy ~352k with \(d\approx0.11\): prefer **EarlyAbort / RewindTo** at first bad cross, not wait-after-sunk-work.  
- Do not fence all basics.

**19606598→599:**  
- Start with quiet prior (few fences).  
- On 599, as handler+program mix appears, **raise SpecRead share**; Wait only on program \(\ell\) that actually fan out (max fanout only 14 — sparse Wait).  
- Flip decay: discard 598’s empty Wait set quickly.

**19469096:**  
- Spine \(\ell\) with 132 writers: **schedule fence only** (commit order + steal), **zero WaitHard** on that WAW chain.  
- Journal RAW still tracked for learning, but π → SpecRead.

**19469097:**  
- More program RAW path, late \(d\): **Bind-if-ready** primary; light Wait on true program RAW, not on WAW spine leftovers.

### 4.3 Fence lifecycle

1. **Arm** soft Wait edge when `choose_action→WaitHard` at \(a_c\).  
2. **Satisfy** on publish Data → Bind / wake.  
3. **Revoke** if posterior drops (`τ_revoke`) or morph flips to quiet/waw_spine.  
4. **Never** arm from inter-block prior alone without an observe.

---

## 5. Learning ↔ region ↔ fence (one diagram)

```
inter-block EMA/ℓ priors ──warm-start──► Bayes(ℓ), morph weights
                                              │
intra observes a=(t,k,ℓ,m) ──update──────────┤
                                              ▼
                                    choose_action(e,d,class,status,prior)
                                              │
                    ┌─────────────┬───────────┼───────────┐
                    ▼             ▼           ▼           ▼
                 Bind          WaitHard    SpecRead   EarlyAbort*
                 at a_c        fence+park  no fence   cut at a_c
                                 on ℓ
                                              │
                                         validate (TCB)
```

\*EarlyAbort when rem/PartialRetry arm exists; today approximated by WaitHard on that niche.

---

## 6. Implementation implications (ordered)

1. Keep **conflict key = MemoryLocation**; remove/ignore account Wait as control (diag only).  
2. Treat **HotSet strictly as dense-tracking index** for high fan-out \(\ell\) (597).  
3. Attach policy decision to **first-cross access**, recording \(d\) when inspect research on; else morph/class prior for \(d\).  
4. Fence = soft edge list in \(\widehat G\), not `RegionMode::Wait` sticky bit alone (sticky bit may mirror soft edge but must be revokeable and never account-wide).  
5. Dual-horizon learner: §2.1 + §2.2; L3 recalibrates constants.  
6. Next plant gap: true EarlyAbort / RewindTo at early \(a_c\) for 597 heavy minority — biggest missing fence type vs data.

---

## 7. Bottom line

From these blocks: **learn per-\(\ell\) fan-out/class/abort and per-access \(d\)/status, with EMA cross-block priors**; **the minimal CC unit is region access \(a=(t,k,\ell,m)\) on MemoryLocation \(\ell\)**; **fences sit on consumer first-cross soft edges — WaitHard for program fan-out/late \(d\), none for handler/WAW spine, EarlyAbort only for early-heavy — never account-wide and never prior-only.**
