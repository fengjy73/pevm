# SpecFence global rethink — multi-block + architecture (not OCC++)

**Date:** 2026-09-08 (Asia/Shanghai)  
**Paused code tip:** `59754eb` (+ docs `8827f0d`)  
**Inputs:** G7 cores multi-block smokes, L1/L2/L4 notes, native resolve campaign, region/fence/learn design  
**Stance:** SpecFence is its **own** CC protocol. OCC is a wall baseline, not the architecture template.

---

## 0. Multi-block reality (SoftWait≈0 regime, prevent-first family)

Representative rows from `resolve-prevent-first*-sf-occ.json` / `resolve-fb-loop*-sf-occ.json` (wall_ms, SoftWait=0):

| Block | Morphology (from anatomy) | SF wall | OCC wall | SF÷OCC | SF aborts | OCC aborts | resume / fb_re |
|------:|--------------------------|--------:|---------:|-------:|----------:|-----------:|----------------|
| **14689597** | ERC-20 **program fan-out storm** (564 tx, fanout≈448) | ~12–14 | ~3.5–4.5 | **~3–4×** | ~170–220 | ~50–100 | ~80–100 / ~80–110 |
| **19606599** | **Mixed** Cancun-like (handler~25%, nesting, flip from 598) | ~17–20 | ~9–11 | **~1.8–2×** | ~240–280 | ~80–95 | ~85–110 |
| **19469097** | Longer RAW after WAW spine | ~10–13 | ~5–7 | **~1.8–2.5×** | ~210–250 | ~80–110 | ~50–90 |
| **19606598** | **Quiet** (RAW~44, bind_frac~1) | ~2.1–2.5 | ~1.1–1.3 | **~1.8–2×** abs small | ~12–25 | ~6–10 | ~3–10 |

**Cross-block facts that matter:**

1. Gap is **not uniform**: 597 is the outlier (~3–4×); 598/599/097 sit nearer ~2× absolute or ratio.  
2. SoftWait Soft = 0 everywhere in this regime — **avoidance-by-Wait is off**.  
3. SF often has **fewer `evm_entries` than OCC** on 597 yet still loses wall → tax + abort-repair, not “more EVM opcode count.”  
4. 598→599 morphology flip is still real; sticky copy of 598 policy into 599 was already rejected by L4 — but **priors are also not steering fences** today.  
5. On noisy OCC outliers (e.g. one run OCC@097 wall 38ms), SF can look “better” — do not confuse schedule luck with protocol win.

---

## 1. Detect / Avoid / Resolve — updated verdict

Classical CC stages on fixed commit order:

| Stage | Meaning | SpecFence today | Verdict |
|-------|---------|-----------------|--------|
| **Detect** | Know conflict on \(\ell\) / access \(a\) | MV read, ESTIMATE, validation, sticky/prior hints | **Not blind.** Conflicts surface. Often **over-eager abort** vs OCC, or **under-signal unfinished writer** before Bind-no-park. |
| **Avoid** | Don’t run conflicting work together | SoftWait Soft **≈0**; BlockingOther prefer-steal on sticky/prior/force_prefix only | **Structurally under-avoiding** on the Bind-no-park common path. Campaign killed SoftWait Soft because it was **useless** (wake_ok≪reabort), not because avoidance is unnecessary. |
| **Resolve** | After conflict: repair serializability | RebindOnly (rare), SuffixRepair once → escalate FullRestart; sticky force_bind | **Still the makespan hole** on 597: ~80–100 FullRestart-class EVM bills. |

**One-line:** SpecFence now **detects**, mostly **refuses to Wait**, and **resolves by re-executing**. That is closer to OCC’s *shape* of discovery than the native protocol thesis (Bind/Await/SuffixRepair without head reexec) — even though the *branding* is native.

**Not the 2026-09-07 dig answer verbatim:** dig said SoftWait scarce and resolve primary. Campaign fixed SoftWait storms and BO idle; SoftWait Soft was then removed. Remaining gap = **resolve still = EVM incarnation** + **avoidance hollowed out** on the hot common path.

---

## 2. EVM / architecture axes

### 2.1 Is Region wrong?

**Conflict identity \(\ell\) = `MemoryLocation` — still correct.** Account-grain Wait was rightly killed.

**Finest event \(a=(t,k,\ell,m)\) — designed right, under-powered in production:**

- Plant/`k`/checkpoints exist for SuffixRepair.  
- But the **control loop rarely fences at \(a\)**: SoftWait Soft=0, EarlyAbort≈0, choose_action skipped on OCC-lite SpecRead/Bind-on-Data.  
- So RegionPlant emits events that **learning and π largely ignore** on the mass path.

**Verdict:** Region *definition* OK; Region as **live control grain** is hollow. The protocol behaves like **tx-incarnation OCC + sticky labels**, not access-grain CC.

### 2.2 Are Fences misplaced?

Design: fence at **consumer’s first unresolved access \(a\)** on hot \(\ell\) (SoftWait / EarlyAbort / BindTarget).

Runtime:

| Fence kind | Design role | Today |
|------------|-------------|-------|
| SoftWait Soft | Avoid until Data | **≈0 arms** — demoted after profile |
| BlockingOther | ESTIMATE / unfinished | Used as Await substitute; steal-friendly |
| Bind / Bind-no-park | Install version without abort | Dominant “success” path; **wrong Bind → abort** |
| SuffixRepair fence | Resume at \(k\) | Once, then escalate FullRestart |
| EarlyAbort at early \(d\) | Cut heavy early cross | Effectively dead in SoftWait=0 regime |

**Verdict:** Fences are not “in the wrong ℓ”; they are **mostly not placed**. The remaining Await is BO (scheduler Blocking), not FenceGraph SoftWait at \(a\). FenceGraph SoftWait as architecture is **dormant**.

### 2.3 Intra-block feedback — effective?

Design: LiveLearner updates fanout, morph, P_abort, wait_useful, θ for AEC every observe/abort/publish.

Runtime:

- Common path: **OCC-lite** — skip learner observe / HotSet / choose_action / revoke.  
- Sticky / force_bind / prior_inc0: limited Await/Bind.  
- After fail: sticky extend + escalate FullRestart — **reactive**, not EV-driven Await at first cross.

**Verdict:** Intra feedback **exists in code** but is **not the controller of mass traffic**. The biggest wall win (`83412fe`) came from **bypassing** intra π. That improved wall by removing tax, and simultaneously **abandoned** the adaptive thesis for the common path.

### 2.4 Inter-block feedback — effective?

Design: InterBlockPrior EMA + flip decay; warm-start top-ℓ / morph; **never arm SoftWait from prior alone** (L4: sticky t−1→t copy loses).

Runtime:

- Prior still seeds HotSet/Bayes tracking.  
- SoftWait Soft=0 → prior **cannot place Wait fences**.  
- 598→599 flip: morphology prior might warm-start, but **policy does not change fence placement** across the flip — both run SoftWait=0 Bind-no-park.

**Verdict:** Inter learning is **warm-start plumbing without control effect**. L4 correctly rejected hard sticky Wait copy; we over-corrected into **priors that never act**.

---

## 3. What each morphology demands (from blocks, not OCC)

### 597 — fan-out storm
- Need: early recognize **one (few) hot \(\ell\)**; Bind when Data; **Await fan-out majority** until writer validated; EarlyAbort only early-heavy \(d\).  
- Today: Bind-no-park / SpecRead through unfinished → abort → FullRestart. SoftWait Soft proved bad *as implemented* (wrong wake/repair), not “Await is wrong.”  
- Hole: **avoidance at \(a\) on hot \(\ell\)** must be reinvented hang-free (BO Await until Executed/Validated, or true mid-tx park), plus resolve that is not FullRestart.

### 598 — quiet
- Need: near-OCC discovery, almost no meta.  
- Today: already ~2× small absolute; OCC-lite path is the right *shape*; remaining tax is rem/engagement residue.

### 599 — mixed + flip
- Need: handler vs program split; don’t Wait handler; adapt after 598 quiet prior **decays**.  
- Today: SoftWait=0; high abort both sides; SF ~2× OCC — less catastrophic than 597 but learning not specializing.

### 096/097 — WAW spine / late \(d\)
- Need: schedule/steal on WAW, not WaitHard after \(d≈0.93\).  
- SoftWait Soft=0 accidentally agrees with “don’t Wait late”; remaining loss is abort repair + meta.

---

## 4. Unified diagnosis (not OCC-shaped)

```
Designed SpecFence:  Region a → Fence at first-cross → dual-horizon π → cheap SuffixRepair
Actual SpecFence:    OCC-lite SpecRead/Bind → abort → FullRestart (+ sticky labels)
```

| Question | Answer |
|----------|--------|
| No detect? | **No** — detects; sometimes wrong **action** after detect. |
| No avoid? | **Yes, now** — SoftWait Soft removed; unfinished-writer avoidance incomplete on mass path. |
| No resolve? | **Yes, still** — repair ≈ extra EVM incarnation (~80–100 on 597). |
| Region wrong? | **Identity OK; event grain unused for control.** |
| Fence wrong place? | **Fences mostly absent** (SoftWait Soft dormant). |
| Intra learn ineffective? | **Bypassed on purpose** for wall — controller hollow. |
| Inter learn ineffective? | **Warm-start only; no fence/policy actuation.** |

The campaign correctly killed **bad avoidance** (SoftWait Soft that didn’t help) and **bad resolve loops** (fb_reabort chains), then plateaued because it never rebuilt **good avoidance at \(a\)** + **cheap resolve ≠ FullRestart**.

---

## 5. Bold directions (SpecFence-native, falsifiable)

Pick one primary bet (do not stack Wait storms):

### A. Reinvent avoidance at access grain (not SoftWait Soft 1.0)
- On hot program \(\ell\) with unfinished writer: **BlockingOther / dependency requeue until writer Validated**, then Bind — for **fan-out consumers**, not only sticky/force_prefix.  
- Success: 597 median wall &lt;10 without SoftWait Soft arms rising to G7-era 428.  
- Risk: BO idle returns if steal fails — measure subtype idle.

### B. Resolve ≠ FullRestart
- Hang-free mid-tx jump **narrow subset** (CallEntry/Storage prefix only) so SuffixRepair cuts interpreter-seconds; or conflict-tx **one serial barrier** then continue.  
- Success: resume/full_restart EVM cost ≪ OCC abort reexec on 597.  
- Risk: hang (inspect history); serial barrier hurts 598-class.

### C. Make learning actuate again
- Intra: force `choose_action` only on **candidate hot \(\ell\)** (prior top-k + live fanout), not every SpecRead.  
- Inter: morph flip 598→599 switches **engagement mode** (quiet OCC-lite vs storm Await-ready), not sticky Wait sets.  
- Success: 599 and 597 specialize; L4-style features predict mode, not Wait bitmaps.

### D. Accept dual protocol honesty
- Discovery = OCC-lite (keep).  
- SpecFence **only** owns: hot-\(\ell\) Await map + SuffixRepair plant.  
- Drop dead SoftWait Soft / Bayes-as-π / account Region — already mostly done.  
- Paper claim = hybrid access-grain Await + plant, not “adaptive OCC.”

---

## 6. Bottom line

Across more blocks, SpecFence’s remaining loss is **largest on fan-out storm 597**, moderate elsewhere. Globally the architecture **still names** Region/Fence/dual-horizon learning, but the running system is **OCC-lite discovery + expensive abort resolve**, with **fences and learning largely disconnected from the mass path**.

From CC: **avoidance was emptied** and **resolve is still EVM**.  
From EVM: **Region identity OK; fences not at \(a\); intra/inter learning not driving control.**

Next work must **rebuild avoidance at \(a\)** and/or **make SuffixRepair cheaper than FullRestart** — not more SoftWait Soft knobs, not more OCC imitation, not more validate micro-opts.
