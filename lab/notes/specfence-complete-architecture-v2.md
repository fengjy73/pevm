# SpecFence complete architecture v2 (standalone SoT)

**Date:** 2026-09-13 (Asia/Shanghai, UTC+8)  
**Status:** AUTHORITATIVE design SoT — **single-iteration full land** (no P0/P1/P2 staging). Implementation map: `lab/notes/specfence-architecture-v2-impl.md`.
**Branch / HEAD at write:** `cursor/specfence-complete-cc-63b0` @ `87d3979`  
**Evidence base:** all-blocks SF/OCC@8 across **99** ethereum snapshots (corrected n=98); focus subgrain 597/599/097; process digests on worst family  
**Companion evidence:** `lab/notes/specfence-all-blocks-deep-evidence.md`  
**Vocab (frozen):** **Spec = Region** (not “speculate”). **Fence** = Bind / WaitFor / serial-lane+admit barriers on Regions. **Unfenced** = optimistic access. Product name SpecFence stays.

This document is a **brand-new complete** architecture. It is not a patch note on v1 / complete-cc / AEC. It absorbs full-set distribution + sub-tx grain + learning gaps, and **deletes** dead control planes.

---

## 0. Hard bans (non-negotiable)

| Ban | Why (evidence) |
|-----|----------------|
| SoftWait Soft storms | Wake≪reabort; serializes fan_out; soft=0 everywhere — keep dead |
| EV Await doors / AdaptiveParams-as-Await-θ | Makespan EV is a feature, not the verb; Await@a=0 |
| tip-identity Bind gate | Plant hygiene ≠ π |
| OCC-retry / bare Block-STM reincarnation as **control plane** | Discovery only |
| Morph Storm/Quiet as edge actuator | Morphology is prior / decay, not mode that *is* CC |
| 597-only hardcodes | Full-set median SF/OCC≈0.36 matches focus — generalize |
| Gate salad / OR-bool π | Signals ≠ Edge state |
| Dead AEC theater (`choose_resolve` on access path) | Retired; remove from SoT and eventually code |
| Celebrating abort↓ while ≪OCC | Wall/TPS vs OCC is the bar |
| Unfenced as response to known essential anti-dep | Bounded Unfenced only |

---

## 1. Protocol identity (CC terms)

SpecFence is a **preset-order hybrid OCC** with:

1. **Ahead Region sketch** (predicted writers / hot H / chain templates).  
2. **Fine Detect** on `EdgeKey(ℓ, reader, k, depth)`.  
3. **Avoid** broadcast that turns discovery into Fence.  
4. **Fence verbs:** Bind (version visibility), WaitFor (producer barrier), serial-lane+admit (hang-free when writer absent).  
5. **Bounded Unfenced** for independence / canary / cold discovery only.  
6. **Resolve ladder R1→R4** with **R1-first makespan** (value-stable RebindOnly).  
7. **Live structural learning** that writes **Region/H/residual/ready weights**, not boolean π.

Family: early-visible MVCC, piece-restricted abort, work-conserving schedule, event-driven first-wave learning.

**Not:** SoftWait meta-CC, AEC argmin EV, Storm Await-ready protocol, OCC-lite as the contention response.

---

## 2. System model

### 2.1 Execution

- Block = ordered txs `0..n-1`. Correct commit order = preset order.  
- Workers = P cores (lab: **8**). Useful parallelism ≤ min(P, wave width of independent Regions).  
- Each tx incarnation executes EVM with SpecFence intercepts on storage/account touches.

### 2.2 Objects

| Object | Meaning |
|--------|---------|
| **Location ℓ** | `MemoryLocation` / hash — conflict object |
| **Access** | `(t, k, depth)` — multi-touch / call-frame |
| **Edge** | typed `wr\|rw\|ww` with state unpublished / published-uncommitted / validated |
| **Region (Spec)** | Contended dependency unit around ℓ (and predicted writer chain) — **not** optimism |
| **Fence** | Barrier: Bind Data, WaitFor(w), or WaitFor(pred)+admit_spine |
| **Unfenced** | Optimistic read without barrier |

### 2.3 Success metric

**Primary:** SF wall / OCC wall and SF/OCC TPS on the same block @ same cores.  
**Secondary:** rewind/rebind, park_idle, cold/canary after residual Bind, soft=0, await=0.  
Full-set bar today: median SF/OCC ≈ **0.36** (wall ≈ **2.8×**). Quiet cohort already ≥1 — must not regress.

---

## 3. Conflict / Region model (fine Detect)

Three layers (illegal to flatten to `(ℓ, reader)` when multi-touch):

1. **L_record** — location ℓ.  
2. **L_access** — `(t, k, depth)`.  
3. **L_edge** — typed edge + publish state.

`EdgeTable` keys `(ℓ, reader, k, depth)`. Avoid is per-ℓ. Detect must record every touch that can abort validation.

**Region** aggregates: hot ℓ ∈ H, Avoid-true ℓ, predicted chain writers, force_prefix residual set, multi-spine unfinished writers. Region is the **SoT for Fence**, not a pile of bools.

---

## 4. Fence ontology (native, not if-else)

Edge control is a **state machine**, not `must_wait = a∨b∨c∨d`.

```
EdgeView → verb:

Published Data (incl. Executed-not-Validated tip)
  → Bind(version)                         # A3 visibility

Unpublished ∧ predicted/essential anti-dep ∧ writer w < reader
  → WaitFor(w)                            # Fence barrier
     · if w Executing: park BlockingOther + steal Ready
     · if w Ready: PreferAdmit(w) (stay Fenced; never Unfenced hang-freedom)
     · if w Done ∧ Data now visible: Bind (race)

Essential / Avoid / force_prefix ∧ writer = None ∧ reader > 0
  → WaitFor(reader-1) + admit_spine       # serial-lane Fence

Independence-certified (ℓ ∉ Region Fence set)
  → UnfencedIndependence

Canary grant open (first-wave probe, no Avoid yet)
  → UnfencedCanary

else cold discovery
  → UnfencedCold
```

**Laws:**
- Known essential ⇒ Bind or WaitFor — **never** Unfenced+retry.  
- Hang-freedom = admit + steal, **not** Unfenced.  
- Reasons are metrics-only; control from Edge/Region state.  
- Implementation today still has OR-salad in `edge.rs::choose_edge_action` — v2 **requires** dissolving it into the machine above (no new θ flags).

---

## 5. Avoid / schedule / parallel compute laws

### 5.1 Avoid

- First abort / first validated anti-dep on ℓ → **Avoid broadcast** immediately.  
- After Avoid: subsequent edges Fence (Bind or WaitFor), not Unfenced.  
- Residual Bind on **Done writers** when Data missing was the hole that produced `unfenced_writer_done` — **closed** via `note_writer_done` → residual Bind. Keep closed.

### 5.2 Schedule (parallel compute)

- Ready queue work-conserving; `ready_steal_on_wait` on WaitFor parks.  
- **PreferAdmit** unfinished spine writers when Ready (S1).  
- Fan_out law: wave independents must keep **useful P≈8**; WaitFor may serialize RAW clique only.  
- Park budget: parked frame must not burn wall while independents starve — **continuation / steal width**, not SoftWait Soft wake storms.  
- Evidence: `6196166` park_idle≈0.93; `14689597` last-iter idle≈0.52; mid `10760440` idle≈0.55.

### 5.3 Parallel compute vs CC (diagnosis law)

| Class | When | Actuator |
|-------|------|----------|
| CC Avoid miss | Unfenced on essential | Fence / residual Bind |
| CC Resolve miss | abort → expensive R2/R4 | R1-first, identity carry |
| CC cold rediscovery | UnfencedCold scales with inc | incarnation-stable residual |
| COMPUTE park | WaitFor BlockingOther on fan_out | park budget / PreferAdmit |
| Wrong Fence on indep | rare today | keep independence cert |

---

## 6. Resolve (R1-first makespan)

### 6.1 Ladder (prefix law)

| Rank | Name | When | Cost intent |
|-----:|------|------|-------------|
| **R1** | RebindOnly | `identity_stable_match` ∨ FF value-stable ∨ certified prefix held | **Default cheap** |
| R2 | SuffixRepair | true suffix invalid; prefix held | Body reexec **with** residual/identity carry |
| R3 | ForceBind extend | sticky conflict locations | Arm residual, prefer R1 next |
| R4 | FullRestart | prefix false / depth cap | **Failure** — escalate rare |

### 6.2 Makespan law (from all-blocks)

Across fan_out majority and global worst `19807137` (rewind≈2002, rebind≈113):  
**wall is SuffixRepair body reexec**, not missing Avoid on star.  
`writer_identity_preserved` and `journal_ff_hits` are abundant; **R1 underfires**.  

**v2 mandate:** `try_validate` / rem path must prefer R1 whenever `rem.rs::identity_stable_match` holds; FF hits promote to RebindOnly count, not only cheapen inside R2 continuation.

### 6.3 Incarnation-stable residual

Repair incarnations must **not** re-cold-miss ℓ already seen on prior inc (597 tx72: tiny gas, inc=9, cold 37). Carry residual Bind / origin map across incarnation.

---

## 7. Learning that is live structural

### 7.1 What to learn (keep / add)

| Signal | Sink | How used |
|--------|------|----------|
| writer_done / after_avoid | `LiveLearner::note_writer_done` → pack_top / H | Residual Bind + hot promote — **KEEP** |
| bind_success / bind_cover | H / cover scores | Prefer Fence cover — **KEEP** |
| canary probe done | `reopen_canary_if_probe_done` | Early Fence after probe — **KEEP** |
| TopLocPrior / chain templates | sketch seed + decay on flip | Warm H — **KEEP** with quiet decay |
| identity_stable / FF match rates per ℓ | **Resolve prior** | Bias R1 vs R2 — **ADD as live** |
| cold-on-repair-inc per ℓ | incarnation residual map | Bind residual next inc — **ADD** |
| park_ns on fan_out clique | ready weights / PreferAdmit heat | Schedule priority — **ADD structural**, not EV Await |
| rewind:rebind ratio | resolve policy telemetry | Falsifier / canary for R1-first — **ADD metric→policy** |

### 7.2 What to delete (dead after AEC retire)

| Item | Fate |
|------|------|
| `choose_resolve` / AEC EV Await doors | **Delete** from SoT; code remains retired until removed |
| AdaptiveParams αβγδ / d_wait / cost_margin / meta_budget as π | **Delete** |
| SoftWait meta_ops / meta wake credit | **Delete** |
| engagement Quiet/Storm as edge actuator | **Delete**; morph EMA may remain for **decay/warm only** |
| tip-identity Bind refuse | **Delete** as π |

### 7.3 Learned-but-unused today (must wire or delete)

| Learned | Unused how | v2 action |
|---------|------------|-----------|
| identity_preserved + FF | Inside R2; not R1 default | Wire as R1-first law |
| PreferAdmit counter | Thin on cold; Ready window miss | Structural ready weights from park heat |
| MorphWeights / engagement | Banned from edge (ok) but still theater | Decay-only; strip Storm Await |
| wait_park_ns → e_idle | EV retired | Feed PreferAdmit / steal priority |
| ChainTemplate.confidence | serial_lane only | Optional Region weight; else delete confidence |

### 7.4 Should-learn but missing

1. Cold-edge residual across incarnations.  
2. Long-tail ℓ promotion **before** first Abort (pack_top lag: canary≪fence_seq).  
3. R4/force_bind_reabort as resolve features (not only note_abort).  
4. L1 RAW depth / producer_effect_k online (optional; offline DAG for falsifiers).  
5. Metric-morph vs L1 DAG calibration on all 99 (heuristic over-calls fan_out).

### 7.5 Inter warm

- Warm-start H + templates with **flip decay** and **quiet morph extra decay** (U6).  
- Never plant Bind priors that flip quiet→fan_out (protect SF/OCC≥1 cohort, n≈18–26).  
- Warm PreferAdmit helps under heat (597 xblock PreferAdmit=12) but does **not** replace R1-first.

---

## 8. End-to-end control loop

```
begin_block:
  seed sketch from InterBlockPrior (H, templates) with morph-flip/quiet decay
  engagement := decay-only label (not edge π)

per access EdgeKey:
  Detect → update EdgeTable / Region
  verb := Fence state machine (Bind | WaitFor | Unfenced*)
  if WaitFor: PreferAdmit Ready spines; park+steal if Executing
  if Done∅Data under Avoid: residual Bind (never UnfencedWriterDone)
  learn: note_writer_done / canary reopen / bind_cover

per validation abort:
  if identity_stable_match ∨ FF stable → R1 RebindOnly
  else if prefix held → R2 SuffixRepair + carry residual map
  else → R3/R4 rare
  learn: identity/FF rates, cold-on-inc, rewind:rebind

end_block:
  pack_top_locations → InterBlockPrior EMA
  morph hat for decay only
  emit falsifier metrics (soft, await, writer_done, R1/R2, park_idle, SF/OCC)
```

Single live access π: **Fence state machine**. Single live resolve π: **R1-first prefix law**. Learning writes **structure**, never OR-gates.

---

## 9. EVM substrate mapping

| EVM / pevm | SpecFence |
|------------|-----------|
| SLOAD / BALANCE / … touch | Detect EdgeKey; maybe_wait_specfence |
| MvMemory published write | Bind version (A3; no Validated gate) |
| Tx Executing / Ready / Done | WaitFor vs PreferAdmit vs residual Bind |
| Validation fail locations | Resolve R1–R4; force_bind residual |
| Call depth / access k | EdgeKey depth/k — multi-frame Regions |
| Journal / FF prefix | identity_stable_match / RebindOnly |
| Scheduler ready / steal | work-conserving + PreferAdmit spines |
| OCC baseline | same block runner @8 — TPS bar |

---

## 10. Correctness

1. **Preset order:** commit serialization ≡ tx index order.  
2. **Fence soundness:** essential anti-dep never Unfenced after Avoid/residual laws.  
3. **Bind safety:** Bind only to published Data versions readable under MVCC rules.  
4. **Hang-freedom:** every WaitFor has admit/steal progress or Bind race.  
5. **Independence:** UnfencedIndependence only when ℓ ∉ Fence set — abort still safe via OCC validate.  
6. **Repair identity:** R2/R4 must not invent writers; residual map monotonic.  
7. **Bans:** soft=0, await=0 invariant in CI falsifiers.

---

## 11. Falsifiers from all-blocks distribution

| Falsifier | Expect (post-v2) | Today (corrected) |
|-----------|------------------|-------------------|
| soft_wait_arms / await_at_a | **0** all blocks | 0 |
| unfenced_writer_done / u_aa / hot_after_fence | **0** | 0 on digests |
| median SF/OCC @8 | **↑ toward ≥0.7** then ≥1 on fan_out pack | **0.36** |
| worst SF/OCC (N≥3) | **≫ 0.09** | 19807137 = 0.090 |
| rewind:rebind on hot fan_out | **rebind ≳ 0.5·rewind** when id/FF high | 19807137: 113 vs 2002 |
| park_idle on fan_out | **≪ 0.5** without SoftWait | 6196166 ≈0.93; 597 ≈0.5 |
| quiet cohort SF/OCC | **stay ≥1** | median quiet ≈1.13 |
| PreferAdmit under park heat | fires when Ready spines exist | thin on cold; better warm |
| morph heuristic vs L1 | calibrated labels | over-fan_out |

N=1 alone is insufficient for ranking extremes (19434587 OCC spike; 2179522 Bind storm) — falsifiers use **N≥3 median**.

---

## 12. Roadmap — single-iteration full land

Phased P0/P1/P2 staging is **rejected**. This cut lands **every** item below in one iteration / one PR. There is no “later P2” list.

| # | Work | Done when |
|---|------|-----------|
| 1 | R1-first resolve when `identity_stable_match` / FF value-stable | `rebind_only` fires; R2/R4 only when identity lost |
| 2 | Incarnation-stable residual / cold carry | Bind residual + snaps survive repair incarnations (tx72-class) |
| 3 | WaitFor park budget + PreferAdmit heat | P cores stay on independents during fan_out Wait; **no SoftWait** |
| 4 | Dissolve `choose_edge_action` OR-salad → Edge/Region version-visibility SM | native CC verbs, no new θ flags |
| 5 | Wire `park_ns` + rewind:rebind into structural learners | Fence / admit / Resolve **read** those signals |
| 6 | Delete AEC / AdaptiveParams / SoftWait theater from live paths | Storm/Await/αβγδ not π |
| 7 | Metric↔L1 morph calibration | safe decay, not Storm actuator |
| 8 | Protect quiet Fence-off | no quiet→fan_out regression |
| 9 | Generalize `fanout_fr_collapse` / absorb | morphology-agnostic (19807137 / 19434587 class); **no bn hardcodes** |

Implementation file:fn map: `lab/notes/specfence-architecture-v2-impl.md` (every row marked **landed**).

---

## 13. Worked examples

### 13.1 Worst — 19807137 (fan_out, SF/OCC 0.09)

- Star Bind cover holds; writer_done=0.  
- Residual Bind 5–9k used; multi_spine_admit live.  
- Failure: **R2 rewind ~2k** with rebind ~100; FF/identity unused as R1.  
- Ideal: R1-first + residual carry → wall toward OCC ~12 ms; keep P on independents.

### 13.2 Quiet — 2179522 / 14689595 (SF/OCC ≥1)

- Fence almost off; meta cheap.  
- Ideal: **keep** UnfencedIndependence; decay any warm Fence priors; do not arm Storm.

### 13.3 Focus — 14689597 fan_out

- Top txs 72/71/60/43: cold-on-repair + Wait park; Avoid on star OK.  
- Ideal P≈8 on wave 434; serialize RAW chain ~29 + star readers.  
- Levers: R1-first, cold carry, park budget.

### 13.4 Focus — 19606599 / 19469097 long_chain

- Wall = abort/repair + canary first-wave; idle low.  
- Tips tx322 Bind-heavy max_inc — identity/repair cascade.  
- Levers: R1 over R2; PreferAdmit spines (live) + cheap resolve.

### 13.5 Mid fan_out — 19737292 / 12243999 (SF/OCC≈0.335)

- Same class as IQR commons: residual Bind used, R1=0, light park, wall ~3×.  
- Proves focus was representative; fixes must be universal.

---

## 14. Architecture one-liner

**SpecFence v2 = Region/Fence state machine + residual Bind (done) + PreferAdmit spines (done) + R1-first value-stable Resolve + incarnation-stable residuals + fan_out park budget — learning writes structure, never SoftWait/AEC/Storm π — until SF/OCC approaches 1 on the fan_out majority without regressing quiet.**

---

## Appendix A — Evidence pointers

- Sweep: `lab/notes/specfence-all-blocks-sweep-summary.md`  
- Deep tables: `lab/notes/specfence-all-blocks-deep-evidence.md`  
- Focus subgrain: `lab/notes/specfence-post-subgrain-multiblock-deep-diagnosis.md`  
- JSON: `lab/results/all-blocks-sf-occ-*.json`, `all-blocks-process-*.json`, `post-subgrain-*`

## Appendix B — File:fn map (v2 land)

Authoritative landed map: `lab/notes/specfence-architecture-v2-impl.md`.

| Law | file:fn |
|-----|---------|
| Edge verb | `specfence/edge.rs::classify_edge` → `choose_edge_action` |
| Wait plant | `vm.rs::fence_wait_for` / `maybe_wait_specfence` |
| Residual writer_done | `learner.rs::note_writer_done`; residual Bind path |
| Incarnation carry | `rem.rs::PartialRetryState::reset` / `inc_carry_*` |
| Canary reopen | `sketch.rs::reopen_canary_if_probe_done` |
| PreferAdmit / park budget | `scheduler.rs::admit_spine_heat`; `learner.rs::prefer_admit_heat` |
| Identity / R1 | `rem.rs::identity_stable_match`; `pevm.rs::try_validate` |
| Structural collapse | `pevm.rs::scan_invalid_spine` / `structural_spine_hot` |
| Retired AEC | `mod.rs::choose_resolve` dead; `resolve.rs` not live π |
| Morph decay / quiet | `sketch.rs::seed_from_prior_morph`; `learner.rs::morph_hat` / `quiet_fence_off` |
