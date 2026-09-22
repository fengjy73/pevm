# SpecFence SfMvMemory Redesign v1

**Date:** 2026-09-22 (Asia/Shanghai)  
**Kind:** design only — no pevm edit, no CloudAgent from this note’s author.  
**Focus:** SpecFence-native version plane (`SfMvMemory` + `VisibilityPolicy`) as the **primary speed lever** for thin Avoid (block `3356896`) without regressing large sticky ≥32 (`15274915`).  
**Parents / SoT:**  
- [`specfence-thin-avoid-no-estimate-v1.md`](specfence-thin-avoid-no-estimate-v1.md) — thin Avoid / No Estimate gate; this note owns the write/read plane.  
- [`specfence-cc-pc-first-principles-redesign-v3.md`](specfence-cc-pc-first-principles-redesign-v3.md) — Detect→Avoid→Resolve→Learn; shared/private PC (§4.4).  
- [`specfence-sf-ps-pr44-shallow-gap-and-true-spine-v1.md`](specfence-sf-ps-pr44-shallow-gap-and-true-spine-v1.md) — T2: `read(ℓ, VisibilityPolicy)`.  
- [`specfence-sf-ps-true-spine-learn-pc-detailed-v1.md`](specfence-sf-ps-true-spine-learn-pc-detailed-v1.md) — WaitReleased / OrderedTip execute semantics.  
- [`specfence-first-class-architecture-redesign-v1.md`](specfence-first-class-architecture-redesign-v1.md) — VisibilityPolicy as first-class Avoid.  

**Metrics language:** report **TPS SF/OCC** (higher better). Wall SF/OCC inverse; wall ms absolute context only.

---

## 0. Six-bullet SfMvMemory thesis

1. **OCC `MvMemory` stays the compare baseline** — Estimate tip, late `record` Data batch, optimistic tip race — SpecFence workers must not consult Estimate.  
2. **SpecFence process is Detect→Avoid→Resolve→Learn** on true RAW/WAW edges: WaitOnce + **true publish** (Data / version tip), NeverWait for beneficiary/basic_lazy, long-chain sticky ≥32 hold on large — not OCC abort trains.  
3. **`SfMvMemory` is SpecFence-native:** shared per-ℓ version tip + live writer + WaitReleased/OrderedTip + exact waiter sets; private journals/stacks per worker.  
4. **Read:** WaitOnce / WaitReleased / OrderedTip read **only after Data/version publish**; Opt path when arm Opt or NeverWait — **SF path never consults Estimate**.  
5. **Write:** install SpecFence version tip **early enough** that WaitOnce readers can proceed without Estimate; publish clears live_writer then wakes **exact** waiters.  
6. **Thin (`3356896`) wins from early tip + exact wake; large (`15274915`) keeps sticky ≥32 hold** — SfMvMemory is the primary speed lever, not thin scheduler hacks alone.

---

## 1. OCC MvMemory responsibilities vs SpecFence process

### 1.1 What OCC `MvMemory` still is (baseline-only)

| Responsibility | OCC role | SpecFence stance |
|----------------|----------|------------------|
| Multi-version map ℓ → incarnations | Optimistic concurrent write races | Keep for OCC harness compare |
| **Estimate tip** (“writer started”) | Race hint for `decide` / Block / `maybe_wait` | **OCC-only.** SF path never consults Estimate |
| Late Data install at `MvMemory::record` end | Batch publish after full exec | OCC artifact; causes thin WaitOnce→Opt→FullReplay |
| Validate bool → abort / incarnation++ | Resolve spine | OCC baseline; SF uses ResolvePlan prefix from fail_k |

OCC remains the Soft=0 Instant-off compare opponent. Do not delete Estimate machinery for OCC workers — **fence SF callers away from it**.

### 1.2 SpecFence process (what SfMvMemory must serve)

```
Detect  → true RAW/WAW edges only (exclude beneficiary, basic_lazy)
Avoid   → AccessArm: Opt | WaitOnce | NeverWait at each read
          WaitOnce ⇒ consume publish of true writer (Data/version tip)
Resolve → prefix from fail_k; large: sticky ≥32 + fail_k Rewind
Learn   → AccessArm only (no pe-without-Avoid, no sticky Opt as Learn output)
```

| SpecFence verb | Needs from version plane |
|----------------|--------------------------|
| **WaitOnce** | True publish signal (Data / SF version tip), not Estimate; WakeOnce per `(reader,ℓ,w)` |
| **True publish** | Tip visible to WaitReleased/OrderedTip **before** readers Opt-fallthrough |
| **NeverWait** | Beneficiary / basic_lazy: OptRead; never enter Detect edges or waiter sets |
| **Long-chain sticky ≥32** | Large block: hold / OrderedTip along chain; exact waiter wake; no Estimate Block |

Incomplete SF-PS today: thin consults WaitOnce but **Skips Block**, Opt-reads Storage pre-state while writer Data is still at OCC `record` end — Estimate is the wrong substitute tip ([thin-avoid §2](specfence-thin-avoid-no-estimate-v1.md)).

---

## 2. SpecFence-native structure: shared vs private

Aligned with first-principles v3 §4.4 and spine T2 (scheme **α**: SF API separated from OCC plane).

### 2.1 Shared (cross-worker, fine-grained / sharded)

| Structure | Semantics |
|-----------|-----------|
| **Version tip per ℓ** | Latest **published** Data / ordered tip for SpecFence readers. Install early on SF write path (not Estimate). |
| **live_writer(ℓ)** | True unpublished writer `w` of ℓ (structure RAW/WAW only). Cleared on publish **after** tip write. |
| **VisibilityPolicy tip state** | Per-ℓ: supports **WaitReleased** (pred published → readable) and **OrderedTip** (installed ordered tip, not OCC race tip). |
| **Waiter sets per ℓ** | Exact set of WaitOnce / WaitReleased readers waiting on this ℓ’s true writer. Publish wakes **only** these. |
| **AccessArmTable / Posterior / NeverWait** | Shared read + clocked batch patch (Learn); NeverWait ℓ never join waiter sets. |
| **Runnable queues** (Q_indep / Q_released / Q_ordered) | PC; publish → batch push waiters to Q_released / advance Q_ordered tip. |

Publish order (hard invariant, v3 §4.4):

```
1. write SfMvMemory version tip (Data / OrderedTip)
2. clear live_writer(ℓ)
3. wake exact waiter set → Q_released (notify stealer)
```

Never: clear gate before tip; never: wake on Estimate; never: broad `rset_w` plant.

### 2.2 Private (per worker / incarnation)

| Structure | Why private |
|-----------|-------------|
| Tx execution stack / EVM frame | Avoid DashMap reentry; cut false sharing |
| Incarnation prefix journal | Resolve fail_k restore without shared mutate |
| Local read/write buffers before publish | Coalesce then install tip once |
| Pick cursor / steal seed | PC contention |

Workers **must not** nest DashMap `get` on OCC `MvMemory` while holding SF locks (true-spine land lesson: deadlock). Prefer SF-plane API that does not re-enter OCC map.

### 2.3 API sketch (design-level)

```text
SfMvMemory:
  read(ℓ, vis: VisibilityPolicy, tx) -> Value
      // Opt | WaitReleased | OrderedTip
  install_tip_early(ℓ, w, tip)      // SF write path — BEFORE full record end
  publish(ℓ, w, data)               // tip + clear live_writer + wake waiters
  register_waiter(ℓ, w, reader)     // WaitOnce arm only; NeverWait forbidden
  live_writer(ℓ) -> Option<TxId>

VisibilityPolicy:
  Opt          // NeverWait or arm Opt: may race / Storage pre-state (DAG antichain)
  WaitReleased // readable iff pred published Data/version tip
  OrderedTip   // read installed ordered tip (WAW install order)
```

**Estimate is not a VisibilityPolicy variant.** It does not appear in SF API.

---

## 3. Read path

### 3.1 WaitOnce → WaitReleased / OrderedTip (true publish only)

```
arm = AccessArm[classify(ℓ,k)]
if arm == NeverWait or ℓ ∈ {beneficiary, basic_lazy}:
    return OptRead(ℓ)                     // §3.2
if arm == WaitOnce and exists true_writer w of ℓ:
    // DO NOT: Block on Estimate; DO NOT: Opt Storage while tip unpublished
    vis = WAW_ordered ? OrderedTip : WaitReleased
    if tip_published(ℓ, w):
        return SfMvMemory.read(ℓ, vis, tx)
    else:
        register_waiter(ℓ, w, tx)         // or thin: defer pick / short help-spin
        // after wake: tip is Data/version — then read
        return SfMvMemory.read(ℓ, vis, tx)
return OptRead(ℓ)                         // no live pred
```

Hard rules:

- WaitOnce / WaitReleased / OrderedTip read **only after Data/version publish**.  
- **SF path never consults Estimate** (no `decide→Block` on Estimate, no `maybe_wait` Estimate, no “Estimate present ⇒ published”).  
- Same `(tx,ℓ,w)` WaitOnce **at most once** (v3 C3); publish/done must release.

### 3.2 Opt path (arm Opt or NeverWait)

- **NeverWait** (beneficiary, basic_lazy): always OptRead; never waiter; never Detect edge.  
- **arm Opt**: explicit gamble when EV prefers Opt or no WaitOnce yet; lose → Resolve prefix from fail_k (large Rewind; thin Prefer Avoid-before-fail via early tip so Opt lose rate falls).  
- Opt may use Storage pre-state / independent MV walk — **still must not branch SF Avoid on Estimate**.

Mapping to AccessArm (thin-avoid §4 + v3 Avoid):

| AccessArm | VisibilityPolicy | Estimate? |
|-----------|------------------|-----------|
| WaitOnce | WaitReleased or OrderedTip | **Never** |
| Opt | Opt | Never on SF gate |
| NeverWait | Opt | Never |

---

## 4. Write path

### 4.1 Early SpecFence version tip (thin-avoid option iii — primary)

OCC installs Data at `MvMemory::record` **end**. Thin WaitOnce then sees no publish → Opt → FullReplay.

SpecFence write path:

```
on SF write known for ℓ (structure WAW/RAW writer w):
  install_tip_early(ℓ, w, tip)   // version tip OR Data slice — NOT Estimate
  // readers with WaitOnce can WaitReleased/OrderedTip without OCC Estimate
on SF publish complete:
  publish(ℓ, w, data)            // finalize tip if needed
  clear live_writer(ℓ)
  wake_exact_waiters(ℓ)          // → Q_released / OrderedTip advance
```

Properties:

- Tip is **Data/version**, never Estimate semantics.  
- Early enough that WaitOnce readers on thin path need **no** Blocking park, Rewind, antichain drain, `mark_gated`, or Estimate Block.  
- OCC Estimate path untouched for baseline workers.

### 4.2 Publish wakes exact waiters

- Wake set = registered WaitOnce/WaitReleased waiters for this ℓ (and OrderedTip next tip only).  
- **Forbid:** broad post-FullReplay plant, `rset_w` collapse, coinbase/beneficiary waiters.  
- Batch enqueue + notify so @8 cores steal released work (v3 PC / detailed spine §4.2).

### 4.3 Explicit: Estimate stays OCC-only

| Plane | Estimate tip | Data/version tip |
|-------|--------------|------------------|
| OCC `MvMemory` | Yes — race / Block / maybe_wait | Late at `record` end |
| SF `SfMvMemory` | **Forbidden** — counters `estimate_block_sf=0` | Early install + publish |

Call-graph audit: SF Avoid / decide / consult / WaitOnce consume **never** branches on Estimate. If a shared helper still sees Estimate, SF callers must skip that branch (dead-code or mode fence).

---

## 5. Thin (`3356896`) vs large (`15274915`) × sticky ≥32

| Axis | Thin 3356896 (n≤176) | Large 15274915 |
|------|----------------------|----------------|
| Dominant edge | RAW=0, pure WAW on basic ℓ `dff71d59…`, fail_k 5/6 | Long dependency chain |
| SfMvMemory role | **Early tip** so WaitOnce sees publish without Estimate; exact wake; Prefer Avoid-before-FullReplay | OrderedTip / WaitReleased along chain; exact one-hop pred |
| Sticky ≥32 hold | **Do not apply** tip-only / 15-writer / sticky hold on thin | **Keep** opt-v2 sticky ≥32 hold, head-first LIFO, no `mark_gated` |
| Resolve | Prefer Avoid so FullReplay↓ without thin Rewind | fail_k Rewind + hang-free PartialAbortRewind; `full_from_0` low |
| Gate predicate | Thin-only early tip / defer-pick behind `n≤THIN_N` or “no sticky≥32 chain” | Bit-compatible with opt-v2 large path |
| Forbid | Estimate Block, park, thin Rewind, broad plant, mark_gated, one-shot reread | Do not strip sticky hold or fail_k Rewind for thin land |

**Interaction rule:** SfMvMemory early tip is shared infrastructure; **scheduler taxes** stay thin-gated. Large sticky hold remains SF PC/CC for long chain (thin-avoid §5), not “OCC residue to delete.”

---

## 6. Acceptance (TPS SF/OCC)

Same harness language as thin-avoid §6; SfMvMemory-specific proofs added.

| # | Criterion |
|---|-----------|
| A | Soft=0 Instant-off **N≥5** both blocks; `seq=par`; `occ_picks=0`; `soft_wait_arms=0`; `explore=0`; no hang |
| B | **3356896** TPS SF/OCC clearly+stably **above remasure ~0.70**; prefer **≥0.80** |
| C | **15274915** TPS SF/OCC **≥ ~0.63** and **not below opt-v2** |
| D | **`estimate_block_sf=0`** (or call-graph: SF Avoid never branches on Estimate) |
| E | SF WaitOnce / WaitReleased / OrderedTip reads hit **Data/version tip** counters; Estimate tip hits on SF path = 0 |
| F | Thin: FullReplay reuse ↓ without park↑ / Rewind↑ / antichain-drain↑; early_tip_install and exact_wake counters > 0 when WaitOnce armed |
| G | Large: sticky ≥32 hold intact; `resolve_rewind`≫0; `full_from_0` low |
| H | Lib release + `complete_arch_edge_pi_seq_eq_par_softwait0` pass |

Reports: lead with **TPS SF/OCC**; prove no Estimate on SF path; name which thin Avoid options (thin-avoid §4.2) shipped with SfMvMemory (expect **iii** primary, optionally **ii** narrow wake).

---

## 7. Land checklist (merge with thin-avoid-no-estimate)

**SfMvMemory is the primary speed lever** — not thin scheduler hacks alone. Merge with [thin-avoid §7](specfence-thin-avoid-no-estimate-v1.md); coordinator launches CloudAgent separately.

### 7.1 Version plane (this note — do first)

1. Introduce / harden `SfMvMemory` (scheme α) with `read(ℓ, Opt|WaitReleased|OrderedTip)`.  
2. SF write: `install_tip_early` + `publish` (tip → clear live_writer → exact wake). Tip = Data/version **not** Estimate.  
3. Fence SF Avoid/consult/decide: **zero** Estimate Block / Estimate branch; leave OCC Estimate for baseline.  
4. Waiter sets per ℓ; NeverWait ℓ never register.  
5. Counters: `estimate_block_sf`, `sf_early_tip_install`, `sf_exact_wake`, WaitOnce consume-hit on published tip.

### 7.2 Thin Avoid (thin-avoid §4 — after or with tip)

6. Enforce read-after-true-publish when WaitOnce + true writer.  
7. Prefer option **(iii)** early tip; else **(ii)** narrow Data-publish wake (no `mark_gated`); **(i)** micro spin only if writer Executing.  
8. Keep `consult_ungated_wait_once`; do not restore `optimistic_skip_gate` skip.  
9. Forbid: Estimate Block, thin Rewind, 15-hold, mark_gated, one-shot reread, broad plant.

### 7.3 Large hold

10. Preserve sticky ≥32 + fail_k Rewind bit-compatible with opt-v2; gate thin-only scheduler changes by `n≤176` / no sticky chain.

### 7.4 Tests / note

11. Soft=0 Instant-off focus pair N≥5; TPS tables; proof SF never Blocks on Estimate.  
12. Land result note; sync lab via `lab/scripts/sync-to-github.sh`.

**Priority order for speed:** SfMvMemory early tip + exact wake (**iii**) ≫ thin defer-pick alone ≫ spin assist. Scheduler-only thin hacks without publish tip are incomplete SF-PS.

---

## 8. Out of scope

- pevm code edits from this note’s author; CloudAgent lands separately.  
- Mixed-49 board (focus pair for acceptance).  
- SoftWait≠0 / Instant-on.  
- Deleting OCC Estimate for OCC workers.  
- Restoring pe-without-Avoid / sticky Opt as Learn output.

---

## 9. Pointer

Thin Avoid / No Estimate gate narrative remains in [`specfence-thin-avoid-no-estimate-v1.md`](specfence-thin-avoid-no-estimate-v1.md). **This file is SoT for SpecFence write/read plane (`SfMvMemory` + VisibilityPolicy).** Do not duplicate the full plane design into the thin-avoid note.
