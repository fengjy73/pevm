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
