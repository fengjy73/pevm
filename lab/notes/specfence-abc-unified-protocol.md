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

### Iter 19 — Bind-snap capture proven; jump hung (plateau)
Hang-free Handler SLOAD Bind-snap at certified-prefix end (not post-SSTORE):
opt-in `SPECFENCE_BIND_SNAP=1` → bsnap≈850 on 597. Absolute jump
(`SPECFENCE_BIND_SNAP_JUMP=1`) hung Lean fixtures — production OFF.
Capture-without-jump wall↑; mega-fan yield falsified. Production = Iter17 tip.
SoftWait Soft=0; 597 N5 ~14.9 / N10 ~16.3 under high load.
See `specfence-abc-iter19-status.md`. Iter 20 cause: hang-free Bind-snap consume
≠ full abs jump, or fix jump hang with seq≡par, or 599-safe critical-path
without yield-deepening.

### Iter 20 — Bind-snap consume: credit hang-free; jump falsified
Hang-free credit consume of Bind tips (`bcredit`) with SNAP opt-in; abs jump
hard-OFF — Storage-FF Bind jump hung 597 even after dropping `!memory_lite_ok` +
Validated-safe origin seed. Basic-only tips refuse (`bytecode_no_storage_ff`).
SoftWait Soft=0; production SNAP OFF (no default tax). 597 N5 ~15.1 under load.
See `specfence-abc-iter20-status.md`. Iter 21 cause: minimal Storage-FF Bind jump
hang repro (seq≡par) or non-jump opcode cut / 599-safe critical-path.

### Iter 21 — Bind jump hang/seq≠par understood (JUMP OFF)
Minimal Storage-FF Bind jump dig: ERC-20 SNAP+JUMP → **aj>0 ∧ seq≠par** (restore
wrong under pevm MV); aj metric was PLANT-blind (fixed via BIND_SNAP). Validated-all
origins + TLS clear + depth≤1 gate. SNAP-only seq≡par. Production JUMP/SNAP OFF.
SoftWait Soft=0; 597 N5 quiet **13.4** / N10 **13.6**.
See `specfence-abc-iter21-status.md`. Iter 22 cause: correct Bind-jump restore
seq≡par on ERC-20, or non-jump opcode cut / 599-safe critical-path ≤12.3→<10.

### Iter 22 — Bind-jump restore plumbing; ERC-20 still flaky (JUMP OFF)
FF journal warm + matching-origin value check + seed clear-on-fail + memory/tip
gates + Validated-prefix dig spin. ERC-20 SNAP+JUMP still **flaky** aj>0∧seq≠par
(even behind Validated prefix) → JUMP/SNAP stay OFF. SoftWait Soft=0; 597 N5
**13.5** plateau. See `specfence-abc-iter22-status.md`. Iter 23 cause: diff-first
restore vs cold SuffixRepair, or non-jump opcode cut / 599-safe ≤12.3→<10.

### Iter 23 — diff-first Bind restore + fra pre-yield (JUMP OFF)
Diff-first: ERC-20 aj>0∧seq≠par = status revert dgas=+661 from stale Bind
SLOAD values already consumed into require/SUB (stack patch insufficient).
Landed tip_sloads log + refuse jump when Bind SLOAD ≠ FF → dig **aj>0∧fail=0**
stable; 597 SNAP+JUMP no-hang. Production JUMP/SNAP stay OFF (capture tax).
Non-jump: high-fan (≥32) first-repair pre-yield skip-park. SoftWait Soft=0.
597 N5 med **12.9**. See `specfence-abc-iter23-status.md`. Iter 24 cause:
cautious enable Bind jump under concurrency (wall proof) or deepen 599-safe
schedule ≤12.3→<10.

### Iter 24 — ResumePath Bind-jump enable (no mass SNAP tax)
Cautious production enable via `SPECFENCE_BIND_SNAP=resume` (default Off — Lean
fixtures hang if JUMP silent-default). Capture only on SuffixRepair resume /
force_bind / needs_live_capture (not every Handler run). JUMP follows resume/mass
with refuse-if-stale. Mass SNAP still `=1`. Wall proof: resume 597 N5 **13.0** ≈
Off / no mass tax; SoftWait Soft=0; 599 N10 **aj=1**. fra≥16 deepen **falsified**.
Stretch <10 unmet. See `specfence-abc-iter24-status.md`. Iter 25 cause: opcode
cut that arms aj on 597 discovery without mass SNAP, or new 599-safe schedule
≤12.3→<10 (not fra≥16 / yield deepen / SoftWait Soft / BO park); or hang-free
silent-default ResumePath.

### Iter 25 — silent-default ResumePath (Lean hang-free)
Default `SPECFENCE_BIND_SNAP` unset → ResumePath (Mass JUMP was Lean hang).
tip_sloads prefix-spin skip / broad inc>0 SNAP **falsified**. SoftWait Soft=0;
597 N5 **13.7** plateau; stretch <10 unmet; aj≈0 on 597.
See `specfence-abc-iter25-status.md`. Iter 26 cause: Validated-fresh tip→jump
on 597 without prefix-spin skip, or new 599-safe schedule ≤12.3→<10.

### Iter 26 — Validated-fresh FF-path tip→jump (wall <13 N5)
FF-served SLOAD arms Bind-snap (tip≡FF); Bind-on-Data snap OFF; prefer tip≡FF at
jump_snap select; one deferred attach/resume (per-SLOAD attach tax falsified).
Keep all-prefix Validated spin. SoftWait Soft=0; silent ResumePath.
597 N5 med **12.4** SoftWait=0 (↓ vs Iter25 13.7; wall <13); N10 **13.3**;
599/097 **aj>0**. Stretch <10 unmet; 597 aj=0 under fan-out.
See `specfence-abc-iter26-status.md`. Iter 27 cause: 597 aj under fan-out without
prefix-spin skip, or N10 schedule ≤12.3→<10.

### Iter 27 — tip≡FF overlap + steps_cap + deeper prefix spin (wall↓)
Cumulative tip_sloads extras no longer refuse-if-stale (overlap match); jump_snap
select prefers tip≡FF ∧ steps≤cap (not max steps→steps_over); all-prefix Validated
spin 8192→32768 (no tip_sloads skip). Nested apply-on-hash-mismatch **hung** — OFF.
SoftWait Soft=0; silent ResumePath; FF-only tip≡FF.
597 N5 med **12.1** SoftWait=0 (↓ vs Iter26 12.4); N10 **12.5** (↓ vs 13.3); aj=0
on 597 (code_hash first-frame); 599/097 aj>0. Stretch <10 unmet.
See `specfence-abc-iter27-status.md`. Iter 28 cause: 597 first-frame tip identity
(hang-free nested apply or capture-at-tx.to) for aj>0, or schedule ≤12.1→<10.

### Iter 28 — first-frame tip identity diagnosed (nested apply hung)
597 Bind tips are router→token nested (code_hash refuse on frame0). First-frame-only
capture starved tip≡FF (wall↑); hang-free defer-until-match nested apply **hung**
under concurrency (same family as Iter27). Production: LAST_SNAP TLS clear + skip
Lean attach_current_live_snap; tip≡FF overlap + steps_cap kept. SoftWait Soft=0;
597 N5 **13.7** / N10 **14.3** (noise≈ Iter27-rerun 14.1); aj=0 on 597; 599/097 aj>0.
See `specfence-abc-iter28-status.md`. Iter 29 cause: hang-free nested Bind consume
≠ frame_init defer, or schedule ≤12.1→<10.

### Iter 29 — hang-free nested Bind consume (credit + opt-in apply)
Hang-free nested path ≠ Iter28e frame_init-defer: stash on code_hash mismatch with
PENDING cleared; natural nested frame_init hash-match one-shot apply
(`SPECFENCE_NESTED_BIND=1` dig — hang-free). Default-on nested apply → Lean seq≠par
— OFF. Production: credit nested tips on code_hash refuse. Richer tip_compact
falsified (wall↑). SoftWait Soft=0; tip≡FF + steps_cap + LAST_SNAP kept.
597 N5 med **14.7** / N10 **13.9**; 599/097 aj>0; stretch <10 unmet.
See `specfence-abc-iter29-status.md`. Iter 30 cause: Lean-safe nested apply default
or 597 arm gates / schedule ≤12.1→<10.

### Iter 30 — Lean-safe nested apply default-on (wall↓ vs Iter29)
Named Iter29→30 gates: tip_sloads addr≡target ∧ depth≤2 ∧ tip≡FF; homogeneous-only
stash (broad stash wall↑ falsified); depth bypass ≤8→≤2; `SPECFENCE_NESTED_BIND`
default ON (opt-out `=0`). SoftWait Soft=0; tip≡FF + steps_cap + LAST_SNAP kept.
597 N5 med **13.4** / N10 **13.2** Soft=0 (↓ vs Iter29 14.7/13.9; min 12.1); 599/097 aj>0;
Lean seq≡par with default-on. Stretch median ≤12.1 / <10 unmet; 597 aj=0 (steps_over/origin_unsafe).
See `specfence-abc-iter30-status.md`. Iter 31 cause: 597 arm gates
(origin_unsafe/steps_over) or schedule ≤12.1→<10 — not fra≥16 / yield deepen /
SoftWait Soft / BO park / mass SNAP / Validated skip / broad nested stash.

### Three-pillar drop — Await@a + resolve arm + morph (post-Iter30 pause)
Landed coherent A∧B∧C default-on after pause-rethink (not Iter31 micro-gates).
Await@a on storm+program+live_fanout≥8 (SoftWait Soft=0); tip≡FF max_steps 8192 +
best deferred tip → **597 aj>0**; Quiet|Storm morph actuates. G7 mean SF/OCC **0.449**
(≈ tip 0.455); 597 wall abs↑ with slower OCC host; stretch &lt;10 unmet.
See `specfence-three-pillar-impl.md` + `specfence-three-pillar-reflect.md`. **STOP.**
