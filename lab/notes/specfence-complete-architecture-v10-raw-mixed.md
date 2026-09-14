# SpecFence complete architecture v10 — RAW_fan_out + mixed_RAW_WAW (AUTHORITATIVE SoT)

**Status:** AUTHORITATIVE design + land SoT for this PR.  
**Date:** 2026-09-14  
**Vocabulary:** [`specfence-cc-glossary.md`](specfence-cc-glossary.md) ONLY.  
**Target set:** 52 ids in [`specfence-high-bound-occ-gap-block-ids.txt`](specfence-high-bound-occ-gap-block-ids.txt)  
(`RAW_fan_out` ∪ `mixed_RAW_WAW`). Appendix `WAW_spine` / `near_independent_meta_gap` are **out of scope**.  
**Spine:** one pevm parallel executor. CC / PC / Bayes are **analysis lenses**, not ownership directories.  
**File law:** file-SRP (v9.4). Soft=0. No P0/P1/P2. No SoftWait Soft.

Supersedes for **this morphology pair** (does not void spine unity or glossary):

- v9.1 bars + call-flow (kept as the fused order)
- v9.3 one pevm spine (kept)
- v9.4 file-SRP (kept)
- v9.4 timely-Resolve land (`k≥8` rem ResumeAtK, Ready-refuse without wave fill)

Parent honesty: [`specfence-v9.4-why-still-slower-than-occ.md`](specfence-v9.4-why-still-slower-than-occ.md),  
[`specfence-v9.4-timely-resolve-honesty.md`](specfence-v9.4-timely-resolve-honesty.md).

---

## 0. Essence (one paragraph)

**v10** is a fused Detect → Avoid → Resolve plant on the **one pevm spine**, specialized to the 52-block high-bound shortfall set. **CC lens:** kill `optimistic_read` → `full_abort_reexecute` storms on `RAW_fan_out` by schedule-first `refuse_admit` of known consumers, and resolve mid-tx RAW only with `wait_for_dependency` that **resumes** (same incarnation, prefix skip) or `partial_abort` when strips cover — never park-then-`full_abort_reexecute` as the default. **PC lens:** when a consumer is refused, **wave-fill** the core with an independent `optimistic_read` instead of spinning the refused head; ProducerStage still runs the star writer. **Bayes lens:** query ports at admit / decide / validate close the loop — abort raises \(P_{\mathrm{RAW}}\) so the next wave refuses or waits; success keeps independents on `optimistic_read`. Independents never take a Fence verb. Ordered admit stays rare. Soft=0.

```
fan_out:   refuse known consumers → producer publishes → consumers optimistic_read Data
           mid-tx wait_for only if rem prefix skip is real; else OCC-equal optimistic_read
mixed:     same RAW edges; fill remaining wave width with independents
indep:     optimistic_read forever (empty PE ∧ no ReadyEdge)
```

---

## 1. Why v9.4 still lost to OCC (absorbed, not redesigned away)

Wall equation (v9.1): `wall = useful_EVM + idle + repair + meta`.

| Fake weld | Symptom | v10 duty |
|-----------|---------|----------|
| WaitForDependency park **without** resume | wake → `FullAbortReexecute`; `park_resume_full_abort_reexecute` ≈ wait volume | **Ban park unless `armed_at_k>0`**. Empty / tiny prefix = `optimistic_read` (OCC-equal), not idle+head reexec |
| Ready-refuse without wave fill | 19606599 refuse **31k** (spin); cores idle on deferred head | **Skip refused consumer; steal next independent** |
| Storage true-k only Basic(addr)@k≈6 | PE miss → `optimistic_read` of unfinished storage RAW | Seed InterPrior storage PE **and** `note_raw_producer` on star Basic so refuse covers the account class |
| `note_fence_success` on Done / empty Wait | sibling optimistic_read uncertified → `partial_abort` bait | Cert **only** a Wait that actually parks with a resume-able prefix |
| decide WaitFor only if `writer_executing` | Ready producer → canary `optimistic_read` → abort | Single unfinished writer + EV → `WaitFor`; `act_wait_for` still Ready/Executing |
| Bayes unused at admit | first-wave satellites Execute before edges exist | `query_admit` seeds PE / edges for high \(P_{\mathrm{RAW}}\) |
| 19807137 hang gates as product law | `k≥8` rem + Ready-refuse fear | **19807137 is WAW_spine — excluded.** Keep rem `k≥8` (tiny ResumeAtK is tax). Do **not** park when rem cannot cash |

Falsifier still live on 14689597 @ timely-Resolve: WaitFor↑ ∧ abort>OCC ∧ `partial_abort`≈0. v10 kills that shape: **refuse ≫ mid-tx Wait**, **abort_SF ≤ OCC** on fan when edges exist, **`park_resume_full_abort_reexecute` ≪ `wait_for_dependency`**.

---

## 2. Morphologies (theoretical bound)

Equal-cost DAG bound @8: `min(8, n/L, W)`. The 52-set has **high bound** and **OCC≪bound** because of conflict waste, not meta.

| Class | DAG | OCC failure | v10 Avoid / Resolve |
|-------|-----|-------------|---------------------|
| `RAW_fan_out` (3; anchor **14689597**) | RAW-heavy, wide \(W\), short-ish \(L\) | Many consumers `optimistic_read` unfinished / wrong version → validate → **full abort storm** | Seed ReadyEdges + ProducerStage **before** satellite Execute. `refuse_admit` while producer Ready\|Executing. After publish: `optimistic_read` Data. Mid-tx `wait_for_dependency` **resumes** or does not park |
| `mixed_RAW_WAW` (49; 19606599 / 19469097) | RAW + WAW, high \(W\) | Mix of RAW abort + WAW validate; **width exists** | RAW edges as above. **Wave fill** independents. WAW stays `optimistic_read` + OCC validate (do not serialise the suffix) |
| independents (inside both) | no RAW/WAW edge | — | `optimistic_read` (OCC-cost). Never `refuse_admit` / WaitFor |

**Approach the bound** = raise useful_EVM fraction: producer + independents run now; consumers run after publish **without** reincarnation.

---

## 3. Triple lens (fused on one spine)

```
PC:    admit, Stages, ready/steal, ProducerStage, WaitFor hold, wall = useful_EVM+idle+repair+meta
CC:    Detect / Avoid / Resolve, Mode(a), certs, validate / partial_abort
Bayes: P_RAW, PE(ℓ,k), EV[refuse|wait|optimistic_read|ordered_admit|full_abort], queried at ports
```

**Ban:** `pc/` `cc/` `bayes/` ownership directories. **Ban:** dual OCC/SF computers. Cold = `optimistic_read` cost class on the **same** `next_sf_task` spine.

Call order (v9.1 kept, duties completed):

```
begin_block:  Bayes.seed → admit_seed (ReadyEdges + PE true-k + ProducerStage)
schedule:     ProducerStage(w) ∪ Execute(t) if ∀edges Done ∨ ¬refuse
              refuse → wave-fill independent; never ReadyCanary of a known consumer
execute a:    k := ordinal; vis; q := Bayes.query_access; verb := decide(q)
              OptimisticRead | WaitFor(resume-armed) | SerialLane | OrderedAdmit(rare)
validate:     RS_spec bool; strips → PartialAbortRebind / PartialAbortRewind; else full_abort
              Bayes.observe(outcome) → next admit/decide
```

---

## 4. CC — timely Avoid / Resolve

### 4.1 Detect

- PredictedEssential at **true stream \(k\)**, never residual-1 / template `[1,6,10,20]`.
- `admit_seed_begin_block`: hint-fan accounts + InterPrior storage tops + Bayes `query_admit`.
- Star Basic(addr) also `note_raw_producer` so storage RAW of that account is a **known** edge class.
- First storage access of a star account plants PE at live \(k\) (already). Abort trains true-\(k\) + `admit_seed_on_abort`.

### 4.2 Avoid (cheapest first)

```
1. refuse_admit     known consumer, producer Ready|Executing     # schedule-first
2. wait_for_dependency   mid-tx, single unfinished writer, rem prefix skip armed
3. SerialLane       multi-writer PE, EV
4. OrderedAdmit     published conflict-tip ∧ ev_ordered_admit_beats_full_abort only
5. optimistic_read  independents, PE-miss, empty prefix, Done producer (cert=false)
never: park then full_abort_reexecute as the WaitFor default
never: ReadyCanary of a known edge
last:  full_abort_reexecute when no honest prefix and validate fails
```

**WaitFor resume law**

- `arm_wait_for_dependency_checkpoint` returns `armed_at_k>0` only when snapped prefix skip is real (`k≥8`, same rem bar — tiny ResumeAtK is tax).
- `armed_at_k==0` → **do not park**. Edges already reserved. Continue `optimistic_read` (OCC-equal). This is the 14689597 honesty fix.
- Wake: `try_arm_wait_for_dependency_resume_at_k` → ResumeAtK (no force-ordered_admit). Soft=0.
- First access / empty snap → FullAbortReexecute **only if we parked**. We no longer park that case.

**Partial abort**

- `covers_all` + value-stable → `PartialAbortRebind`.
- Strip-cover + one RewindTo → `PartialAbortRewind` (`suffix_repair_depth==0`).
- Second strip-cover without progress → honest `full_abort_reexecute`.
- Do not increment `partial_abort_attempt` unless RewindTo arms.

### 4.3 Cert

- `note_fence_success` only when WaitFor **parks with resume armed**, or OrderedAdmit actually saw Data.
- DoneOptimisticRead `cert=false` (kept).

---

## 5. PC — wave fill / multi-core

```
next_sf_task:
  ProducerStage(w) first (promote if off collaborative index)
  next_task_with_wave_ready:
    try execution_idx
    if refuse_admit(t←w):
      defer t (idempotent — do not spin-count)
      admit_spine(w)
      steal first independent in (t+1 .. t+FILL)     # WAVE FILL
      do not fetch_max(t+1) in a way that fights admit_spine(w)
    independents: optimistic_read immediately
  park (resume-armed WaitFor only): steal producer, then independents
```

`refuse_admit` is **Avoid**, not a trophy. Useful iff `refuse ≫ mid-tx Wait` **and** abort≪OCC **and** cores stay busy (independents execute).

ProducerStage + wave ready wake consumers on `note_producer_done`. No v6 “defer everyone, nobody runs \(w\)”.

---

## 6. Bayes — closed loop

| Port | Query | Actuator |
|------|-------|----------|
| begin_block `query_admit` | \(P_{\mathrm{RAW}}(\ell) ≥ τ\) | seed PE + ReadyEdge / ProducerStage |
| decide `query_access` | PE, \(P_{\mathrm{RAW}}\), depth_frac, EV[wait vs abort], known_star | WaitFor / OrderedAdmit / optimistic_read |
| validate `query_validate` | P(covers∣strips), tip snap | PartialAbort vs full_abort |
| abort `observe_conflict` | location + true \(k\) | next wave refuse / WaitFor |
| success `observe_ok` | independent / healed RS | keep PE from spraying |

`depth_frac` is high for a **single unfinished** producer (Ready **or** Executing). `known_star` opens WaitFor/refuse, **never** OrderedAdmit.

OR-bool Fire remains a no-query test adapter only.

---

## 7. File-SRP (no new kingdoms)

| File | One job (v10) |
|------|----------------|
| `admit.rs` | begin_block / abort seed (edges + PE + star tip) |
| `ready_edge.rs` | known-consumer membership; **idempotent** defer |
| `producer_stage.rs` | reserve / promote |
| `computer.rs` | `next_sf_task` (ProducerStage then wave) |
| `scheduler.rs` | refuse + **wave fill** + wait_for_dependency park/wake |
| `access_policy.rs` | sole live Mode(a) decide ← Bayes |
| `fence_act.rs` | WaitFor / DoneOptimisticRead / park-only-if-armed |
| `bayes.rs` | posteriors + query ports (not π) |
| `feeder.rs` | seed / observe into learner+Bayes |
| `executor.rs` | validate / partial_abort / OCC kernel |
| `rem.rs` | rem checkpoint / ResumeAtK / PartialAbortRewind |
| `vm.rs` | EVM host; calls fence_act / decide (no new π) |

Museums stay gated. Dual π stay `#[cfg(test)]`.

---

## 8. Product bars (this collection)

Sweep default = 52-id file. Soft=0 required.

| # | Metric | Bar (honest ship) |
|---|--------|-------------------|
| B1 | Nonempty median SF/OCC TPS @8 | beat prior wall **0.703** (product stretch 0.95) |
| B5 | **14689597** @8 **N≥3** | beat prior **0.449**; honesty if still `wait→full_abort` |
| B7 | `soft_wait_arms` | **0** |
| B8 | `partial_abort` win / attempt when strips cover | not theater (win≈attempt if attempt>0) |
| B10 | WaitFor↑ ∧ abort≈OCC | **forbidden** as a win claim |
| B12 | abort SF/OCC on 14689597 when edges present | ≤ 1.0 target |
| — | `park_resume_full_abort_reexecute` | ≪ `wait_for_dependency` |
| — | OrderedAdmit | rare (≪ WaitFor / refuse) |

**Do not celebrate** `wait_for_dependency`↑ or `refuse_admit`↑ alone.

---

## 9. Hard bans

- SoftWait Soft (`soft_wait_arms>0`)
- Convert `wait_for_dependency` → `full_abort_reexecute` as the **default** wake
- Park when rem cannot ResumeAtK
- ReadyCanary of a known consumer
- Suffix-global refuse (v6 deadlock)
- Dual computer / `next_occ_task` retreat
- `pc/` `cc/` `bayes/` dirs as SoC
- Template PE `[1,6,10,20]`
- OrderedAdmit-after-Done as Avoid
- Celebrating label moves without abort↓ and useful_EVM↑
- P0/P1/P2 staging

---

## 10. Land order (this PR — full batch)

1. SoT (this file).
2. Plant: admit star tip + Bayes `query_admit`; decide single-unfinished WaitFor; fence_act park-only-if-armed; scheduler wave-fill + idempotent defer; validate loop unchanged except Bayes observe already on abort.
3. Units: refuse wave-fill, no-park-without-prefix, storage/star tip seed, Bayes admit.
4. Soft=0 seq≡par (existing evm / lean fixtures).
5. Soft=0 sweep on 52; 14689597 N≥3 honesty.
6. Impl map [`specfence-v10-raw-mixed-impl.md`](specfence-v10-raw-mixed-impl.md).
