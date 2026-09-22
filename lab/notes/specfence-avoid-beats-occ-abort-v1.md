# SpecFence Avoid Beats OCC Abort — Redesign SoT v1

**Date:** 2026-09-22 (Asia/Shanghai)  
**Kind:** redesign Source of Truth — **not** another rem/tip/Win HOLD micro-cut land.  
**No pevm code from this note’s author. No CloudAgent from this note’s author.**  
**Status:** replaces micro-cut plateau narrative after SfMvMemory land v1–v5.

**Parents / evidence:**
- [`specfence-sf-mvmemory-land-v5.md`](specfence-sf-mvmemory-land-v5.md) — plateau declared; Win_2 HOLD / rem-skip / claim-wake falsified
- [`specfence-sf-mvmemory-land-v4.md`](specfence-sf-mvmemory-land-v4.md) — best thin ~0.83; WaitOnce-only gate; sticky writers HOLD + Prefer Rewind
- [`specfence-four-conflict-classes-v1.md`](specfence-four-conflict-classes-v1.md) — RAW/WAR/WAW/Chain; concurrent Detect|Avoid|Resolve
- [`specfence-sf-mvmemory-redesign-v1.md`](specfence-sf-mvmemory-redesign-v1.md) — SfMvMemory + VisibilityPolicy plane
- [`specfence-cc-pc-first-principles-redesign-v3.md`](specfence-cc-pc-first-principles-redesign-v3.md) — CC/PC first principles (skim)

**Harness facts (immutable for this SoT):** Soft=0 Instant-off focus `3356896` / `15274915`; bar **TPS SF/OCC ≥ 1.5 both**; MvMemory Estimate is **OCC-only**.

**Co-author:** `0xstride <fengjy73@users.noreply.github.com>`

---

## 0. Five-line thesis

1. Micro-cuts v1–v5 plateaued because on thin the SpecFence shell is still **more expensive than OCC’s abort train**, and on large Opt→FullReplay/Rewind remains **cheaper than Fence / Win prepaid Avoid**.
2. Hitting TPS≥1.5 requires redesign so **schedule Avoid is cheaper than the OCC abort train** for each of RAW / WAR / WAW / Chain — not more rem/tip strip or Win HOLD.
3. Concrete levers: **SfMvMemory early Data/version publish + exact wake**; **Admission WaitOnce without Fence prepaid**; **Learn must not demote sticky Win→Opt on chain**; **PC width vs WAW/spine serialization**.
4. Explicit forbid: Win HOLD Fence prepaid, ordinal/`access_log` strip, Estimate Block, thin Rewind, mark_gated, long Chain busy-spin, pe-without-Avoid.
5. Acceptance: Soft=0 Instant-off N≥5 both blocks TPS≥1.5 **and** four-class audit with Avoid (b) ≫ Resolve-abort (c).

---

## 1. Why micro-cuts plateaued

### 1.1 Measured ceiling (Soft=0 Instant-off)

| Block | Best reuse med TPS | Land | Gap to ≥1.5 |
|------:|-------------------:|:----:|:-----------:|
| 3356896 (thin) | **~0.83** | v4 | ~1.8× |
| 15274915 (large) | **~0.72** | v2 | ~2.1× |
| v5 confirmatory (v4 tip restored) | thin ~0.74 / large ~0.59 | v5 | worse / noise |

v5 falsified the remaining v4 candidates (Win_2 HOLD, rem/vis/metrics skip, claim-wake alone) and restored v4 tip. Further rem/tip/Win HOLD work is **exhausted**.

### 1.2 Thin (`3356896`): OCC abort train cheaper than SF shell

- Calm reuse: SF ≈ **1.4 ms**, OCC ≈ **1.1–1.2 ms** → TPS~0.8 even with WaitOnce-only OCC-shaped gate.
- Need SF ≲ OCC/1.5 ≈ **0.8 ms** for ≥1.5 — ~40% absolute wall cut unavailable from rem/tip/vis/metrics.
- Remaining thin wall: **`access_log.note` all ℓ** (forbidden to strip; ordinal strip → fail_k gaps → FullReplay storm) + **~17-writer WAW spine serialization**.
- Calm `(c)=0` still SF ≳ OCC: SpecFence pays shell tax with **no conflict work left to Avoid**. OCC’s path (optimistic race + cheap abort when needed) wins the empty-conflict case.
- **Plateau diagnosis:** beating OCC on thin is not “cheaper SpecFence shell”; it is **more antichain / break WAW serialization**, or Accept that Soft=0 ordinal+spine ceiling ~0.8–1.0 under current constraints.

### 1.3 Large (`15274915`): Opt path cheaper than Fence WaitOnce

- Sticky writers HOLD keeps `chain_n≈60–62` (necessary); Prefer Rewind fires (`resolve_rewind` 37–94/iter) but FullReplay still 40–103.
- `chain_ab/c` stuck **~1.5–2.0×** ≪ target ≥3×.
- **Learn demotes sticky Win→Opt** on wall ℓ (`abd6bb…`): Avoid ratio never jumps; Opt→validate→FullReplay/Rewind **dominates wall** (~8–11 ms vs OCC ~5–6 ms).
- Win_2 HOLD after Learn (v5): Detect/Avoid stayed, but **Fence/refuse prepaid raised SF wall** (reuse SF ~9–14 ms); TPS ~0.59 worse. **Win HOLD ≠ free Avoid.**
- Claim-wake alone: no ≥3× lift on `chain_ab/c`.
- **Plateau diagnosis:** OCC’s Opt abort train is still the cheaper Resolve path. SpecFence must make **schedule-native Avoid** (plant → Version → Released → succ runnable) cheaper than that train — **without** Blocking park, Win OrderedAdmit prepaid, or Estimate Block.

### 1.4 Cumulative discard (do not reopen as micro-cuts)

Estimate Block · thin Rewind · 15-hold · mark_gated · long Chain busy-spin · Opt / per-ℓ `access_log` skip · sticky **Win** HOLD after Learn · claim-wake-only as ≥3× lever · rem/vis/metrics micro-skip as path to thin SF ≲1.0 ms.

---

## 2. Required redesign: schedule Avoid cheaper than OCC abort train

**Principle:** For every true edge, the cost of **arming Avoid before admission** (wait/version/order/one-hop gate + antichain fill) must be **strictly less** than OCC’s cost of optimistic exec → validate fail → abort → incarnation++ → re-exec. Detect|Avoid|Resolve remain concurrent; Estimate Block is forbidden; MvMemory Estimate stays OCC-only.

### 2.1 Per-class mechanisms (RAW / WAR / WAW / Chain)

| Class | Why OCC abort wins today | Avoid-cheaper-than-abort mechanism |
|-------|--------------------------|-------------------------------------|
| **RAW** | Late Data at OCC `record` end → WaitOnce misses publish → Opt → FullReplay | **SfMvMemory early Data/version tip** + WaitReleased read **only after publish**; exact waiter wake → Q_released; prefix Resolve from `fail_k` if assumption fails |
| **WAR** | Writer clobber / schedule-only absorption; no cheap reader protection | **Admission-time reader reserve / version protect** without Fence prepaid; Detect overwrite → defer or version writer; repair only affected access/prefix — never treat WAR as schedule-absorbed-only |
| **WAW** | Opt tip race → EffectiveWAW → Full/Ordered from 0 | **OrderedTip one-hop**: install ordered tip early; successor waits **true predecessor publish only**; continue from `fail_k`; no repeat Blocking; thin needs **width** (antichain slots) not cheaper shell |
| **Chain** | Learn nails sticky ℓ → Opt; Opt→FullReplay/Rewind wall; Win HOLD prepaid | **Admission WaitOnce without Fence prepaid**; plant→Version→Released→succ runnable; **Learn must not demote sticky Win→Opt** on chain templates; Prefer prefix Rewind over FullReplay; PC: ≤1 core on next hop, rest fill antichain |

### 2.2 Concrete levers (ship these — not rem strip)

#### A. SfMvMemory publish (shared speed plane)

```
1. install_tip_early(ℓ, w, Data|OrderedTip)   # NOT Estimate
2. on complete: write tip → clear live_writer → wake exact waiters → Q_released
3. VisibilityPolicy: Opt | WaitReleased | OrderedTip only
```

- Thin: early tip so WaitOnce never Opt-falls through to Storage pre-state while writer unpublished.
- Large: OrderedTip / WaitReleased along sticky spine; exact one-hop wake (not claim-wake theater alone).
- Counters: `estimate_block_sf=0`, `sf_early_tip_install`, `sf_exact_wake`, WaitOnce consume-hit on published tip.

#### B. Admission WaitOnce **without** Fence prepaid

- Arm WaitOnce at admission when true live pred exists (or sticky chain template says WaitOnce).
- **Do not** enter Win OrderedAdmit / Fence refuse prepaid as the Avoid vehicle (v5 falsified).
- Thin Soft=0: Prefer defer-pick / short help-spin / Q_released after publish — never Blocking park, never SoftWait into interpreter.
- Same `(tx,ℓ,w)` WaitOnce **at most once**; publish/done must release.

#### C. Learn: must not demote sticky Win→Opt on chain

- Legal Learn outputs: **AccessArm** Opt | WaitOnce | NeverWait only (no pe-without-Avoid).
- On large sticky ≥32 / `avoid_hold` spine: **preserve WaitOnce** (or equivalent chain Avoid arm) across Learn; writers HOLD into `last_location_writers` remains.
- **Forbid** Learn nail that converts sticky chain ℓ Win/WaitOnce → Opt when wall pressure rises — that is the Opt→FullReplay dominance path.
- Learn “early basic-WAW @ k_bucket” templates so next hop **first touch is WaitOnce**, not whole-tx Win_N.
- IntraPatch on first EffectiveWAW/RAW: subsequent same-key → WaitOnce immediately.

#### D. PC width vs spine

- Thin WAW spine (~17 writers / 176 txs): SF≪OCC needs **more antichain fill**, not shell tax cuts. Runnable antichain must occupy idle cores while one worker advances the next true hop.
- Large: Ready = `argmax remaining_crit_work`; prefer Q_released (unlock chain) then Q_indep (fill cores). Forbid reverse-chain launch and coinbase false-head.
- Ideal @8: ≤1 core on current longest hop; rest on independent / released work. Never re-pick gated tx; never `next_task*`.

### 2.3 Cost inequality (acceptance language)

For each class on focus pair Soft=0 Instant-off:

```
cost(schedule Avoid | class)  <  cost(OCC abort train | class)
```

Operational proxy (large Chain): **`chain_ab / chain_c` ≥ 3×** (Avoid/release beats Resolve-abort).  
Operational proxy (all classes): four-class audit **b ≫ c** (see §4).  
Thin: calm SF wall must fall below OCC/1.5 **or** measured antichain width must rise enough that SF TPS ≥1.5 despite spine — shell-only cuts do not count.

---

## 3. Explicit forbid list

| Forbid | Why |
|--------|-----|
| **Win HOLD / Fence prepaid** as Avoid vehicle | v5: Detect stayed, wall exploded; Win HOLD ≠ free Avoid |
| **Per-ℓ / Opt `access_log` ordinal strip** | fail_k gaps → FullReplay storm; ordinals mandatory |
| **Estimate Block** on SF path | Estimate is OCC-only; `estimate_block_sf` must stay 0 |
| **Thin Rewind** | Discarded; Prefer Avoid-before-fail via early tip |
| **mark_gated** | Discarded theater |
| **Long Chain busy-spin** | Spin theater; use publish→Q_released |
| **15-hold / thin sticky hold** | Wrong block; large sticky ≥32 only |
| **claim-wake alone** as ≥3× lever | Measured; no Avoid-ratio lift without cheaper Avoid |
| **rem / vis / metrics micro-skip** as path to thin ≥1.5 | Exhausted; calm SF still ≳ OCC |
| **pe-without-Avoid / sticky Opt as Learn output** | Converts Avoid into abort train |
| **Learn demote sticky Win→Opt on chain** | Root of large Opt wall |
| **Blocking park / SoftWait≠0** into interpreter | Soft=0 Instant-off harness |
| **Broad `rset_w` / post-FullReplay plant / coinbase waiters** | False edges; fake chain |
| **OCC `MvMemory` Estimate as VisibilityPolicy** | Not a SpecFence variant |

Allowed (continue): sticky **writers** HOLD ≥32 on large; Prefer Rewind / prefix from `fail_k`; WaitOnce-only OCC-shaped gate on thin; full `access_log.note`; SfMvMemory early tip + exact wake; NeverWait for beneficiary / basic_lazy.

---

## 4. Acceptance

Harness: Soft=0 Instant-off (`env -u SPECFENCE_HANG_TRACE`), 8 cores, `SPECFENCE_COMPARE_CHECK=1`, **N≥5 both** `3356896` and `15274915`.

| # | Criterion |
|---|-----------|
| A | Soft=0; `seq≡par`; `occ_picks=0`; `soft_wait_arms=0`; `explore=0`; no hang |
| B | **TPS SF/OCC ≥ 1.5** on **every** accepted focus-block result for **both** blocks |
| C | **`estimate_block_sf=0`**; SF Avoid/consult/decide never branches on Estimate |
| D | **Four-class audit** RAW/WAR/WAW/Chain + WAR first-class: for each class record Detect / Avoid arm / Resolve; require **b ≫ c** (Avoid/release counts ≫ abort/FullReplay-from-0 counts) |
| E | Large: `chain_ab/c` **≥ 3×** median reuse; sticky writers HOLD intact (`chain_n` not collapsed 62→4); Learn does **not** nail sticky chain → Opt |
| F | Thin: calm SF ≲ OCC/1.5 **or** proven antichain-width lift delivering TPS≥1.5; full `access_log.note` kept |
| G | SF WaitOnce/WaitReleased/OrderedTip consume **Data/version tip**; Estimate tip hits on SF path = 0 |
| H | Lib release + arch SoftWait=0 seq≡par checks pass |

**Not success:** any reuse med TPS < 1.5 with Soft=0 held. Soft success below 1.5 is forbidden language (v5).

---

## 5. Full-batch land checklist (for CloudAgent — separate launch)

Coordinator launches CloudAgent; this note does **not**. Land is a **redesign batch**, not rem/tip/Win HOLD.

### 5.0 Preflight

1. Read this SoT + four-class v1 + SfMvMemory redesign v1 + land-v4/v5.  
2. Branch from current true-spine tip (PR #45 lineage); do not reopen discarded micro-cuts.  
3. Baseline Soft=0 Instant-off N=5 both; record TPS, `chain_ab/c`, `estimate_block_sf`, four-class counters.

### 5.1 Version / Avoid plane (primary)

4. Harden `SfMvMemory`: `read(ℓ, Opt|WaitReleased|OrderedTip)`; `install_tip_early` + `publish` (tip → clear live_writer → exact wake).  
5. Fence SF path: zero Estimate Block / Estimate branch; leave OCC Estimate for baseline workers.  
6. Admission WaitOnce **without** Fence / Win OrderedAdmit prepaid.  
7. Waiter sets per ℓ; NeverWait ℓ never register; WakeOnce per `(reader,ℓ,w)`.

### 5.2 Learn / sticky chain

8. Stop Learn demotion sticky Win→Opt on chain / `avoid_hold` ≥32 spine; output AccessArm WaitOnce templates.  
9. Keep sticky **writers** HOLD into `last_location_writers`; Prefer Rewind / prefix Resolve.  
10. IntraPatch first Effective conflict → WaitOnce before next same-key access.

### 5.3 PC / width

11. Q_indep / Q_released / Q_ordered steal; Ready = longest remaining crit; no reverse launch; no re-pick gated.  
12. Thin: measure antichain fill vs WAW spine; do not strip ordinals for wall.

### 5.4 Four-class + WAR

13. Instrument RAW/WAR/WAW/Chain Detect|Avoid|Resolve counters; prove WAR not schedule-absorbed-only.  
14. Target **b ≫ c** per class; large `chain_ab/c` ≥3×.

### 5.5 Verify / note / sync

15. Soft=0 Instant-off N≥5 both; TPS tables lead; wall ms secondary.  
16. Prove `estimate_block_sf=0` and no Win HOLD Fence prepaid regress.  
17. Land result note under `lab/notes/`; sync via `lab/scripts/sync-to-github.sh`.  
18. If TPS < 1.5: **stop micro-cutting**; revise Avoid cost model — do not strip `access_log` or re-try Win HOLD.

### 5.6 Out of scope for this land

- pevm edits from this SoT’s author  
- SoftWait≠0 / Instant-on  
- Mixed-49 as acceptance (focus pair only)  
- Deleting OCC Estimate for OCC workers  
- Another rem/tip/Win HOLD micro-cut PR

---

## 6. Pointers

| Topic | SoT |
|-------|-----|
| Version/visibility plane | [`specfence-sf-mvmemory-redesign-v1.md`](specfence-sf-mvmemory-redesign-v1.md) |
| Four classes + concurrent Detect\|Avoid\|Resolve | [`specfence-four-conflict-classes-v1.md`](specfence-four-conflict-classes-v1.md) |
| CC/PC first principles | [`specfence-cc-pc-first-principles-redesign-v3.md`](specfence-cc-pc-first-principles-redesign-v3.md) |
| Plateau evidence | land-v4 / land-v5 |

**This file is SoT for: why micro-cuts stop, and what redesign makes schedule Avoid beat the OCC abort train.** Do not fold another rem/Win HOLD experiment into this narrative.
