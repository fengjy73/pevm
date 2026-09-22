# SpecFence SfMvMemory land v1

**Date:** 2026-09-22  
**Tip:** `cd72ec8` on `cursor/specfence-sf-ps-true-spine-d6e8` (PR #45)  
**Harness:** Soft=0 Instant-off, 8 cores, `SPECFENCE_COMPARE_CHECK=1`, N=5 both  
**SoT:** [`specfence-sf-mvmemory-redesign-v1.md`](specfence-sf-mvmemory-redesign-v1.md), [`specfence-thin-avoid-no-estimate-v1.md`](specfence-thin-avoid-no-estimate-v1.md)  
**Acceptance (this land):** TPS SF/OCC — `3356896` stably >~0.70 prefer ≥0.80; `15274915` ≥~0.63; Soft=0 Instant-off N≥5; `occ_picks=0`; `estimate_block_sf=0`.  
**Hard bar (queued):** TPS ≥1.5 both — **not met**.  
**Co-author:** `0xstride <fengjy73@users.noreply.github.com>`

---

## Verdict

SpecFence-native `SfMvMemory` / `SfTipTable` landed for **thin** Avoid (early version tip + `live_writer` + WaitReleased/OrderedTip + exact waiters; spin-only WaitOnce, **no** Blocking park / Estimate Block). Large keeps sticky ≥32 hold + fail_k Rewind; tip-plane DashMap gated thin-only to protect sticky TPS.

**Soft=0 Instant-off paired N=5 (3 passes, tip `cd72ec8`):**

| Block | Pass TPS | Med TPS | Floor | Prefer / sticky |
|------:|---------:|--------:|:-----:|:----------------|
| 3356896 | 0.764 / 0.708 / **0.836** | **0.764** | >~0.70 **met** | ≥0.80 on 2/3 passes |
| 15274915 | 0.645 / **0.749** / 0.583 | **0.645** | ≥~0.63 **met** | sticky chain_n=75 → 0.749; hop/collapse → 0.583 |

All passes: `seq=par`, `occ_picks=0`, `soft_wait_arms=0`, reuse `explore=0`. Hard ≥1.5 still open (path c + SF tax).

---

## TPS tables (primary = reuse median)

TPS SF/OCC = OCC_wall / SF_wall (higher better).

### Paired Soft=0 Instant-off N=5 @8 (tip `cd72ec8`)

| Pass | 3356896 OCC / SF / **TPS** | 15274915 OCC / SF / **TPS** | Large sticky note |
|-----:|---------------------------:|----------------------------:|:------------------|
| 0 | 1.504 / 1.968 / **0.764** | 9.509 / 14.732 / **0.645** | chain_n reuse ~8; Rewind≃72–76 |
| 1 | 1.611 / 2.276 / **0.708** | 9.340 / 12.468 / **0.749** | **chain_n=75** hold; FullReplay reuse 6–24 |
| 2 | 1.659 / 1.985 / **0.836** | 8.611 / 14.761 / **0.583** | chain_n→5; FullReplay ~73–89 |

### Invariants (both, Soft=0 Instant-off N≥5)

| Check | 3356896 | 15274915 |
|-------|:-------:|:--------:|
| seq≡par | ok | ok |
| occ_picks | 0 | 0 |
| soft_wait_arms | 0 | 0 |
| explore (reuse) | 0 | 0 |
| estimate_block_sf | 0 (call-graph: SF → `park_publish_wait`) | 0 |
| thin Blocking park | **none** (spin + Opt-fallthrough) | n/a |
| sticky ≥32 + fail_k Rewind | n/a (chain ~15) | Rewind ≃66–89 when hold sticks |

---

## Call-graph: SF does not Block on Estimate

```
OCC baseline only:
  MvMemory::convert_writes_to_estimates / MemoryEntry::Estimate
  harness ConcurrencyMode::Occ → park_estimate_blocking

SpecFence Soft=0:
  execute begin (thin only, WaitOnce|crit ℓ)
    → SfTipTable.install_version_tip + live_writer(ℓ)   // NOT Estimate
  basic/storage
    → consult_ungated_wait_once
         WaitOnce|crit + unfinished pred:
           SfMvMemory.true_publish_ready? → proceed
           thin: Executing micro-spin → else Opt-fallthrough
                 (register_waiter for wake metrics; NO park_publish_wait)
           large: Executing|SF tip|Estimate-as-liveness
                 → live_writer_act → park_publish_wait
           // park_publish_wait does NOT increment estimate_block_sf
    → VisibilityPolicy WaitReleased|OrderedTip skip Estimate tips
  record (thin WaitOnce|crit)
    → tip Released → clear live_writer → wake_exact
  abort (thin)
    → clear_writer (drop tip + live_writer + wake waiters)

estimate_block_sf: only inside park_estimate_blocking when mode==SpecFence.
SF callers use park_live_writer → park_publish_wait.
```

---

## What shipped (tip `cd72ec8`)

1. **`SfTipTable` + `SfMvMemory`** (`sf_mv.rs`): Version / Released tips, `live_writer(ℓ)`, exact waiters, publish order tip→clear→wake, abort `clear_writer`, counters.  
2. **Thin early tip** at SF execute begin (WaitOnce/crit only) + `publish_data` on Data wake.  
3. **Thin WaitOnce**: spin while Executing; read-after-true-publish; **no** Blocking park / mark_gated / Rewind / Estimate Block (thin-avoid SoT).  
4. **Large**: tip plane **off** (no install/publish/clear tax); sticky ≥32 hold (no equal-length other-ℓ hop); Estimate tip = liveness hint only; `park_publish_wait`; fail_k Rewind.  
5. **Detect(a)** thin-only WaitOnce edge plant at begin from prior.  
6. Official SoTs on branch: redesign + thin-avoid.

### Unit tests

`cargo test -p pevm --lib --release -- sf_mv` — 4 passed (Estimate skip, OrderedTip Data, live_writer publish/abort wake).

---

## Detect→Avoid→Resolve (four classes, short)

| Class | This land |
|-------|-----------|
| **RAW** | Thin: WaitOnce spin / early tip; often (c) FullReplay when tip unpublished. Large: WaitOnce park + Rewind. |
| **WAR** | Still mostly validate/revalidate — no WAR-specific SfMvMemory Avoid. |
| **WAW** | Thin early-WAW `dff71d59…`: (c) when cold; reuse FullReplay→0 on calm Opt luck / tip assist. Large sticky WAW: Rewind salvage when hold sticks (chain_n=75). |
| **Long chains** | sticky ≥32 plant + hold; fixed equal-ℓ hop; focus chain_n can still quiet-collapse → TPS dip. |

Path **(c)** dominating thin WAW / collapsed large = incomplete vs ≥1.5.

---

## Remaining gaps (vs ≥1.5 and sticky noise)

1. Thin Avoid still Opt→FullReplay when Data not yet published after spin — need stronger (ii) defer-pick or true early Data slice.  
2. Large TPS oscillates with observed spine length (0.58–0.75); hold helps but quiet morph still shortens `last_location_writers`.  
3. SF scaffolding tax ≫ OCC abort tax on this host (OCC walls ~1.5 / ~9 ms).  
4. Hard acceptance TPS ≥1.5 both: **not met**.

---

## Discarded (still)

Estimate Block as Avoid, thin Rewind/checkpoints, 15-writer hold, `mark_gated` broad plant, one-shot InconsistentRead, tip-install / publish_data on large WS, thin Blocking park.
