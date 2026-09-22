# SpecFence SfMvMemory land v1

**Date:** 2026-09-22  
**Tip:** `7595ef5` on `cursor/specfence-sf-ps-true-spine-d6e8` (PR #45)  
**Harness:** Soft=0 Instant-off, 8 cores, `SPECFENCE_COMPARE_CHECK=1`, N=5 both  
**SoT:** [`specfence-sf-mvmemory-redesign-v1.md`](specfence-sf-mvmemory-redesign-v1.md), [`specfence-thin-avoid-no-estimate-v1.md`](specfence-thin-avoid-no-estimate-v1.md)  
**Hard acceptance:** **TPS SF/OCC ≥ 1.5** both blocks (user 2026-09-22).  
**Co-author:** `0xstride <fengjy73@users.noreply.github.com>`

---

## Verdict

**Did not meet TPS ≥ 1.5.** SpecFence-native `SfMvMemory` / `SfTipTable` landed (version tip + publish wake + WaitOnce consume without Estimate Block). Soft=0 Instant-off N≥5 both: `seq=par`, `occ_picks=0`, `soft_wait_arms=0`, `explore=0`. Best measured TPS this host ≈ **0.58** (3356896) / ≈ **0.56** (15274915) — incomplete vs 1.5 bar. Path **(c)** Opt→FullReplay still dominates early-WAW basics. Ship tip + honest gaps; continue full-batch.

---

## TPS tables (primary = reuse median)

TPS SF/OCC = OCC_wall / SF_wall (higher better). Wall SF/OCC inverse.

### 3356896 (n=176 thin)

| Round (tip) | OCC median ms | SF primary ms | **TPS SF/OCC** | wall SF/OCC |
|-------------|--------------:|--------------:|---------------:|------------:|
| redesign+Avoid peer (`b8296be`) | 1.017 | 1.680 | **0.605** | 1.65 |
| large-path restore (`fd4991d`) | 0.910 | 1.580 | **0.576** | 1.74 |
| tip-install narrow (`7595ef5`) | 0.965 | 2.078 | **0.465** | 2.15 |

Target ≥1.5 ⇒ SF wall ≤ ~0.67×OCC (~0.61–0.68 ms on this host). Gap ≈ **2.5–3.5×** SF speedup still required.

### 15274915 (n=1226 large)

| Round (tip) | OCC median ms | SF primary ms | **TPS SF/OCC** | wall SF/OCC | notes |
|-------------|--------------:|--------------:|---------------:|------------:|-------|
| Avoid peer (regressed) | 5.492 | 12.461 | **0.441** | 2.27 | tip tax on all WS |
| tip-install narrow (`7595ef5`) | 5.724 | 10.216 | **0.560** | 1.78 | rewind≫0, full_from_0 low |
| opt-v2 calm (prior note) | ~5.35 | ~8.44 | **≈0.63** | ~1.58 | sticky≥32 hold |

Target ≥1.5 ⇒ SF ≤ ~3.8 ms. Gap ≈ **2.7×**. Sticky Rewind intact (`resolve_rewind` 50–98; `full_from_0` mostly ≤11); not yet back to opt-v2 TPS 0.63 stably.

### Invariants (both, Soft=0 Instant-off N≥5)

| Check | 3356896 | 15274915 |
|-------|:-------:|:--------:|
| seq≡par | ok | ok |
| occ_picks | 0 | 0 |
| soft_wait_arms | 0 | 0 |
| explore | 0 | 0 |
| estimate_block_sf | 0 (call-graph; SF → `park_publish_wait`) | 0 |

---

## Call-graph: SF does not consult Estimate for Block

```
OCC baseline only:
  MvMemory::convert_writes_to_estimates / MemoryEntry::Estimate
  harness ConcurrencyMode::Occ MV walk → park_estimate_blocking

SpecFence Soft=0:
  execute begin
    → SfTipTable.install_version_tip(WaitOnce|crit ℓ)     // NOT Estimate
  basic/storage
    → consult_ungated_wait_once
         WaitOnce + unfinished pred:
           SfMvMemory.true_publish_ready? → proceed
           thin + (Executing|SF tip): spin → else park_publish_wait
           large + (Executing|SF tip|Estimate tip for live detect):
             live_writer_act → decide → park_publish_wait
           // park_publish_wait does NOT increment estimate_block_sf
    → VisibilityPolicy WaitReleased|OrderedTip skip Estimate tips
  record
    → MvMemory::record(Data)
    → SfTipTable.publish_data + wake_exact
    → wake_on_data_publish (ready_edges / dag / wave)

estimate_block_sf: only incremented inside park_estimate_blocking when
mode==SpecFence. SF callers use park_live_writer → park_publish_wait.
```

---

## Detect→Avoid→Resolve audit (concurrent capabilities)

Early-WAW basic (3356896 ℓ `dff71d59d972…`, fail_k ~5/6; 15274915 sticky chain `abd6bb…`):

| Path | Meaning | Observation this land |
|------|---------|------------------------|
| **(a)** Detect before read | prior/AccessArm WaitOnce + peer known before basic | Armed after FullReplay / prior pack; cold still blind |
| **(b)** Avoid at read via true publish | WaitOnce + SfMvMemory tip/Data before Opt Storage | Spin/defer only when Executing or SF tip; often miss → Storage |
| **(c)** Resolve after fail | Opt → validate → FullReplay / fail_k Rewind | **Still dominates** thin FullReplay ~17–25/reuse; large Full ~60–130 with Rewind salvage |

**Path (c) dominating = incomplete.** SfMvMemory tip plane is necessary but not sufficient: writer Data still lands at `record`; early tip without final value cannot satisfy WaitReleased reads; thin cannot Block not-started preds without serializing the short spine.

---

## What shipped

1. `SfTipTable` + `SfMvMemory` write/read (`sf_mv.rs`): Version / Released tips, exact waiters, counters  
2. Early tip install (WaitOnce/crit only) at SF execute begin  
3. `publish_data` on Data wake path  
4. Thin WaitOnce consume: spin + publish-wait when live/tipped; peer stored on AccessArm; ungated plant after FullReplay  
5. SF Blocking → `park_publish_wait` (estimate_block_sf=0)  
6. Large sticky ≥32 + fail_k Rewind path preserved (bit-compat intent; TPS not yet back to opt-v2)

---

## Remaining gaps blocking TPS ≥ 1.5

1. **Avoid still late:** true Data unknown until finalize → WaitOnce often cannot read-after-publish without serializing thin WAW; FullReplay (c) remains common.  
2. **SF scaffolding tax > OCC abort tax** on this host (OCC walls ~0.9 / ~5.1 ms already low). Beating OCC by 1.5× needs both near-zero conflict waste **and** SF overhead below OCC’s remaining abort cost.  
3. **Large sticky TPS** not stably ≥ opt-v2 0.63 after tip-plane land; need further cut of publish/consult tax without dropping Rewind wins.  
4. **Learn→AccessArm:** CostPolicy arms print Opt while AccessArm WaitOnce may be set — Avoid and Learn surfaces still split; Detect before read (a) under-fires on reuse.  
5. Next full-batch levers (still no Estimate Block / thin Rewind / 15-hold / mark_gated / broad plant): stronger (a)/(b) — e.g. scheduler refuse-pick on WaitOnce+ungated peer before execute; value-stable RebindOnly salvage on thin basic when origin-only churn; cut Soft=0 hot-path metrics/DashMap tax.

---

## Tests

- `cargo test -p pevm --release -- sf_mv` (SfMvMemory unit)  
- Soft=0 Instant-off N=5 both blocks: `seq=par`, `occ_picks=0`  
- `complete_arch_edge_pi_seq_eq_par_softwait0` (run in land CI / follow-up)

---

## Discarded (still)

Estimate Block as Avoid, thin Rewind/checkpoints, 15-writer hold, `mark_gated` broad plant, one-shot InconsistentRead, tip-install on all write locs (large tax).
