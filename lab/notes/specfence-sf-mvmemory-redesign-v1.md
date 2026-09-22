# SpecFence SfMvMemory Redesign v1

**Date:** 2026-09-22  
**Kind:** design SoT (implement on pevm PR #45)  
**Focus:** Soft=0 Instant-off blocks `3356896` + `15274915`  
**Companion:** [`specfence-thin-avoid-no-estimate-v1.md`](specfence-thin-avoid-no-estimate-v1.md)  
**Hard acceptance (supersedes remasure/opt-v2 floors):** **TPS SF/OCC ≥ 1.5** both blocks (wall SF/OCC ≤ ~0.67).

---

## 0. Thesis

1. OCC `MvMemory` + `MemoryEntry::Estimate` is **baseline-only**. SpecFence must not gate Avoid on Estimate tips or `park_estimate_blocking`.
2. SpecFence-native **`SfMvMemory`** owns version tips, true Data publish, exact waiter wake, and VisibilityPolicy reads (`Opt` | `WaitReleased` | `OrderedTip`).
3. Primary lever: write path installs a **Data/version tip early enough**; WaitOnce reads only after **true publish**; publish wakes **exact** waiters; beneficiary/lazy = **NeverWait**.
4. Thin Avoid consumes WaitOnce via SfMvMemory VisibilityPolicy (read-after-true-publish), not OCC Estimate/`maybe_wait`.
5. Large keeps sticky ≥32 + fail_k Rewind. Soft=0 Instant-off; `occ_picks=0`.

---

## 1. Data structures (SfMvMemory)

```
SfTipTable (shared, SpecFenceCtx):
  tips: DashMap<ℓ, BTreeMap<tx, SfTip>>
    SfTip::Version { incarnation }   // claimed — NOT Estimate
    SfTip::Released { incarnation }  // true Data live in MvMemory
  waiters: DashMap<(ℓ, writer), Vec<consumer>>  // exact wake set
  counters: early_tip_n, publish_wake_n, wait_once_consume_n, estimate_block_sf (=0)

SfMvMemory<'a> { inner: &MvMemory, tips: &SfTipTable }
  read(ℓ, vis, tx) -> SfRead::{Data, Estimate(Opt-only), Storage}
  install_version_tip(ℓ, writer, inc)   // SF write path begin / known WS
  publish_data(ℓ, writer, inc)          // after MvMemory::record Data
  register_waiter(ℓ, writer, consumer)
  wake_exact(ℓ, writer) -> Vec<consumer>
```

OCC `convert_writes_to_estimates` untouched for the OCC harness path. SF workers never Branch→Block on Estimate.

---

## 2. Call-graph (target)

```
SF execute begin
  → SfMvMemory.install_version_tip(prior write_locations)   // early version tip
  → evm…
  → MvMemory.record(Data…)
  → SfMvMemory.publish_data + wake_exact → ready_edges / runnable

SF basic/storage (ungated):
  → consult_ungated_wait_once
       WaitOnce + true writer unfinished:
         consume via VisibilityPolicy WaitReleased|OrderedTip
         spin/help while Executing for true Data; else exact-waiter defer
         NEVER park_estimate_blocking / Estimate tip as publish signal
       NeverWait (beneficiary/lazy): skip
  → SfMvMemory.read(vis)   // WaitReleased skips Estimate
  → validate_to_plan …

estimate_block_sf must be 0 on Soft=0 Instant-off focus pair.
```

---

## 3. Thin vs large

| | Thin (n≤176) | Large (15274915) |
|--|--------------|------------------|
| WaitOnce | read-after-true-publish via SfMvMemory; no Estimate Block; no thin Rewind; no 15-hold; no mark_gated; no broad plant | parks when pred live allowed; sticky ≥32 hold; fail_k Rewind |
| Early tip | install from prior WS / known WaitOnce locs | same tip API; sticky plant unchanged |
| Discard | Estimate Block, thin Rewind, 15-hold, mark_gated, broad plant, one-shot InconsistentRead | do not regress sticky/Rewind |

---

## 4. Acceptance

| # | Criterion |
|---|-----------|
| A | Soft=0 Instant-off N≥5 both; seq≡par; occ_picks=0; soft_wait_arms=0; explore=0 |
| B | **TPS SF/OCC ≥ 1.5** both (primary). Wall ≤~0.67 secondary |
| C | estimate_block_sf=0; call-graph: SF Avoid never Blocks on Estimate |
| D | 15274915 sticky≥32 + fail_k Rewind intact |
| E | Land note `specfence-sf-mvmemory-land-v1.md` with TPS tables + call-graph |

If a land cannot hit 1.5: ship best Soft=0 tip + honest gaps vs 1.5, then another full-batch pass in-run.
