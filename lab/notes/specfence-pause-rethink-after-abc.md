# SpecFence pause rethink — after ABC Iter-30 (not OCC++)

**Date:** 2026-09-08 (Asia/Shanghai)  
**Code tip:** `1058c89` (ABC Iter-30 Lean-safe nested apply default-on)  
**Smoke:** `lab/results/pause-rethink-tip-sf-occ.json` — G7 @8 cores, **N=5 median**, SoftWait Soft=0  
**Stance:** SpecFence is its own CC. OCC is a wall/TPS baseline only. Pause micro-tuning (Iter-31 cancelled).

---

## 0. Real mainnet TPS / Abort vs OCC (tip, authoritative)

| Block | Morphology | n_tx | SF wall med | OCC wall med | wall SF÷OCC | SF TPS | OCC TPS | SF/OCC TPS | SF abort med* | OCC abort med* | SoftWait Soft |
|------:|------------|-----:|------------:|-------------:|------------:|-------:|--------:|-----------:|--------------:|---------------:|:-------------:|
| **14689597** | ERC-20 **fan-out storm** (fanout≈448) | 564 | **12.5** | **4.0** | **~3.1×** | 48.2k | 140k | **0.35** | **~164** | **~79** | 0 |
| **19606599** | Mixed + flip from 598 | — | **21.2** | **8.9** | **~2.4×** | 17.0k | 37.9k | **0.45** | **~225** | **~84** | 0 |
| **19469097** | Longer RAW / WAW spine | 336 | **11.4** | **5.3** | **~2.2×** | 28.5k | 53.6k | **0.53** | **~205** | **~89** | 0 |
| **19606598** | Quiet | — | **2.1** | **1.2** | **~1.7×** | 44.1k | 89.7k | **0.49** | **~15** | **~8** | 0 |

\*abort_med from smoke SUMMARY lines; last-iter `occ_aborts` in JSON rows are one-shot (similar order).

**mean SF/OCC TPS @ tip = 0.455** (v8 reference ~0.325 on a different set — not apples-to-apples, but directionally still ~½ OCC).

### Cross-block facts that survive ABC

1. **Gap is morphology-dependent**, not a flat constant: **597 is the makespan outlier (~3×)**; others ~1.7–2.4×.  
2. **SoftWait Soft = 0** on every core — designed avoidance arm is **off**.  
3. On 597 last SF iter: **evm_entries 706 ≪ OCC 1454**, yet wall loses → loss is **park/meta + abort-repair**, not “more opcodes than OCC.”  
4. On 597 SF: **park_ms ≈ 8.6 of ~12.5 wall** (BO parks) while SoftWait Soft arms = 0 — “waiting” still happens, but as **scheduler BlockingOther idle**, not FenceGraph SoftWait at access \(a\).  
5. **absolute_jump_applied ≈ 0 on 597** after Iter-30 nested work; aj only shows on 599/097. SuffixRepair rarely *skips* interpreter work on the storm block.  
6. SF aborts ≈ **2× OCC** on conflict blocks (597/599/097); quiet 598 closer in absolute count.  
7. ABC Iter-1…30 moved wall ~19ms→~12ms era then **plateaued**; nested Bind / tip identity did **not** close 597→OCC.

---

## 1. Detect / Avoid / Resolve — post-ABC verdict

| Stage | Meaning | Tip reality | Verdict |
|-------|---------|-------------|---------|
| **Detect** | Know conflict on \(\ell\) / \(a\) | MV + validation + sticky/force_bind; high abort & fb_reabort | **Detect works.** Problem is **what we do after**. Over-abort vs OCC (~2×). |
| **Avoid** | Don’t co-schedule conflicting work | SoftWait Soft=0; BO park/steal on some paths (~8.6ms park on 597) | **Avoidance hollow as Fence protocol**; residual Wait is **BO idle**, not Bind-when-Validated at first-cross. SoftWait Soft was killed because *implementation* was useless (wake≪reabort), not because avoidance is unnecessary. |
| **Resolve** | Repair after conflict | ResumePath / SuffixRepair → escalate FullRestart; RebindOnly rare; aj≈0 on 597 | **Still the primary makespan hole** on storm: repair ≈ **extra EVM incarnation** (fra/fb_reabort), not native mid-tx continue. |

**One-line:** We **detect**, we **mostly refuse Fence-Await**, we **resolve by re-executing** (OCC-shaped discovery + sticky labels). That is **not** the designed native protocol (Fence at \(a\) → Bind/Validated → SuffixRepair ≪ FullRestart).

---

## 2. EVM axes — Region / Fence / learning

### Region wrong?
- **\(\ell\) = MemoryLocation** still correct (account Wait was rightly killed).  
- **\(a=(t,k,\ell,m)\)** exists in plant but **does not drive mass-path control** (OCC-lite SpecRead/Bind skips choose_action).  
- **Verdict:** Region *identity* OK; Region as **live control grain** still hollow.

### Fence misplaced?
| Fence | Role | Tip |
|-------|------|-----|
| SoftWait Soft | Await until Data at \(a\) | **0 arms** — dormant |
| BlockingOther | ESTIMATE / unfinished | **Dominant Wait** — park tax, steal-friendly |
| Bind / Bind-no-park | Install version | Common success path; wrong Bind → abort storm |
| SuffixRepair / abs jump | Cheap continue | **aj=0 on 597**; resume often → FullRestart |
| EarlyAbort | Cut early heavy \(d\) | Effectively dead |

**Verdict:** Not “wrong ℓ”; **fences mostly not placed at first-cross \(a\)**. Remaining Wait is scheduler BO, not FenceGraph.

### Intra-block learning effective?
- LiveLearner / AEC / HotSet exist; mass path still **OCC-lite bypass** for wall.  
- ABC spent cycles on Bind-snap / nested tips — **repair quality**, not π actuation.  
**Verdict:** Intra feedback **does not control** mass traffic; biggest historical wall win was *removing* controller tax.

### Inter-block learning effective?
- 598→599 flip still visible in data (quiet→mixed, abort explode).  
- Priors warm-start; SoftWait=0 → **cannot place Wait**; morph mode switch weak.  
**Verdict:** Inter = **warm-start without policy actuation** across flips.

---

## 3. What morphologies still demand (from blocks)

| Block | Need (native CC) | Tip failure mode |
|-------|------------------|------------------|
| **597** | Recognize hot \(\ell\) early; **Await fan-out until Validated**; then Bind; SuffixRepair ≪ FullRestart | Bind-no-park / SpecRead → abort → FullRestart; BO park burns wall; aj=0 |
| **598** | Near-OCC, minimal meta | Already ~2× small abs; rem residue |
| **599** | Handler vs program; decay quiet prior after flip | High abort both sides; SF ~2.4×; learning not specializing |
| **097** | Steal on WAW / late \(d\); don’t WaitHard late | SoftWait=0 ok-ish; abort repair + meta remain |

---

## 4. Unified diagnosis (architecture, not knobs)

```
Designed:  Region a → Fence at first-cross → dual-horizon π → cheap SuffixRepair
Running:   OCC-lite SpecRead/Bind → abort (~2× OCC) → FullRestart (+ BO park tax)
ABC effect: cheaper some resume paths; nested tip plumbing; **did not change the shape**
```

| Question | Answer |
|----------|--------|
| No detect? | **No** — detects; wrong/late **action**; over-aborts. |
| No avoid? | **Yes as Fence** — SoftWait Soft empty; BO park ≠ productive Await. |
| No resolve? | **Yes** — still EVM incarnation on storm; aj fails on 597. |
| Region wrong? | Identity OK; **event grain unused for control.** |
| Fence wrong? | **Mostly absent** at \(a\); BO is a poor substitute. |
| Intra learn? | **Bypassed / not actuating mass path.** |
| Inter learn? | **Warm-start only; flip not steered.** |

**Why ABC plateaued:** Iter-1…30 optimized **resolve plant / Bind tips / gates** inside an OCC-lite shell. Without **avoidance at \(a\)** that Bind can consume, and without **SuffixRepair that actually skips opcodes on 597**, knobs cannot beat ~3× on fan-out.

---

## 5. Bold directions (pick primary bet; stop arm-gate tuning)

1. **Reinvent avoidance at \(a\)** (not SoftWait Soft 1.0): on hot program \(\ell\), fan-out consumers **dependency-requeue / BO until writer Validated**, then Bind — measure park subtype idle, success = 597 med &lt;10 without SoftWait Soft→400+.  
2. **Resolve ≠ FullRestart on 597:** hang-free mid-tx continue that **aj applies under fan-out**, or **one serial barrier on conflict clique** then continue — success = resume EVM cost ≪ OCC reexec.  
3. **Learning actuates mode, not Wait bitmaps:** quiet OCC-lite vs storm Await-ready morph switch on flip features (L4), only on top-k \(\ell\).  
4. **Honest hybrid claim:** discovery OCC-lite; SpecFence owns **hot-\(\ell\) Await map + plant** — drop dead SoftWait Soft / Bayes-as-π fiction.

---

## 6. Bottom line

Tip multi-block: SpecFence is **~0.35–0.53× OCC TPS**, **~1.7–3.1× wall**, SoftWait Soft=0, SF aborts ~2× OCC on conflict blocks. Largest hole remains **14689597 fan-out**.

From CC: **detect OK; avoid as Fence empty; resolve still EVM.**  
From EVM: **Region identity OK; Fence not at \(a\); intra/inter learning not driving control.**

Next work must **rebuild productive avoidance at access grain** and/or **make SuffixRepair cut 597 interpreter-seconds** — not Iter-31-style arm gates.
