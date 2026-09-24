# SpecFence Avoid: Overlap, Not Serialize — Redesign SoT v1

**Date:** 2026-09-22 (Asia/Shanghai)  
**Kind:** redesign Source of Truth after Avoid-beats-OCC land-v1 **FAILED** the cost inequality.  
**No pevm code from this note’s author. No CloudAgent from this note’s author.**  
**Status:** supersedes the schedule-native Avoid vehicle in land-v1 (“hold sticky succ off `Q_indep` until pred done”) as the sole large Avoid.

**Parents / evidence:**
- [`specfence-avoid-beats-occ-abort-land-v1.md`](specfence-avoid-beats-occ-abort-land-v1.md) — FAILED: `chain_ab/c` up to **3.78×** but large TPS **0.44**; SF 17–30 ms vs OCC ~8 ms
- [`specfence-avoid-beats-occ-abort-v1.md`](specfence-avoid-beats-occ-abort-v1.md) — prior SoT (cost inequality still correct; vehicle falsified)
- [`specfence-sf-mvmemory-land-v4.md`](specfence-sf-mvmemory-land-v4.md) — large floor ~0.65; thin ~0.83
- [`specfence-sf-mvmemory-land-v5.md`](specfence-sf-mvmemory-land-v5.md) — Win HOLD / rem-skip / claim-wake falsified
- [`specfence-four-conflict-classes-v1.md`](specfence-four-conflict-classes-v1.md) — RAW/WAR/WAW/Chain concurrent
- [`specfence-sf-mvmemory-redesign-v1.md`](specfence-sf-mvmemory-redesign-v1.md) — SfMvMemory + VisibilityPolicy

**Harness (immutable):** Soft=0 Instant-off; focus `3356896` / `15274915`; bar **TPS SF/OCC ≥ 1.5 both**; MvMemory Estimate OCC-only.

**Co-author:** `0xstride <fengjy73@users.noreply.github.com>`

---

## 0. Five-line thesis

1. Land-v1 proved **Avoid can raise `chain_ab/c` (≥3× on one iter)** while still **losing wall to OCC**: holding every sticky successor off all queues until pred **done** turns the critical chain into a **single-core serial walk** of wall time (~68 hops × hop latency).
2. Required Avoid: **overlap** — successor may run antichain / independent work while waiting; when WaitOnce on the chain fires, only the *access* waits for true publish — **not** the whole tx parked off `Q_indep`/`Q_released`/`Q_ordered` from admission.
3. Levers: **early SfMvMemory Data/version tip** so WaitOnce completes mid-pred-exec (no full pred Commit); **PC** fills remaining cores with antichain while one hop waits; **Learn** keeps WaitOnce on sticky≥32 **without** Win Fence prepaid.
4. Four classes RAW/WAR/WAW/Chain stay concurrent Detect|Avoid|Resolve. Forbid: full-tx serialize-until-pred-done as sole large Avoid (falsified); Win HOLD Fence; Estimate Block; ordinal strip.
5. Acceptance: Soft=0 Instant-off N≥5 both TPS≥1.5; large must **not regress below v4 ~0.65** while climbing; `chain_ab/c`≥3× **with** SF wall ≤ OCC/1.5.

---

## 1. What land-v1 falsified

### 1.1 Measured outcome (Soft=0 Instant-off N=5 @8, tip `1f7213e`)

| Block | v4 best | Avoid land-v1 reuse med TPS | Notes |
|------:|--------:|----------------------------:|-------|
| 3356896 | ~0.83 | **0.74** | no antichain-width win; calm SF≳OCC |
| 15274915 | ~0.65 | **0.44** | **regression**; SF 17–30 ms vs OCC ~8 ms |

Large reuse detail (land-v1):

| Iter | SF ms | TPS | chain_ab/c | chain_n |
|-----:|------:|----:|-----------:|--------:|
| 0 | 21.6 | 0.45 | — | 68 |
| 1 | 17.1 | 0.47 | 1.73× | 68 |
| 2 | 30.0 | 0.30 | 1.59× | 68 |
| 3 | 25.3 | 0.31 | 1.91× | 68 |
| 4 | 17.2 | 0.44 | **3.78×** | 68 |

`seq=par`, `occ_picks=0`, `estimate_block_sf=0`, four-class + WAR held. Avoid **worked** as Detect|Avoid (FullReplay fell; one iter `chain_ab/c` 3.78×). The cost inequality still failed.

### 1.2 Root cause (first principles)

```
land-v1 Avoid vehicle:
  sticky succ with blocking_producer
    → stay ST_WAIT
    → NOT pushed to Q_indep (nor runnable antichain)
    → whole tx off all queues until pred done
```

That is **full-tx serialize-until-pred-done**. On a sticky spine of length ~68:

- One core walks hop₀ → done → hop₁ → done → … → hop₆₇.
- Idle cores starve for antichain fill that those parked successors never contribute.
- OCC abort-parallel finishes the same block sooner (median ~8 ms) even with more Resolve aborts.

**Falsified claim:** “schedule Avoid = hold entire successor tx until pred done” as the **only** large Avoid.

**Preserved claim:** `cost(schedule Avoid) < cost(OCC abort train)` remains the acceptance language — the **vehicle** must change.

### 1.3 What not to reopen

Win HOLD / Fence prepaid · Estimate Block · ordinal / Opt `access_log` strip · mark_gated · thin Rewind · long Chain busy-spin · rem/vis/metrics micro-skip · claim-wake-only as ≥3× lever · another “succ off-queue until pred done” land.

---

## 2. Required design: Overlap Avoid

**Principle:** Avoid arms the **dependent access**, not the **whole transaction’s presence on the ready set**. Successors stay schedulable for independent / antichain work; only the WaitOnce access stalls on true publish.

### 2.1 Overlap (successor runs while waiting)

| Rule | Meaning |
|------|---------|
| **Admit successor** | Sticky successor may enter exec / remain on PC steal sets for work that does **not** touch the blocked `(ℓ,w)`. |
| **Access waits** | When the chain Read/Write hits WaitOnce, **that access** waits for true Data/OrderedTip publish — not “tx never scheduled until pred Commit”. |
| **Antichain fill** | Work independent of the live hop runs on remaining cores **while** one hop WaitOnce is armed. |
| **Never sole Avoid** | Do **not** implement large Avoid solely as “hold entire succ tx off `Q_*` until pred done”. |

```
overlap Avoid (required):
  succ admitted / runnable for antichain & independent accesses
  on WaitOnce(ℓ, w):
      stall *this access* until SfMvMemory tip Data|OrderedTip for (ℓ,w)
      # NOT: park whole tx off all queues from pick time
  on publish: exact wake → resume access (Q_released only if needed for resume)

serialize Avoid (falsified as sole large Avoid):
  sticky succ → ST_WAIT → off Q_indep until pred done
  → single-core spine wall > OCC abort-parallel
```

### 2.2 Early SfMvMemory Data/version tip

WaitOnce must complete **mid-pred-exec**, not after full pred Commit / `record` batch end.

```
1. install_tip_early(ℓ, w, Data|OrderedTip)   # NOT Estimate
2. on write complete for ℓ:
     tip → clear live_writer → wake exact waiters
3. VisibilityPolicy: Opt | WaitReleased | OrderedTip only
4. WaitOnce consume-hit on published tip mid-pred-exec
```

- Large: OrderedTip / WaitReleased along sticky spine; one-hop exact wake.
- Thin: early tip so WaitOnce never Opt-falls to Storage pre-state while writer unpublished.
- Counters: `estimate_block_sf=0`, `sf_early_tip_install`, `sf_exact_wake`, WaitOnce mid-exec hit rate.

### 2.3 PC: fill remaining cores while one hop waits

| Ideal @8 | Policy |
|----------|--------|
| ≤1 core | current longest WaitOnce hop (or advancing true publish) |
| rest | antichain / independent / Q_released resume — **never idle because sticky succs were held off-queue** |
| Ready | `argmax remaining_crit_work`; prefer Q_released then Q_indep; no reverse-chain launch; no coinbase false-head; no re-pick gated; no `next_task*` |

**Explicit anti-pattern (land-v1):** prefer `Q_released` then `Q_indep` **while** sticky successors with `blocking_producer` are never enqueued → antichain empty → serialize.

**Required:** sticky successors remain eligible for PC fill of **non-blocked** work; WaitOnce is access-grain, not tx-grain park.

### 2.4 Learn: WaitOnce on sticky≥32 without Win Fence prepaid

- Legal AccessArm: Opt | WaitOnce | NeverWait only.
- Sticky ≥32 / `avoid_hold` spine: **preserve WaitOnce** across Learn; writers HOLD into `last_location_writers` remains.
- **Forbid** Learn demote sticky Win/WaitOnce → Opt under wall pressure.
- **Forbid** Win OrderedAdmit / Fence refuse prepaid as Avoid vehicle (v5 falsified).
- IntraPatch on first EffectiveWAW/RAW: subsequent same-key → WaitOnce immediately.
- Early basic-WAW templates: first touch WaitOnce, not whole-tx Win_N.

### 2.5 Four classes still concurrent

| Class | Overlap Avoid (this SoT) |
|-------|--------------------------|
| **RAW** | Early Data tip; WaitOnce on *read access*; succ may run other work; prefix Resolve from `fail_k` if assumption fails |
| **WAR** | Admission-time reader reserve / version protect without Fence; Detect overwrite → defer or version writer; repair affected access — not schedule-absorbed-only |
| **WAW** | OrderedTip one-hop; succ waits **true pred publish** at access; continue from `fail_k`; thin needs width not shell cut |
| **Chain** | AccessArm WaitOnce without Fence prepaid; plant→Version→Released→access resume; PC ≤1 core on hop, rest antichain; **no** full-tx off-queue serialize |

Detect|Avoid|Resolve remain concurrent. Estimate Block forbidden; MvMemory Estimate stays OCC-only.

### 2.6 Cost inequality (revised operational proxy)

```
cost(overlap Avoid | class)  <  cost(OCC abort train | class)
```

**Large Chain (must hold jointly):**

1. `chain_ab / chain_c` **≥ 3×** (Avoid/release beats Resolve-abort) — land-v1 already hit this once.
2. **AND** SF wall **≤ OCC/1.5** (so TPS ≥1.5) — land-v1 **failed** this; serialize Avoid cannot satisfy both.
3. Large reuse med TPS must **not regress below v4 ~0.65** on the climb (floor while iterating toward ≥1.5).

**Thin:** calm SF ≲ OCC/1.5 **or** measured antichain-width lift delivering TPS≥1.5; full `access_log.note` kept.

---

## 3. Explicit forbid list

| Forbid | Why |
|--------|-----|
| **Full-tx serialize-until-pred-done as sole large Avoid** | land-v1: `chain_ab/c`↑ but SF wall 17–30 ms ≫ OCC ~8; single-core spine |
| **Win HOLD / Fence prepaid** | v5: Detect stayed, wall exploded |
| **Per-ℓ / Opt `access_log` ordinal strip** | fail_k gaps → FullReplay storm |
| **Estimate Block** on SF path | OCC-only; `estimate_block_sf=0` |
| **Thin Rewind / mark_gated / long Chain busy-spin** | discarded theater |
| **claim-wake alone / rem micro-skip** | exhausted; no ≥1.5 path |
| **Learn demote sticky WaitOnce→Opt / pe-without-Avoid** | Opt abort train dominance |
| **Blocking park / SoftWait≠0** into interpreter | Soft=0 Instant-off |
| **Parking sticky succ off all `Q_*` from pick** as the Avoid definition | same as serialize vehicle |

**Allowed (continue):** sticky **writers** HOLD ≥32 on large; Prefer Rewind / prefix from `fail_k`; WaitOnce-only OCC-shaped gate on thin; full `access_log.note`; SfMvMemory early tip + exact wake; NeverWait for beneficiary / basic_lazy; **access-grain** WaitOnce with antichain overlap.

---

## 4. Acceptance

Harness: Soft=0 Instant-off (`env -u SPECFENCE_HANG_TRACE`), 8 cores, `SPECFENCE_COMPARE_CHECK=1`, **N≥5 both** `3356896` and `15274915`.

| # | Criterion |
|---|-----------|
| A | Soft=0; `seq≡par`; `occ_picks=0`; `soft_wait_arms=0`; `explore=0`; no hang |
| B | **TPS SF/OCC ≥ 1.5** on **every** accepted focus-block result for **both** blocks |
| C | Large climb: reuse med TPS **never below v4 ~0.65** while iterating; final ≥1.5 |
| D | **`estimate_block_sf=0`**; no Win HOLD Fence prepaid |
| E | Four-class audit RAW/WAR/WAW/Chain + WAR first-class; **b ≫ c** per class |
| F | Large: `chain_ab/c` **≥ 3×** median reuse **and** SF wall **≤ OCC/1.5**; sticky `chain_n` not collapsed; Learn keeps WaitOnce |
| G | Thin: calm SF ≲ OCC/1.5 **or** proven antichain-width lift → TPS≥1.5; ordinals kept |
| H | WaitOnce consume hits **Data/version tip mid-pred-exec** (not only post-Commit); Estimate tip hits on SF = 0 |
| I | Instrumentation proves sticky successors **contribute antichain / independent exec** while a WaitOnce access is armed (overlap evidence — not serialize) |
| J | Lib release + arch SoftWait=0 seq≡par checks pass |

**Not success:** TPS&lt;1.5 with Soft=0 held; `chain_ab/c`≥3× with SF wall ≫ OCC (land-v1 pattern); large TPS &lt;0.65 regress.

---

## 5. Full-batch land checklist (for CloudAgent — separate launch)

Coordinator launches CloudAgent; this note does **not**. Land is an **overlap Avoid redesign**, not another off-queue serialize or rem/Win HOLD cut.

### 5.0 Preflight

1. Read this SoT + Avoid-beats-OCC land-v1 (FAILED) + prior Avoid SoT + four-class v1 + SfMvMemory redesign + land-v4/v5.  
2. Branch from current true-spine tip (PR #45 lineage); **revert or replace** land-v1 “succ off `Q_indep` until pred done” as sole Avoid.  
3. Baseline Soft=0 Instant-off N=5 both vs v4 (~0.83 / ~0.65) and land-v1 (~0.74 / ~0.44); record TPS, wall ms, `chain_ab/c`, antichain fill, `estimate_block_sf`.

### 5.1 Overlap Avoid plane (primary)

4. Replace tx-grain park with **access-grain WaitOnce**: succ schedulable for independent work; stall only at blocked access.  
5. Prove with counters: sticky successors execute non-blocked work while WaitOnce armed; idle-core time on large spine falls vs land-v1.  
6. Harden `SfMvMemory`: early `install_tip_early` + publish mid-exec; WaitOnce completes without full pred Commit.  
7. Fence SF path: zero Estimate Block; Admission WaitOnce **without** Fence / Win OrderedAdmit prepaid.  
8. Exact waiter wake per `(reader,ℓ,w)`; NeverWait ℓ never register.

### 5.2 Learn / sticky chain

9. Keep WaitOnce on sticky ≥32 / `avoid_hold`; stop demotion to Opt.  
10. Sticky **writers** HOLD into `last_location_writers`; Prefer Rewind / prefix Resolve.  
11. IntraPatch first Effective conflict → WaitOnce before next same-key access.

### 5.3 PC / width

12. Ideal @8: ≤1 core on current WaitOnce hop; rest antichain / Q_released / Q_indep.  
13. Ready = longest remaining crit; no reverse launch; no re-pick gated.  
14. Thin: measure antichain fill vs WAW spine; do not strip ordinals.

### 5.4 Four-class + WAR

15. Instrument RAW/WAR/WAW/Chain Detect|Avoid|Resolve; prove WAR not schedule-absorbed-only.  
16. Target **b ≫ c** per class; large `chain_ab/c`≥3× **with** SF wall ≤ OCC/1.5.

### 5.5 Verify / note / sync

17. Soft=0 Instant-off N≥5 both; TPS tables lead; wall ms secondary.  
18. Prove no serialize-only Avoid; no Win HOLD Fence; `estimate_block_sf=0`; large TPS ≥ v4 floor on climb.  
19. Land result note under `lab/notes/`; sync via `lab/scripts/sync-to-github.sh`.  
20. If TPS&lt;1.5: revise overlap cost model — **do not** return to full-tx off-queue serialize, ordinal strip, or Win HOLD.

### 5.6 Out of scope for this land

- pevm edits from this SoT’s author  
- SoftWait≠0 / Instant-on  
- Mixed-49 as acceptance (focus pair only)  
- Deleting OCC Estimate for OCC workers  
- Re-landing “hold sticky succ off queue until pred done” as sole Avoid  
- Another rem/tip/Win HOLD micro-cut PR

---

## 6. Pointers

| Topic | SoT |
|-------|-----|
| Failed serialize Avoid | [`specfence-avoid-beats-occ-abort-land-v1.md`](specfence-avoid-beats-occ-abort-land-v1.md) |
| Prior cost-inequality SoT | [`specfence-avoid-beats-occ-abort-v1.md`](specfence-avoid-beats-occ-abort-v1.md) |
| Version/visibility plane | [`specfence-sf-mvmemory-redesign-v1.md`](specfence-sf-mvmemory-redesign-v1.md) |
| Four classes | [`specfence-four-conflict-classes-v1.md`](specfence-four-conflict-classes-v1.md) |
| v4 floor | land-v4 thin ~0.83 / large ~0.65 |

**This file is SoT for: Avoid must overlap antichain with WaitOnce access-waits — never sole full-tx serialize-until-pred-done.** Land-v1 falsified serialize; the cost inequality still stands.
