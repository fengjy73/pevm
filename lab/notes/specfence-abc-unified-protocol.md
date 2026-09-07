# SpecFence A+B+C unified protocol (one system, iterative)

**Date:** 2026-09-08 (Asia/Shanghai)  
**Status:** AUTHORITATIVE for next implementation — A/B/C are **one** protocol, not phases  
**Base tip:** `59754eb` / rethink `2d683c8`  
**Stance:** Not OCC++. Dual-horizon learning + access-grain fences + cheap resolve.

---

## 0. Non-negotiable process

Every iteration **must** answer before coding the next patch:

1. Multi-block evidence (597/598/599/097 at least) + current architecture map  
2. CC: **detect / avoid / resolve** — which is the hole *this round*?  
3. EVM: **Region** wrong? **Fence** misplaced/absent? **Intra** learn dead? **Inter** learn dead?  
4. Locate cause → one integrated fix that may touch A∩B∩C together → measure → repeat  

Never “finish A then B then C.” Never optimize only SF/OCC branding.

---

## 1. Unified runtime (A∧B∧C)

```
Inter prior (C): morph + top-ℓ → engagement mode
  quiet  → OCC-lite discovery (keep tax-free SpecRead)
  storm  → hot-ℓ Await-ready (A) + plant-ready resolve (B)

Intra (C): on Observe hot ℓ only → update fanout/θ → may arm fence
  cold ℓ → OCC-lite SpecRead (no π)

Avoid (A): fence at consumer first-cross a on hot ℓ
  unfinished writer → BlockingOther prefer-steal Await until Validated/Data
  then Bind — NOT SoftWait Soft 1.0 (useless wake_ok≪reabort)
  SoftWait Soft stays ~0 unless EV proves useful on measured wake

Resolve (B): on fail
  RebindOnly if value-stable
  else SuffixRepair + hang-free mid-tx jump when jump_is_safe (narrow)
  else one SuffixRepair resume; escalate FullRestart only after depth≥2
  certified prefix never ESTIMATE-poisoned

Region: conflict id = ℓ; event = a=(t,k,ℓ,m); fence attaches to a
FenceGraph: Await/BindTarget at a; SoftWait Soft dormant unless re-proven
```

---

## 2. Success metrics (multi-block)

| Block | Target direction |
|-------|------------------|
| 597 | median wall → &lt;10 then &lt;8; SoftWait Soft ≪428; fb_reabort/resume down |
| 598 | stay near OCC absolute (don’t regress tax) |
| 599 | specialize vs 598 flip via inter morph mode |
| 097 | no WaitHard-after-late-d; steal/WAW friendly |

Each iter report: detect/avoid/resolve verdict + Region/Fence/intra/inter + tables.

---

## 3. Iteration log (append each round)

### Iter 0 — protocol freeze
This note. Implement A+B+C in one landing, then Iter 1 diagnose.

### Iter 1 — unified landing + diagnose
Landed A∧B∧C runtime (BO Await+armed_at_k; RebindOnly/SuffixRepair/escalate; Quiet|Storm mode).  
597 N=5 med **13.3** SoftWait=0 (≈ plateau). Hole remains **resolve=FullRestart EVM**; jump_applied=0.  
See `specfence-abc-iter1-status.md`. Iter 2 cause: hang-free live snap → jump before escalate.

### Iter 2 — live_prime→jump ladder falsified
Tried hang-free live snap via narrow Lean inspect prime + delay fb escalate when `jump_is_safe`. Preview can be true, but inspect/jump under concurrency **hangs 597/599**. Production: live_prime OFF; classic escalate. SoftWait=0; 597 med ~13.9 ≈ plateau; aj=0.  
See `specfence-abc-iter2-status.md`. Iter 3 cause: hang-free PC capture ≠ full inspect_run (or serial-barrier resolve).

### Iter 3 — serial-barrier resolve (modest)
Storm∧was_force_bind escalate → once park behind Executing conflict writer (no inspect).  
597 N=10 med **13.4** SoftWait=0 (↓ vs Iter2 13.9); stretch &lt;10 unmet; aj=0.  
See `specfence-abc-iter3-status.md`. Iter 4 cause: hang-free PC ≠ inspect_run and/or stronger clique barrier.

### Iter 4 — Handler SSTORE plant + hot-ℓ barrier
Hang-free SSTORE lite capture plumbing landed but Lean plant TLS livelocks WaitHard (same family as inspect). Production: plant OFF on Lean; hot-ℓ fanout writer select for serial-barrier.  
597 N=10 med **14.2** SoftWait=0 (no win vs Iter3 13.4); aj=0; hsstore=0.  
See `specfence-abc-iter4-status.md`. Iter 5 cause: serial one-tx capture window or write-prefix skip without plant TLS.

### Iter 5 — head-FF retain on escalate (modest)
Write-prefix DB skip: escalate FullRestart keeps certified FF values (`ff_head`) for try_ff_* without plant TLS / Lean jump.  
597 N=5 med **13.1** SoftWait=0 (↓ vs Iter3 13.4 / Iter4 14.1); N=10 med **14.1** (↓ vs Iter4 14.2, not vs Iter3 13.4). ff_hits↑~2.4×; aj=0. Lean absolute jump falsified (seq≠par).  
See `specfence-abc-iter5-status.md`. Iter 6 cause: opcode-skip hang-free jump (memory-safe) or fewer FullRestarts via RebindOnly/longer SuffixRepair.

### Iter 6 — fewer FullRestarts via cheap-resume SuffixRepair + RebindOnly widen
Estimate→Data spin + multi-origin Basic snap; one extra SuffixRepair when RewindTo/FF armed (depth≥2, not was_force_bind-at-1). depth≥3 / storm fanout-Await **falsified** (wall↑).  
597 N=5 med **13.0** SoftWait=0 (↓ vs Iter5 13.1); N=10 med **14.0**; **full_restart ≈½**.  
See `specfence-abc-iter6-status.md`. Iter 7 cause: SuffixRepair resume opcode-seconds / make 2nd repair succeed (sticky Await) or memory-lite jump.

### Iter 7 — 2nd SuffixRepair sticky Await (BO) before resume
After first SuffixRepair fail: sticky + force_bind-extend fail locs; storm∧Executing writer BO-park before 2nd resume (sra). Memory-lite abs jump gated on non-empty live memory (aj=0 without inspect). SoftWait Soft=0.  
597 N=5 med **13.2** SoftWait=0; N=10 med **13.9**; **fb_reabort/resume/fr↓**.  
See `specfence-abc-iter7-status.md`. Iter 8 cause: remaining SuffixRepair opcode-seconds; hang-free memory snap ≠ inspect, or RebindOnly on certified-prefix-only / Storage-stable fails.

### Iter 8 — hang-free memory snap plumbing; jump falsified
Handler SSTORE memory clone (≤8KiB) + run_exec_loop PENDING_RESUME apply (no inspect). Broad memory-lite jump **seq≠par**; capture-without-jump wall↑; RebindOnly widen / first-repair Estimate park falsified. Production jump/capture OFF. SoftWait=0; aj=0; hsstore proof off-path.  
597 N=5 med **13.4**; N=10 med **13.4** (↓ vs Iter7 13.9).  
See `specfence-abc-iter8-status.md`. Iter 9 cause: correct Handler jump restore → aj>0 seq≡par then enable capture+jump.

### Iter 9 — Handler jump restore gates; multi-SSTORE still unsafe
Diagnosed early-tip + clobbered plant write_replays as seq≠par cause; landed tip embedding + plant-gas preserve + refuse multi-SSTORE/`sstore_index!=1`. Multi-SSTORE last tip still seq≠par; single-SSTORE safe but aj≈0 on ERC-20. Production jump/capture OFF. SoftWait Soft=0; aj=0; hsstore=0.  
597 N=5 med **17.4** / N=10 **19.4** (no wall win vs Iter8d 13.4 — jump stayed OFF).  
See `specfence-abc-iter9-status.md`. Iter 10 cause: multi-SSTORE Handler abs jump ≡ sequential under pevm MV (or cut resume opcode-seconds without abs jump).

### Iter 10 — restore Iter9 wall tax; jump still OFF
Diagnosed Iter9 accidental hot-path tax (fat SSTORE wrap always installed; first_k-from-gas; gas>0 write_replay keep). Stock SSTORE unless plant wanted; rem first_k restore. Value-stable FF falsified. SoftWait Soft=0; aj=0.  
597 N=5 med **13.3**; N=10 med **13.8** (restored vs Iter9 19.4 / approx Iter8d 13.4).  
See `specfence-abc-iter10-status.md`. Iter 11 cause: multi-SSTORE Handler abs jump == sequential under pevm MV (or new evidence for resume without abs jump).

### Iter 11 — multi-SSTORE jump falsified; plant gas fixed
Diagnosed plant pre-sload warm (−2100) seq≠par; fixed no-warm original. Landed k<k_fail snap select + plant-only first_k + last-tip gates. Multi-SSTORE abs jump still not aj>0∧seq≡par on Lean → jump/capture OFF. SoftWait=0; 597 N=5 med **12.7**.  
See `specfence-abc-iter11-status.md`. Iter 12 cause: alternate resume opcode cut without abs jump.

### Iter 12 — Validated gate + doomed-2nd-repair→barrier (wall↓)
Lock-free `is_validated`; 2nd-repair prefer_await Validated spin (no park);
force_prefix ESTIMATE→BO (Executing only); doomed SuffixRepair→serial-barrier escalate;
longer RebindOnly spin on force_bind/ff_head. Abort-path evidence spins falsified (wall↑).
597 N=5 med **12.5** SoftWait=0 (↓ vs Iter11 12.7); N=10 med **14.0** (↓ vs 14.2). aj=0.
See `specfence-abc-iter12-status.md`. Iter 13 cause: hang-free opcode cut on *successful*
SuffixRepair without abs jump / abort-path spins.

### Iter 13 — Validated-gated vs-FF + multi-cand barrier
Validated-gated Storage+Basic value-stable journal FF (Iter10 bare falsified);
serial-barrier multi-candidate (no sibling park). Executed→Validated escalate spin /
FF-path yield / single-SSTORE JUMP=1 hung — falsified. SoftWait=0; aj=0; vs_ff rare.
597 N5 med **12.9** / N10 **13.7** (≤ Iter12 N10 14.0). Stretch <10 unmet.
See `specfence-abc-iter13-status.md`. Iter 14 cause: schedule-side Validated Await
before first repair / RebindOnly collapse / hang-free non-TLS jump — not FF/spin.

### Iter 14 — first-repair schedule Await + RebindOnly Validated collapse
Schedule-side Executing BO park after first SuffixRepair arm (prevent doomed
first resume; Estimate park stays falsified). RebindOnly Validated collapse spin
pre-abort. Keep Iter12 2nd-repair + Iter13 Validated FF. Jump/capture OFF.
597 N5 med **12.4** SoftWait=0 (≤ Iter12 12.5 / Iter13 12.9); N10 **13.6**.
fra fires; aj=0. Stretch <10 unmet.
See `specfence-abc-iter14-status.md`. Iter 15 cause: collapse fan-out FullRestarts
/ RebindOnly on true_suffix / hang-free opcode skip — not SoftWait Soft / Estimate park.

### Iter 15 — fan-out FR collapse (fr↓; wall plateau)
First-fail true_suffix + fan≥8 Executing spine → escalate+serial-barrier; widen
storm barrier; longer true_suffix Validated RebindOnly spin. Keep Iter12–14.
Jump/capture OFF. SoftWait Soft=0.
597 N5 med **13.1** SoftWait=0; **fr≈½ vs Iter14**; stretch <10 unmet.
Drain-spin / BO-OR Await falsified.
See `specfence-abc-iter15-status.md`. Iter 16 cause: cheap absorb (validate-defer /
RebindOnly-after-spine) or hang-free opcode skip — not early FullRestart tax.

### Iter 16 — cheap absorb (wall ≤ Iter14)
SuffixRepair+fra absorb on Executing-spine fan≥8 **without sticky** (16a sticky
falsified wall↑); FR collapse only for Estimate/Aborting doomed; validate-defer
plumbing for !true_suffix (fvd≈0 on 597). Keep Iter12–15. Jump/capture OFF.
597 N5 med **12.3** SoftWait=0 (≤ Iter14 12.4; ↓ vs Iter15 13.1).
See `specfence-abc-iter16-status.md`. Iter 17 cause: hang-free opcode skip /
RebindOnly-after-fra on true_suffix / schedule Await before SpecRead — not sticky-absorb.

### Iter 17 — yield-spin Await before SpecRead (plateau)
Schedule Await before SpecRead via yield-spin on storm+program live_fanout≥8
unfinished writers (no BO park). BO park / true_suffix defer / long RebindOnly
wait falsified. Keep Iter16 absorb-no-sticky. Jump/capture OFF. SoftWait Soft=0.
597 N5 med **12.7–12.8**; N10 **13.0** (≤ Iter16 N10 13.5). Stretch <10 unmet.
See `specfence-abc-iter17-status.md`. Iter 18 cause: hang-free opcode skip on
successful SuffixRepair / later RewindTo without mass-path tax — not fan BO park.

### Iter 18 — opcode-skip diagnosis (plateau; SoftWait Soft=0)
Plant+capture → aj=0 (SSTORE snaps after k_fail on RAW-read fails). Synthetic mid
RewindTo / late-k yield / ForceBind·park ff_head falsified (599 wall↑). Production
= Iter17 tip. SoftWait Soft=0; 597 N10 **12.6**; N5 hits **12.3** in quiet samples.
See `specfence-abc-iter18-status.md`. Iter 19 cause: hang-free live snap at
certified-prefix end (not post-SSTORE plant) or 599-safe critical-path schedule.
