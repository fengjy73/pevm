# Thin Avoid · No Estimate Gate v1

**Date:** 2026-09-22 (Asia/Shanghai)  
**Kind:** design only — no pevm edit, no CloudAgent from this note’s author.  
**Focus:** block `3356896` (n≤176 thin); must not regress `15274915` (large, sticky ≥32).  
**SoT evidence:**  
- [`specfence-cc-pc-v3-focus-pair-opt-v2.md`](specfence-cc-pc-v3-focus-pair-opt-v2.md)  
- [`specfence-cc-pc-v3-focus-pair-opt-v3.md`](specfence-cc-pc-v3-focus-pair-opt-v3.md)  
- [`specfence-cc-pc-first-principles-redesign-v3.md`](specfence-cc-pc-first-principles-redesign-v3.md)  
- [`specfence-access-grain-dig-v1.md`](specfence-access-grain-dig-v1.md)  
**Metrics language (going forward):** report **TPS SF/OCC** (higher better). Wall SF/OCC is inverse; keep wall ms only as absolute context.

---

## 0. Five-line thesis

1. **Estimate is Block-STM/OCC spine residue** — SpecFence Avoid must not gate on Estimate tips, `maybe_wait` Estimate Block, or OCC `optimistic_skip_gate` tips; that is incomplete SF-PS (SF still consulting OCC).  
2. On `3356896`, ℓ `dff71d59d972…`, fail_k **5/6**, **RAW=0 pure WAW**: WaitOnce is consulted (`consult_ungated_wait_once`) but thin **Skips Block/Rewind** → Opt reads Storage pre-state → FullReplay; writer Data often appears only at `MvMemory::record` end — Estimate tip is an OCC race artifact, not an SF publish signal.  
3. Thin Avoid = SpecFence-native **publish-order**: read-after-true-publish (AccessArm WaitOnce + true writer) without Blocking park, Rewind tax, antichain drain, Estimate Block, 15-writer hold, `mark_gated`, or one-shot InconsistentRead.  
4. Large `15274915` keeps opt-v2 wins (sticky ≥32 hold, fail_k Rewind, `full_from_0≈0`); thin-only lands must not drop its TPS.  
5. Acceptance in TPS: Soft=0 Instant-off N≥5 both; `3356896` clearly+stably above remasure TPS ~0.70 (prefer ≥0.80); `15274915` ≥ ~0.63 and not below v2; `occ_picks=0`; **no Estimate Block on SF path**.

---

## 1. Baseline numbers (TPS primary)

Harness: Soft=0, Instant-off, 8 cores, `SPECFENCE_COMPARE_CHECK=1`. Primary = reuse median.  
opt-v2 tip ≡ Soft=0 Instant-off calm samples; remasure = census remasure on same host class.

| Block | opt-v2 wall SF/OCC | opt-v2 **TPS SF/OCC** | remasure wall | remasure **TPS** |
|------:|-------------------:|----------------------:|--------------:|-----------------:|
| 3356896 | ~1.35 | **≈0.74** | ~1.43 | **≈0.70** |
| 15274915 | ~1.58 | **≈0.63** | ~2.37 | **≈0.42** |

Absolute calm SF walls (opt-v2 note): 3356896 ~1.33–1.41 ms; 15274915 reuse median ~8.44 ms (OCC ~5.35). Host noise moves ratios; compare TPS inside one run and against remasure TPS floor.

Code tip after opt-v3: restored to opt-v2 (`c62f5bd` ≡ `190b926` behavior). No net Avoid land from v3 attempts.

---

## 2. Diagnosis: why Estimate is mixed into SpecFence (incomplete SF-PS)

### 2.1 Call-graph today (thin ungated, opt-v2/v3)

```
basic/storage (ungated: skip_ungated_tx_path_tax, optimistic_skip_gate)
  → consult_ungated_wait_once(AccessArm WaitOnce / crit)
       thin: return consult-only  // no Block, no rem checkpoint
  → maybe_wait                     // no-op on ungated
  → Opt MV walk / Storage pre-state
       // writer Data often only at MvMemory::record end; no Estimate tip
  → validate_to_plan Opt → try_early_waw_rewind = None (thin)
  → FullReplay
  → Learn note_early_waw arms WaitOnce for reuse
       // AccessArm changes; execute plane does NOT consume it without tax
```

### 2.2 Root cause (first principles)

| Fact | Source |
|------|--------|
| `3356896` RAW=0, WAW on shared basic ℓ `dff71d59d972…`, fail_k 5/6, rebind=0 | access-grain dig |
| WaitOnce consulted but thin Skips Block/Rewind | opt-v2 Criterion C |
| Writer installs **Data** at `record` end; Estimate tip is OCC race hint for “writer started” | opt-v1 open #1, opt-v3 §Why |
| Estimate Block when WaitOnce armed → wall↑ (primary 1.45–1.70 band) | opt-v3 hyp.2 **Discarded** |
| OCC `validate_to_plan` OrderedReplay-from-0 / sticky Opt still shape thin Resolve | first-principles C2/C6/C7 |

**Conclusion:** SpecFence still treats OCC Estimate as a live gate signal. That is **spine residue**, not SpecFence Detect→Avoid. SF Avoid must key off **true publish (Data / SfMvMemory VisibilityPolicy tip)** and AccessArm, never Estimate.

---

## 3. Spine honesty — pevm/Block-STM surfaces vs SpecFence path

Mark each leak: **Keep-as-OCC-baseline-only** | **Delete-from-SF path** | **Replace-with-SF mechanism**.

| Surface | Role today | Mark | SpecFence action |
|---------|------------|------|------------------|
| `MvMemory` Estimate tip (writer-started marker) | OCC race tip; `decide`/Block may wait on Estimate | **Delete-from-SF path** | SF path must not call Block/park on Estimate; counters must prove zero Estimate Block |
| `maybe_wait` Estimate Block | OCC wait-for-estimate | **Delete-from-SF path** | Ungated SF: keep no-op; gated SF large: WaitOnce only on **Data publish / live Executing bounded help**, never Estimate |
| `optimistic_skip_gate` | Skipped AccessArm consult on Opt path | **Replace-with-SF mechanism** | Keep ungated consult (`consult_ungated_wait_once`); do not re-skip WaitOnce for “Opt purity” |
| OCC `validate_to_plan` → OrderedReplay / FullReplay from 0 | OCC Resolve spine | **Keep-as-OCC-baseline-only** | OCC harness remains baseline; SF Resolve on large uses fail_k Rewind (opt-v2); thin Prefer Avoid-before-fail so FullReplay count falls without thin Rewind |
| Sticky Opt / pe-without-Avoid | Learn residue | **Delete-from-SF path** | Learn output = AccessArm only (Opt \| WaitOnce \| NeverWait) at read point |
| `MvMemory::record` late Data install | OCC publish batching | **Replace-with-SF mechanism** | Option (iii): early Data/version tip on SpecFence write path (`SfMvMemory` + `VisibilityPolicy`) so WaitOnce sees **published** tip — **not** Estimate |
| Blocking park / antichain drain / `mark_gated` | OCC-style hold | **Delete-from-SF path** (thin); large sticky hold remains **ungated** plant (opt-v1/v2, no `mark_gated`) | Thin: forbid park + mark_gated + broad plant |
| SoftWait / explore / occ_picks | Shell taxes | **Delete-from-SF path** | Soft=0 Instant-off; explore=0; occ_picks=0 hard |
| Large sticky ≥32 hold + fail_k Rewind | opt-v2 large win | **Keep-as-OCC-baseline-only** is wrong — this **is** SF PC/CC for long chain | **Keep** as SF large-block mechanism; thin must not touch |

---

## 4. Thin Avoid rule (n small, e.g. n≤176) — SpecFence-native early basic-WAW

### 4.1 Prefer read-after-true-publish

At `basic`/`storage` on thin path:

```
arm = AccessArm[classify(ℓ,k)]
if arm == WaitOnce and exists true_writer w of ℓ (structure WAW/RAW, not beneficiary):
    // DO NOT Opt-read Storage pre-state while w has not published Data
    consume_wait_once_for_publish(ℓ, w)   // §4.2 options — no Estimate
    then ReadPublished(ℓ, w)              // SfMvMemory / VisibilityPolicy WaitReleased|OrderedTip
else:
    OptRead(ℓ)
```

Hard forbid: `decide→Block` on Estimate tip; `maybe_wait` Estimate; treating “Estimate present” as published.

### 4.2 Without Blocking park — options to evaluate (implement one; measure TPS)

All options share: **no** Blocking park tax, **no** Rewind tax on thin, **no** antichain drain, **no** Estimate Block, **no** 15-writer begin hold, **no** `mark_gated`, **no** one-shot InconsistentRead.

| Opt | Mechanism | Bound / invariant | Risk if wrong |
|-----|-----------|-------------------|---------------|
| **(i) Short spin / help-release** | While true writer `w` is **Executing** same ℓ, reader spins briefly or helps release waiters when Data lands; else fall through | Bound spin (μs-class / iteration cap); **not** antichain drain; exit if `w` not Executing | Busy-wait wall if bound too high |
| **(ii) Defer pick until pred published** | Scheduler: if WaitOnce armed and pred unfinished, **do not pick** this tx; pick antichain / other Runnable; requeue when pred publishes | **Without** `mark_gated` / Estimate; wake on Data publish only | Under-help if wake missed; over-serialize if wake set too broad (opt-v3 broad plant) |
| **(iii) Early Data / version tip on SF write path** | On SpecFence write path install early **Data/version tip** (`SfMvMemory` `VisibilityPolicy`) as soon as write is known for ℓ — so WaitOnce sees **published** tip | Tip is Data/version, **NOT** Estimate; OCC Estimate path untouched for baseline | API split work; must not leak Estimate semantics into SF read |

**Recommended evaluation order for land:** (iii) if write-path tip is cheap and local; else (ii) with **narrow** wake (EffectiveWAW WaitOnce loc, thin-only, no broad `rset_w` plant); (i) only as micro-bound assist when writer already Executing. Combine (ii)+(iii) if needed; never combine with discarded taxes.

### 4.3 Explicit forbid list (thin Avoid)

- Estimate Block when WaitOnce armed  
- Ungated Blocking park / SoftWait park  
- Thin RewindTo / thin rem checkpoints  
- Broad publish-order plant after FullReplay (empties antichain, `rset_w` collapse)  
- Tip-only short hold / 15-writer hold on 3356896  
- `mark_gated` / gated nearest-pred  
- One-shot InconsistentRead re-read  

---

## 5. Large block `15274915` — hold v2 wins

Thin-only changes. Do **not** regress:

| Keep | Evidence |
|------|----------|
| Sticky ≥32 hold, head-first LIFO, no `mark_gated` | opt-v2: primary wall ~1.58 → TPS ~0.63; head first-start ~0.59–0.65 ms |
| Hang-free early `PartialAbortRewind` + fail_k checkpoint when mid-tx checkpoint exists | `resolve_rewind` ~48–80; `full_from_0` 0–5 (was 32–80) |
| WaitOnce parks when pred live (large allowed) | wait_once ~50–80 per reuse |
| Soft=0, occ_picks=0, explore=0 | met |

If a thin Avoid option touches shared scheduler/write path, gate by `n≤THIN_N` (e.g. 176) or “no sticky chain” so 15274915 path stays bit-compatible with opt-v2.

---

## 6. Acceptance (TPS SF/OCC)

| # | Criterion |
|---|-----------|
| A | Soft=0 Instant-off **N≥5** both blocks; `seq=par`; `occ_picks=0`; `soft_wait_arms=0`; `explore=0`; no hang |
| B | **3356896** TPS SF/OCC clearly+stably **above remasure ~0.70**; prefer **≥0.80** (wall ≤~1.25 if OCC similar). Must beat calm noise band that prints wall 1.4–1.9 |
| C | **15274915** TPS SF/OCC **≥ ~0.63** and **not below opt-v2** (wall primary not worse than ~1.58 on same-host compare) |
| D | **No Estimate Block on SF path** — counter `estimate_block_sf=0` (or equivalent) **or** call-graph audit: SF Avoid/decide never branches on Estimate tip |
| E | Thin: no new park / Rewind / antichain-drain / 15-hold / mark_gated / one-shot reread counters rising vs opt-v2 |
| F | Lib release tests pass; `complete_arch_edge_pi_seq_eq_par_softwait0` pass |

Reports: lead with **TPS SF/OCC**; wall ms as secondary absolute.

---

## 7. Land checklist (CloudAgent — full-batch, no P0/P1/P2)

Single land batch. Coordinator launches CloudAgent separately. This checklist is the whole job.

### 7.1 Code / call-graph

1. Audit SF Avoid path: remove or dead-code any `decide`/`maybe_wait`/Block branch that keys on **Estimate** for SpecFence workers; leave OCC baseline path intact for compare harness.  
2. Implement **one** thin Avoid from §4.2 (document which: i / ii / iii / ii+iii) behind thin predicate (`n≤176` or no sticky≥32 chain).  
3. Enforce read-after-true-publish when AccessArm WaitOnce + true writer (§4.1).  
4. Ensure `consult_ungated_wait_once` remains; do not restore AccessArm skip via `optimistic_skip_gate`.  
5. Do **not** reintroduce: Estimate Block, thin Rewind/checkpoints, 15-hold, `mark_gated`, one-shot InconsistentRead, broad post-FullReplay plant.  
6. Preserve large sticky ≥32 + fail_k Rewind path bit-compatible with opt-v2.

### 7.2 Files (expected touch set — adjust to tree)

- SpecFence Avoid / consult: ungated WaitOnce consume (thin)  
- Scheduler pick / wake (if option ii): publish-wake without `mark_gated`  
- `SfMvMemory` / `VisibilityPolicy` write path (if option iii): early Data tip — not Estimate  
- Counters: `estimate_block_sf` (or rename existing), WaitOnce consume-hit, thin defer-pick, early Data tip install  
- **Do not** edit OCC-only Estimate machinery except to fence SF callers away from it  

### 7.3 Counters / telemetry

- `estimate_block_sf` ≡ 0 on Soft=0 Instant-off both focus blocks  
- `occ_picks=0`, `soft_wait_arms=0`, `explore=0`  
- Thin: FullReplay reuse count trend ↓ without Rewind↑ / park↑  
- Large: `resolve_rewind`≫0, `full_from_0` low, sticky hold intact  
- Report **TPS SF/OCC** primary for N≥5 both blocks vs remasure TPS and vs opt-v2 TPS  

### 7.4 Tests

- Lib release suite (expect ~416 pass class)  
- `complete_arch_edge_pi_seq_eq_par_softwait0`  
- Soft=0 Instant-off focus pair N≥5 harness with `SPECFENCE_COMPARE_CHECK=1`  
- Call-graph / unit: SF path never Blocks on Estimate when WaitOnce armed  

### 7.5 Note / PR

- Land result note: TPS tables for both blocks; which §4.2 option shipped; proof of no Estimate Block; confirm 15274915 not below v2.  
- Tip on pevm branch; sync lab notes via `lab/scripts/sync-to-github.sh` if lab repo writable.

---

## 8. Discarded attempts (do not retry as Avoid)

From opt-v1/v2/v3 — still discarded:

1. Estimate Block when WaitOnce armed  
2. Ungated publish-order after FullReplay — broad plant (antichain / `rset_w` drain) and narrow unstable on large  
3. Tip-only short hold on thin reuse  
4. Thin RewindTo / thin rem checkpoints  
5. 15-writer begin hold on 3356896  
6. Gated nearest-pred / `mark_gated` hold  
7. One-shot InconsistentRead re-read  
8. Two early Rewinds before FullReplay escalate  

Open cut (opt-v3): **execute plane must consume WaitOnce** without those taxes — this design’s §4 is that cut.

---

## 9. Out of scope

- Mixed-49 board (focus pair only for this land)  
- SoftWait≠0 / Instant-on  
- Restoring pe-without-Avoid / sticky Opt as Learn output  
- Editing pevm from the note author agent; CloudAgent lands separately  

