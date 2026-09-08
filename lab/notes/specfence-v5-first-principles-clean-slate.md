# SpecFence v5 — EVM × CC first principles, clean slate

**Date:** 2026-09-07 (Asia/Shanghai)  
**Status:** AUTHORITATIVE REDESIGN — supersedes stacked v1–v4 / plant-v2 milestone soup / HotSet-CC / control-law-v3 Boolean / AEC-as-patch  
**Code tip at freeze:** `0d17b89`  
**Imperative:** shovel sedimentary control planes; keep only what the principles require.

---

## 0. Why the system is sick

~10.7k LOC under `specfence/` + 40+ design notes encode **overlapping eras**:

| Era | Residue still in tree |
|-----|------------------------|
| PCC/heat sticky Wait | `HeatMap`, `RegionTable` account Wait, `seed_wait_regions` |
| Spec v1 Bayes+τ | `BayesMap` dual with AEC EV |
| HotSet / Engagement ladders | `H_w`/`H_a`, abort_rate cuts, Lean vs HotLocal story |
| Plant v2 M1a–M1l | `boundary.rs` ~2.3k LOC inspect/jump — **off by default**, hang history |
| FenceGraph + RegionTable mirrors | two Wait authorities |
| Control law v3 Boolean | mostly deleted by AEC but constants/docs remain |
| AEC + abort-cheap patch | correct *direction*, still bolted onto the pile |

**Symptom:** SF/OCC ~0.16 on 597 after heroic patches — not because EV math is unknown, but because **meta, dual policies, and disabled plant** fight the Block-STM scheduler.

**Diagnosis:** SpecFence today is not one CC algorithm. It is a **museum of attempts**. First-principles redesign means **delete until one algorithm remains**.

---

## 1. EVM execution first principles (non-negotiable)

1. **Consensus total order** \(T_0,\ldots,T_{n-1}\) is fixed. Commit reorder is illegal.  
2. **Sequential ≡ parallel** on state, receipts, gas — TCB. Learning never commits.  
3. **RW sets are dynamic.** Locations appear as the interpreter runs.  
4. **Scarce resource = useful interpreter-seconds on the true dependency critical path** (+ core idle). Not SoftWait count, not HotSet size.  
5. **EVM cost ≠ MV lookup.** Repair that re-enters the interpreter is expensive; ESTIMATE cascades are expensive.  
6. **Lazy beneficiary / LazySender-Recipient** commute — never speculative Wait/learn hot path.  
7. **Validators must replay.** Priors = function of agreed history / process snapshot at block start, or identical warm-start.

---

## 2. Concurrency-control first principles

1. **Concurrency object = location \(\ell\)** (`MemoryLocation`), versions in commit order.  
2. **Finest event = access \(a=(t,k,\ell,m)\)** — but **scheduling grain** may stay tx until mid-tx resume is hang-free.  
3. **True DAG \(G^\star\)** unknown a priori → discover online (speculate) or wait for producer.  
4. **OCC (Block-STM)** is the strong baseline: speculate → validate → abort/reexec. Winning means **beats OCC makespan**, not “more mechanisms.”  
5. **Hybrid actions** at a conflicting read: Bind (known version) / SpecRead (dirty) / Wait (block until Data) / EarlyAbort (cut early).  
6. **Policy must optimize expected makespan**, not Boolean features. Features inform EV; they must not be gates.  
7. **One policy choke point.** No post-π escalate, no sticky account Wait, no HotSet Wait gate, no Bayes bool alongside EV.

---

## 3. The single algorithm (SpecFence v5)

```text
Block-STM scheduler + MvMemory          # L0 unchanged
    ↑
FenceGraph: SoftWait / wake only        # L2 — scheduling constraints
    ↑
π = argmin EV[Bind, Wait, Spec, Early]  # L3 — sole decision (AEC kept, purified)
    ↑
OutcomeLearner: P_abort, T_wait, C_reexec, cascade  # L4 — continuous θ
    ↑
RepairPlant: force-bind certified + selective invalidate;   # L1 repair
             RewindTo/FF only behind proven hang-free flag
```

**Default:** SpecRead (OCC-like). Wait only when \(\mathrm{EV_{Wait}} < \mathrm{EV_{Spec}}\) with **measured** wake/abort costs.  
**Fan-out:** raises \(\mathrm{EV_{Wait}}\) (discourage serialize).  
**Abort:** cheapen with force-bind + selective invalidate on Lean; never require inspect for correctness.

---

## 4. Shovel list — DELETE / DEMOLISH from SpecFence control

| Item | Action | Why |
|------|--------|-----|
| `HeatMap` + heat-driven seed Wait | **Remove from SpecFence path** (PCC may keep) | Sticky heat ≠ makespan EV |
| `RegionTable` account Wait / `promote_account` on SpecFence | **Delete calls**; optionally delete account map later | Account grain false Wait |
| `RegionTable` location Wait as authority | **Delete authority**; FenceGraph only (mirror optional then gone) | Dual Wait |
| HotSet `H_w`/`H_a` as any policy input to Wait | **Demote to optional dense-stat cache or delete** | Threshold ladder |
| `AdaptiveEngagement` abort_rate ladders | **Delete / no-op** | Ladder; Lean is default execute |
| Bayes `should_wait_hard` / τ Boolean Wait | **Delete from SpecFence π**; keep Beta only as \(P_{\mathrm{abort}}\) feature for EV | Dual policy |
| control-law-v3 Boolean leftovers (`want_wait`, D_WAIT Wait force) | **Gone** (verify none remain) | |
| M1d–M1l **default-on** inspect/jump stories in mod docs | **Docs: research-only**; code stays behind `SPECFENCE_ENABLE_INSPECT` | Hang museum |
| `seed_wait_regions` for SpecFence | **No-op** | Prior must not arm SoftWait |
| Finegrain in production Ctx | Keep opt-in lab only | Measurement ≠ control |
| Duplicate metrics eras | Trim later | Noise |

**KEEP (purified):**
- `FenceGraph` SoftWait + WavePark (M2)
- `choose_action` AEC argmin EV
- `LiveLearner` / `InterBlockPrior` as θ only
- `PartialRetry` force-bind + selective invalidate (abort cheapening)
- `RwPriorMap` as Bind feature (not Wait gate)
- Block-STM validate / ESTIMATE fence for correctness
- finegrain research collectors

---

## 5. Clean module target (after shovel)

```text
specfence/
  mode.rs          # ConcurrencyMode
  fence.rs         # FenceGraph + SoftWait (ex-dag)
  policy.rs        # PolicyCtx + choose_action AEC (ex-resolve)
  learner.rs       # θ: P_abort, T_wait, C_reexec, morph features
  repair.rs        # PartialRetry force-bind / selective / optional RewindTo
  wave.rs          # WavePark steal
  prior.rs         # RwPriorMap Bind features
  metrics.rs       # slim
  finegrain.rs     # lab only
  boundary.rs      # research inspect — feature-gated, not in default Ctx path
```

Rename can be incremental; **behavioral shovel first**, file rename second.

---

## 6. Correctness & performance contracts

**Correctness:** existing seq≡par tests; SpecFence ≡ sequential on Ethereum fixtures.  
**Performance:** optimize **SF/OCC@8 makespan** on 597/599/097/598; success = sustain SoftWait≪G7 while SF/OCC → toward OCC (not by Wait storms).  
**Forbidden “wins”:** raising SF/OCC by restoring fanout→WaitHard.

---

## 7. Implementation phases (v5)

### V5-P0 — Shovel (this sprint)
1. SpecFence: disable Heat seed, account promote, RegionTable Wait authority, Engagement ladders, Bayes Boolean Wait.  
2. π only AEC; Bayes feeds `posterior_conflict` / bind success as EV features.  
3. HotSet unused by π (or remove inserts from hot path).  
4. Rewrite `mod.rs` header to v5; add this note as authority.  
5. Tests: seq≡par + AEC unit + 597 smoke SoftWait scarce.

### V5-P1 — Single repair story
Lean abort always force-bind+selective when prefix exists; one code path; delete dead FullRestart branches where safe.

### V5-P2 — θ quality
Wire wake latency / reexec into EV tightly; meta budget as measured tax → SpecRead.

### V5-P3 — Research plant
Inspect/jump remains opt-in; only graduate pieces that beat OCC on 597 hang-free.

---

## 8. Bottom line

SpecFence v5 = **Block-STM + FenceGraph SoftWait + one makespan-EV π + continuous outcome learner + cheap Lean repair**.  
Everything else is sedimentary and must be shoveled off the SpecFence hot path — especially heat/account sticky Wait, HotSet/Engagement ladders, Bayes-as-second-π, and default inspect mythology.
