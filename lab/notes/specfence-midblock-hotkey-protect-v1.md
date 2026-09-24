# SpecFence Mid-Block Hot-Key Protect — Redesign SoT v1

**Date:** 2026-09-22 (Asia/Shanghai)  
**Kind:** redesign Source of Truth after schedule-only Avoid vehicles **failed** vs the OCC abort train.  
**No pevm code from this note’s author. No CloudAgent from this note’s author.**  
**Status:** supersedes serialize-spine Avoid and overlap access-wait as the primary path to TPS≥1.5; preserves the cost inequality, changes the Learn→Avoid vehicle.

**Parents / evidence:**
- [`specfence-avoid-overlap-land-v1.md`](specfence-avoid-overlap-land-v1.md) — FAILED: overlap land reuse med **0.54 / 0.55** (under v4 floor)
- [`specfence-avoid-overlap-not-serialize-v1.md`](specfence-avoid-overlap-not-serialize-v1.md) — overlap SoT; vehicle falsified by land
- [`specfence-avoid-beats-occ-abort-land-v1.md`](specfence-avoid-beats-occ-abort-land-v1.md) — FAILED: serialize spine; `chain_ab/c`↑ but large TPS **0.44**
- [`specfence-avoid-beats-occ-abort-v1.md`](specfence-avoid-beats-occ-abort-v1.md) — cost inequality still correct
- [`specfence-sf-mvmemory-land-v4.md`](specfence-sf-mvmemory-land-v4.md) — best thin ~0.83 / large ~0.65 (restore target if tip worse)
- [`specfence-sf-mvmemory-land-v5.md`](specfence-sf-mvmemory-land-v5.md) — Win HOLD / rem-skip / claim-wake falsified
- [`specfence-four-conflict-classes-v1.md`](specfence-four-conflict-classes-v1.md) — RAW/WAR/WAW/Chain; concurrent Detect|Avoid|Resolve
- [`specfence-focus-pair-tps-bar-v1.md`](specfence-focus-pair-tps-bar-v1.md) — Soft=0 Instant-off TPS≥1.5 both
- [`specfence-sf-mvmemory-redesign-v1.md`](specfence-sf-mvmemory-redesign-v1.md) — SfMvMemory + VisibilityPolicy

**Harness (immutable):** Soft=0 Instant-off; focus `3356896` / `15274915`; bar **TPS SF/OCC ≥ 1.5 both**; MvMemory Estimate OCC-only.

**Co-author:** `0xstride <fengjy73@users.noreply.github.com>`

---

## 0. Five-line thesis

1. **Schedule-only changes cannot beat the OCC abort train.** Serialize spine and overlap access-wait both raised Avoid theater (or antichain fill) while SF wall stayed ≫ OCC/1.5.
2. **Core:** handle conflicts **before** they collide — arm Avoid on hot ℓ as soon as mid-block evidence exists — so later txs **do not repeatedly Opt-discover → FullReplay**.
3. **Dynamic learning (same block):** first EffectiveWAW / hot evidence on ℓ → IntraPatch AccessArm **WaitOnce** (or OrderedTip) for remaining txs that will touch ℓ **this block**; protect **before** their Opt read.
4. **Lock-like** means SpecFence **publish-order Avoid** (WaitOnce + SfMvMemory **true tip**) — **not** a mutex and **not** OCC Estimate Block.
5. Acceptance: Soft=0 Instant-off N≥5 both TPS≥1.5; if current tip is worse than v4 (~0.83 / ~0.65), **restore toward v4 tip first** (overlap land 0.54/0.55 is not a floor to climb from blindly).

---

## 1. User correction (2026-09-22) — direction after schedule/overlap failed

After schedule Avoid and overlap Avoid lands failed vs OCC abort:

| Claim | Status |
|-------|--------|
| Schedule-only changes beat OCC abort train | **Falsified** |
| Core = cheaper shell / rem tax / Win prepaid | **Falsified** |
| Core = handle conflicts **before** collide so later txs need not re-execute | **Required** |
| When keys become hot **earlier in the same block**, promptly protect them (lock-like Avoid) so later accesses do not Opt-discover again | **Required** |

This SoT is that correction, written as landable redesign language.

---

## 2. Contrast: falsified paths

| Path | What it tried | Measured / why it fails vs OCC abort |
|------|---------------|--------------------------------------|
| **Serialize spine** | Hold sticky succ off all `Q_*` until pred done | land-v1: `chain_ab/c` up to 3.78× but large TPS **0.44**; single-core walk of ~68 hops ≫ OCC abort-parallel (~8 ms) |
| **Overlap access-wait** | Admit succ; stall only at chain access; early Data + fill | overlap land: thin **0.54** / large **0.55**; successors still Opt-hit unpublished ℓ → park/replay; window too late to beat abort train |
| **Rem tax strip** | Skip rem/vis/metrics / ordinal `access_log` | Exhausted by v4–v5; ordinal strip → fail_k gaps → FullReplay storm; calm SF still ≳ OCC |
| **Win Fence prepaid** | Win_2 HOLD / OrderedAdmit refuse as Avoid | v5: Detect stayed, SF wall exploded; Win HOLD ≠ free Avoid |

**Preserved:** `cost(Avoid) < cost(OCC abort train)` as acceptance language.  
**Discarded as primary vehicles:** schedule-only serialize, overlap-only access-wait, rem strip, Win Fence prepaid.

```
falsified (schedule / overlap):
  later txs still Opt-race ℓ → validate fail → FullReplay/Rewind train
  Avoid theater (ab/c↑ or overlap_fill↑) without cutting subsequent hops' re-exec

required (mid-block hot-key protect):
  first hot evidence on ℓ this block
    → arm WaitOnce/OrderedTip for remaining touchers of ℓ
    → later hops never Opt-discover the same conflict again
    → zero/few FullReplay on subsequent hops once protected
```

---

## 3. Mid-block Learn→Avoid protocol

### 3.1 Trigger (same block, earliest evidence)

Arm protect on ℓ when **any** of the following fires **intra-block** (not cross-block Learn only):

1. **First EffectiveWAW** on ℓ (ordered/write collision observed).  
2. **Hot evidence:** sticky writer, `avoid_hold`, chain template, or repeated Opt→conflict on ℓ within the block.  
3. **First Effective RAW / WAR** that names ℓ as the contested location (same IntraPatch path).

Do **not** wait for end-of-block Learn nail before protecting remaining touchers.

### 3.2 Action: IntraPatch AccessArm WaitOnce

```
on first hot evidence for ℓ in this block:
  1. classify ℓ hot for remainder of block
  2. IntraPatch AccessArm for every remaining tx that will touch ℓ:
       AccessArm = WaitOnce | OrderedTip   # never sticky→Opt demotion
  3. install protect BEFORE those txs' Opt read/write of ℓ
  4. SfMvMemory: successors consume true Data|OrderedTip tip only
  5. exact wake on publish → resume access (Q_released if needed)
```

- **Protect-before-Opt** is the non-negotiable: later accesses must not Opt-discover the conflict again.  
- Legal AccessArm outputs remain **Opt | WaitOnce | NeverWait** only (no pe-without-Avoid).  
- Sticky ≥32 / `avoid_hold` spine: **preserve WaitOnce**; Learn must **not** demote sticky WaitOnce→Opt under wall pressure.

### 3.3 Lock-like — precise meaning

| Lock-like **is** | Lock-like **is not** |
|------------------|----------------------|
| SpecFence **publish-order Avoid**: WaitOnce / OrderedTip until **true** SfMvMemory Data\|OrderedTip tip | OS/mutex exclusive lock on ℓ |
| One-hop ordered tip install + exact waiter wake | OCC **Estimate Block** / Estimate tip as VisibilityPolicy |
| Access-grain stall on contested ℓ after protect armed | Whole-tx serialize-until-pred-done as sole Avoid |
| Mid-block IntraPatch so remaining touchers never Opt-race | Win Fence / OrderedAdmit **prepaid** refuse theater |

`estimate_block_sf` must stay **0**. MvMemory Estimate remains **OCC-only**.

### 3.4 Why this beats the abort train

```
OCC abort train (per hop):
  Opt exec → validate fail → abort → incarnation++ → FullReplay
  × every later toucher of hot ℓ

Mid-block protect (once per ℓ per block after first evidence):
  first collision / hot Detect → arm WaitOnce for remaining touchers
  → subsequent hops wait true tip → commit without Opt-discover
  → zero/few FullReplay on subsequent hops once protected
```

Operational proxy:

- Per hot ℓ: **FullReplay_after_protect ≪ FullReplay_before_protect** (ideally ~0 on later hops).  
- Large Chain: `chain_ab/c` ≥ 3× **and** SF wall ≤ OCC/1.5 (Avoid without re-exec storm).  
- Four-class audit: **b ≫ c** (Avoid/release ≫ abort/FullReplay-from-0).

---

## 4. Four classes — concurrent Detect|Avoid|Resolve

Detect|Avoid|Resolve stay **concurrent** capabilities, not a staged pipeline. Mid-block protect is the **Avoid** arm fed by timely **Detect**; **Resolve** remains prefix/`fail_k` when an assumption fails.

| Class | Mid-block hot-key protect |
|-------|---------------------------|
| **RAW** | First bad/needed producer evidence on ℓ → WaitOnce remaining readers on true Data tip; prefix Resolve from `fail_k` if assumption fails |
| **WAR** | Hot read→write evidence → reader reserve / version protect for remaining writers; never schedule-absorbed-only; repair affected access only |
| **WAW** | First EffectiveWAW on ℓ → OrderedTip / WaitOnce for remaining writers; continue from `fail_k`; no Opt→FullReplay-from-0 loops |
| **Chain** | First sticky/hot hop evidence → IntraPatch WaitOnce along remaining chain touchers; plant→Version→Released→access resume; PC ≤1 core on hop, rest antichain; **no** full-tx off-queue serialize as sole vehicle |

WAR remains first-class. No pe-only telemetry substitutes for access-level protect.

---

## 5. Explicit forbid list

| Forbid | Why |
|--------|-----|
| **Schedule-only Avoid as primary path to ≥1.5** | serialize + overlap lands failed cost inequality |
| **Full-tx serialize-until-pred-done as sole large Avoid** | land-v1: ab/c↑, wall ≫ OCC |
| **Overlap access-wait alone** (no mid-block protect) | overlap land 0.54/0.55; still Opt-discover |
| **Win HOLD / Fence prepaid** | v5 wall explosion |
| **Rem / vis / metrics / ordinal `access_log` strip** | exhausted; ordinal strip → FullReplay storm |
| **Estimate Block** on SF path | OCC-only; `estimate_block_sf=0` |
| **Learn demote sticky WaitOnce→Opt** / pe-without-Avoid | recreates abort train |
| **Protect only cross-block** (miss same-block later touchers) | later hops Opt-discover again |
| **Mutex / blocking park / SoftWait≠0** into interpreter | Soft=0 Instant-off; lock-like ≠ mutex |
| **Thin Rewind / mark_gated / long Chain busy-spin / claim-wake-only** | discarded theater |
| **Treating overlap land 0.54/0.55 as climb floor** | restore toward **v4 tip first** if tip worse |

**Allowed (continue):** sticky **writers** HOLD ≥32 on large; Prefer Rewind / prefix from `fail_k`; WaitOnce-only OCC-shaped gate on thin; full `access_log.note`; SfMvMemory early true tip + exact wake; NeverWait for beneficiary / basic_lazy; access-grain WaitOnce **after** mid-block protect; antichain fill while protected access waits.

---

## 6. Acceptance

Harness: Soft=0 Instant-off (`env -u SPECFENCE_HANG_TRACE`), 8 cores, `SPECFENCE_COMPARE_CHECK=1`, **N≥5 both** `3356896` and `15274915`.

| # | Criterion |
|---|-----------|
| A | Soft=0; `seq≡par`; `occ_picks=0`; `soft_wait_arms=0`; `explore=0`; no hang |
| B | **TPS SF/OCC ≥ 1.5** on **every** accepted focus-block result for **both** blocks |
| C | **`estimate_block_sf=0`**; no Win HOLD Fence prepaid; no Estimate tip hits on SF path |
| D | Four-class audit RAW/WAR/WAW/Chain + WAR first-class; **b ≫ c** per class |
| E | Mid-block protect evidence: after first hot evidence on ℓ, later touchers show WaitOnce/OrderedTip **before** Opt read; subsequent-hop FullReplay ≈ 0 or ≪ pre-protect |
| F | Large: `chain_ab/c` ≥ 3× median reuse **and** SF wall ≤ OCC/1.5; sticky `chain_n` not collapsed; Learn keeps WaitOnce |
| G | Thin: calm SF ≲ OCC/1.5 **or** proven antichain-width + protect delivering TPS≥1.5; ordinals kept |
| H | If tip TPS &lt; v4 (~0.83 / ~0.65): **restore toward v4 tip first** before new protect land (do not climb from overlap 0.54/0.55) |
| I | Lib release + arch SoftWait=0 seq≡par checks pass |

**Not success:** TPS&lt;1.5 with Soft=0 held; ab/c↑ with FullReplay storm on later hops; soft language below 1.5; incubating on worse-than-v4 tip without restore.

---

## 7. Land checklist (no CloudAgent from this note’s author)

Land is a **mid-block Learn→Avoid protect** redesign. Coordinator may launch implementers separately; this note does **not** launch CloudAgent and does **not** edit pevm.

### 7.0 Preflight

1. Read this SoT + overlap land (FAILED 0.54/0.55) + serialize land (FAILED 0.44) + Avoid-beats SoT + four-class v1 + focus TPS bar + SfMvMemory redesign + land-v4/v5.  
2. **If current tip &lt; v4 floor:** restore toward v4 tip (~0.83 / ~0.65) before protect experiments.  
3. Baseline Soft=0 Instant-off N=5 both; record TPS, wall ms, `chain_ab/c`, FullReplay-before/after first protect, `estimate_block_sf`.

### 7.1 Mid-block protect plane (primary)

4. IntraPatch on first EffectiveWAW / hot evidence on ℓ → AccessArm WaitOnce|OrderedTip for remaining touchers **this block**.  
5. Gate: protect installed **before** those txs’ Opt read/write of ℓ (instrument `protect_before_opt` hits).  
6. SfMvMemory true tip only; exact wake; `estimate_block_sf=0`.  
7. Admission WaitOnce **without** Fence / Win OrderedAdmit prepaid.

### 7.2 Learn / sticky

8. Stop Learn demotion sticky WaitOnce→Opt; preserve WaitOnce on sticky≥32 / `avoid_hold`.  
9. Sticky **writers** HOLD into `last_location_writers`; Prefer Rewind / prefix Resolve.  
10. Cross-block Learn may seed templates; **same-block** IntraPatch is mandatory for later touchers.

### 7.3 PC / width

11. Ideal @8: ≤1 core on current protected WaitOnce hop; rest antichain / Q_released / Q_indep.  
12. Ready = longest remaining crit; no reverse launch; no re-pick gated; no sole full-tx off-queue serialize.

### 7.4 Four-class + verify / sync

13. Instrument RAW/WAR/WAW/Chain Detect|Avoid|Resolve; prove WAR not schedule-absorbed-only; **b ≫ c**.  
14. Soft=0 Instant-off N≥5 both; TPS tables lead; prove subsequent-hop FullReplay collapse after protect.  
15. Land result note under `lab/notes/`; sync via `lab/scripts/sync-to-github.sh`.  
16. If TPS&lt;1.5: revise protect timing/arm — **do not** return to serialize-only, overlap-only, rem strip, or Win HOLD.

### 7.5 Out of scope for this note’s author

- pevm edits  
- CloudAgent launch  
- SoftWait≠0 / Instant-on  
- Mixed-49 as acceptance (focus pair only)  
- Deleting OCC Estimate for OCC workers  
- Re-landing serialize or overlap-alone as sole Avoid  
- Another rem/tip/Win HOLD micro-cut PR

---

## 8. Pointers

| Topic | SoT |
|-------|-----|
| Failed overlap land (0.54/0.55) | [`specfence-avoid-overlap-land-v1.md`](specfence-avoid-overlap-land-v1.md) |
| Failed serialize land (0.44) | [`specfence-avoid-beats-occ-abort-land-v1.md`](specfence-avoid-beats-occ-abort-land-v1.md) |
| Cost inequality (preserved) | [`specfence-avoid-beats-occ-abort-v1.md`](specfence-avoid-beats-occ-abort-v1.md) |
| Four classes + concurrent DAR | [`specfence-four-conflict-classes-v1.md`](specfence-four-conflict-classes-v1.md) |
| Focus TPS≥1.5 bar | [`specfence-focus-pair-tps-bar-v1.md`](specfence-focus-pair-tps-bar-v1.md) |
| Version/visibility plane | [`specfence-sf-mvmemory-redesign-v1.md`](specfence-sf-mvmemory-redesign-v1.md) |
| v4 restore floor | land-v4 thin ~0.83 / large ~0.65 |

**This file is SoT for: mid-block hot-key Learn→Avoid protect (WaitOnce/OrderedTip on true tip) so later txs never Opt-discover the same conflict — the path that can beat the OCC abort train after schedule/overlap failed.**
