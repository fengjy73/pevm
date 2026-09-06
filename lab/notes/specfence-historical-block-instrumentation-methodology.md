# How to deeply instrument historical EVM blocks — CC & parallel-computing first principles

**Date:** 2026-09-07 (Asia/Shanghai)  
**Status:** METHODOLOGY (authoritative for next instrumentation)  
**Audience:** SpecFence research — redesign measurement before more hooks or `choose_action`  
**Stance:** Prior user tables and our ad-hoc RAW/HotSet/journal passes are **hypotheses**, not the measurement specification. This note defines what *must* be measured from first principles.

---

## 0. Why previous instrumentation was incomplete

### 0.1 What we (and earlier analyses) optimized for
- **Count matching** (final-RW edges → Db hooks → journal SLOAD instances → chase external ~N RAW).
- **Single scalar depth** (gas/limit, then gross-work) as if one \(d\) decides Wait vs abort.
- **Location HotSet / writer counts** as if membership ≈ scheduling policy.
- **Post-hoc OCC abort aggregates** without a closed **cost model** tying discovery time → redo → wait → critical path.

### 0.2 What CC + parallel computing actually need
A parallel execution engine under a **fixed commit order** is not free-form parallel computing: it is **constraint satisfaction + work scheduling** under:
1. a known total order on commits (consensus),
2. unknown dynamic RW sets (EVM),
3. a **cost model** where the expensive resource is **interpreter seconds** (and secondarily core idle),
4. a **correctness TCB** (sequential ≡ parallel on state/receipts/gas).

Instrumentation must answer **decision variables of the scheduler/CC**, not produce prettier graphs.

---

## 1. Problem formalization (first principles)

### 1.1 Parallel computing view
After consensus, block \(B=(T_0,\ldots,T_{n-1})\) is a batch of tasks with:
- **Precedence:** commit order fixed; if \(T_j\) reads a version written by \(T_i\) (\(i<j\)), then in the true dependency DAG \(G^\star\), \(T_i \prec T_j\) on that location.
- **Objective:** minimize makespan (or maximize TPS) ≈ minimize
  \[
  T_{\mathrm{crit}}(G^\star) + T_{\mathrm{waste}},\quad
  T_{\mathrm{waste}} = T_{\mathrm{redo}} + T_{\mathrm{idle}} + T_{\mathrm{meta}}.
  \]
- **Unknown \(G^\star\):** must be **discovered** online (speculation) or **predicted** (learning). Historical blocks give an oracle \(G^\star\) via sequential replay.

Classical parallels:
- **Task graph scheduling** with discovery (like speculative parallelization / TLS).
- **Work stealing** under producer–consumer constraints (M2 park/steal).
- **Critical-path vs wasted work** tradeoff (always-speculate OCC vs wait-for-producer Bohm).

### 1.2 Concurrency-control view
- **Concurrency object:** versions on **locations** \(\ell\) (account basic / storage slot / code) — not whole txs, not account-grain alone.
- **Histories:** reads-from, ww-order must match sequential history for committed effects.
- **Mechanisms:** optimistic validate+abort (OCC/Block-STM), wait-for-writer (Bohm/2PL-ish), hybrid.
- **Learning ∉ TCB:** wrong Wait/Bind only costs performance if validate still enforces correctness.

Iron law for TPS: policy wins iff it reduces **interpreter-seconds on the critical path** or **idle**, by more than meta cost.

### 1.3 EVM plant constraints (why DB ≠ interpreter)
- Billing unit = **gas / opcode work**, not MV map lookups.
- **Journal caching:** repeated SLOAD of same slot may not re-enter storage DB — instance RAW ≠ “every logical read” unless defined at opcode layer with explicit warm/cold.
- **Lazy beneficiary / LazySender/Recipient:** false WW cliques — must be **excluded from speculative \(G^\star\)** or labeled non-critical.
- **Mid-tx state:** PC/stack/memory/gas — repair ≠ free; instrumentation of “early abort EV” needs **work already sunk** at discovery, in **gross-work** (used/total_used), not gas/limit.

---

## 2. Questions instrumentation must answer (the real checklist)

Design measurements so each question has a **primary metric** and a **decision it informs**.

| # | Question (CC / parallel) | Informs | Primary measurements |
|---|--------------------------|---------|----------------------|
| Q1 | What is true \(G^\star\) under sequential semantics? | Ceiling / waves | Per-tx final RS/WS; per-location write timeline; effective DAG excluding lazy/beneficiary |
| Q2 | Where is **critical path** vs **parallel slack**? | Whether Wait helps | Longest chain; max wave width; independent fraction; chain composition (RAW vs WAW) |
| Q3 | How much work is **wasted** under OCC-style discovery? | Redo budget | Per-tx incarnation count; `evm_entries`; work per incarnation; abort cause (validate fail loc) |
| Q4 | **When** is a RAW dependency discoverable relative to consumer work? | Wait vs SpecRead vs EarlyAbort | At each cross-tx read: \(d=\mathrm{gross\_work}\), opcode depth, producer status |
| Q5 | What is **producer readiness** at discovery? | Bind vs Wait vs SpecRead | Data / ESTIMATE / running / not-started; time-to-publish proxy |
| Q6 | What is **remaining consumer work** after discovery? | EarlyAbort EV | \(1-d\); remaining gas; remaining effects; (research) PC/region suffix |
| Q7 | Fan-out vs deep pipeline morphology? | Policy class | Out-degree of writers; program-RAW path length; WAW-only spines |
| Q8 | Program vs handler / schedule noise? | Different priors | Edge class by location kind; lag distributions; Cancun-like mixes |
| Q9 | Cascade / ESTIMATE fanout? | Contagion cost | Readers poisoned per abort; fence skips |
| Q10 | Cross-block **morphology persistence**? | Inter-block prior | Adjacent-block deltas of {program_frac, fanout, gw_depth, abort, chain} |
| Q11 | Meta / idle tax of a candidate policy? | Don’t reintroduce v7 | Inspect steps, Wait park time, steal success, lean vs full |
| Q12 | Counterfactual EV of policies offline? | Before coding | Replay traces under M-A / M-D / Wait-if / Bind-if simulators |

**If a hook does not serve ≥1 of Q1–Q12, do not add it.**

---

## 3. Measurement layers (layered instrumentation architecture)

Instrument in layers. Each layer has a clear oracle role. Never collapse layers into one “RAW count.”

### L0 — Block / plant identity
- Block number, n_tx, gas_used, fork (Cancun etc.), cores, mode (serial/OCC@k/SpecFence).
- Bytecode/cache warm vs cold (first serial run flagged).

### L1 — Sequential oracle \(G^\star\) (enforcement truth)
**Purpose:** ground-truth dependencies and work.

For sequential replay only:
1. **Effect log (append-only):** every world-state **mutating** effect and every **read observe** with:
   - `(tx, effect_k, op_class, location, mode∈{R,W}, gas_used_so_far, opcode_steps_so_far, call_depth)`
   - Warm/cold bit if journal hit vs DB miss (definitional clarity).
2. **Version timeline per \(\ell\):** list of writers `(tx, k, value_hash)` in order.
3. **Reads-from map:** each read → `(producer_tx, producer_k)` or Storage.
4. **Tx work totals:** `gas_used`, opcode_steps, effect counts.

**Outputs:** RAW/WAW/WAR edge **instances** (no silent dedupe; report both instance and unique-(p,c,ℓ) summaries), effective DAG stats, wave schedule of \(G^\star\).

### L2 — Speculative discovery under OCC (online truth)
**Purpose:** how Block-STM **learns** \(G^\star\) the hard way.

On OCC@1 (ordered) and OCC@≥2 (true parallel):
1. Per incarnation: start/end, abort?, fail locations, ESTIMATE set, readers poisoned.
2. On each cross-tx observe during incarnation: same fields as L1 **plus** `producer_status∈{Data,Estimate,Running,Absent}`, `consumer_incarnation`, `ready_for_bind?`.
3. Scheduler: wait/park/steal counts, idle time if available.

**Key:** sample producer status **at the discovering incarnation**, not only on the final successful incarnation (final first-cross overstates readiness).

### L3 — Cost / EV offline laboratory (not production hooks)
**Purpose:** answer “what should policy have done?” without implementing SpecFence.

From L1+L2 traces, run **counterfactual simulators**:
- **M-A (always speculate):** on conflict, redo remaining work \((1-d)\hat w\) (and optional full reexec).
- **M-D (discover→decide):** at discovery, choose Wait / Bind / EarlyAbort / SpecRead using measured \(d\), producer_status, morphology features; accumulate redo + wait.
- **Critical-path replay:** assign each tx work \(\hat w\) on a machine with \(P\) cores under true \(G^\star\) (lower bound) vs under OCC abort DAG.

Report: redo_saved, wait_added, estimated makespan — **per morphology class**, not one block scalar.

### L4 — Cross-block morphology (learning prior)
For contiguous windows (e.g. A/B/C segments):
- Feature vector per block: `{n_tx, program_frac, handler_frac, fanout_max, raw_path, waw_path, gw_depth_{p10,p50,p90}, abort@8, reexec_frac, indep_frac}`.
- Transition matrix / delta norms between adjacent blocks.
- **Prior usefulness test:** does block \(t-1\) features predict block \(t\) morphology better than a global prior? (quantify; if no, don’t ship sticky priors.)

### L5 — Policy overhead (only when evaluating SpecFence)
Inspect steps, meta maps, WaitHard, lean_mode — compare to OCC baseline on **same** block. This layer evaluates implementations; it is not how you *discover* \(G^\star\).

---

## 4. Definitions that must be fixed before collecting

Publish these in every result JSON `method` field:

| Term | Definition |
|------|------------|
| **Location** | pevm `MemoryLocation` (Basic / Storage / CodeHash / …) |
| **Program edge** | RAW whose location is storage/code/selfdestruct-related |
| **Handler edge** | RAW on account basic / balance / tx-env style |
| **Excluded from effective \(G^\star\)** | coinbase beneficiary; `basic_lazy` LazySender/Recipient |
| **RAW instance** | one consumer **read observe** that reads-from a prior tx’s write (warm journal re-read of same slot: either always emit with `warm=true`, or never — **choose one and stick**) |
| **Gross-work depth \(d\)** | `gas_used_so_far / tx_gas_used` at discovery (primary); opcode fraction secondary; **forbid gas/limit as primary** |
| **Producer ready** | MV holds non-ESTIMATE Data for that writer version at observe time |
| **Account-grain diag** | separate counter; **never** primary Wait key |

Ambiguity killed comparability between “3800 RAW” analyses and pevm hooks. Fix definitions first; then counts become comparable **within** the plant.

---

## 5. Recommended analysis workflow on historical blocks

### Phase A — Sequential L1 oracle (mandatory)
1. Fetch snapshot (Alchemy).  
2. Serial replay with L1 effect log (opt-in).  
3. Emit: DAG summary, RAW instance stats, morphology label (fan-out / mixed / WAW-spine / quiet), depth histograms.  
4. **Do not** yet argue Wait vs Abort — only characterize \(G^\star\) and work.

### Phase B — OCC L2 discovery (mandatory)
1. OCC@1: validate L1 vs online observes (sanity).  
2. OCC@8: discovery timing + producer_status + incarnation.  
3. Abort–depth correlation tables.

### Phase C — Offline EV lab L3 (mandatory before coding policy)
1. Run M-A / M-D / Wait-if-program-fanout / Bind-if-ready simulators on traces.  
2. Score by estimated \(T_{\mathrm{redo}}+T_{\mathrm{wait}}\) and crude makespan.  
3. **Only policies that win offline on ≥1 hot morphology and don’t lose on quiet neighbors** become implementation candidates.

### Phase D — Contiguous L4 prior test
1. On segments A/B/C: prior predictivity test.  
2. If predictivity weak → inter-block learning is low priority vs intra-block EV.

### Phase E — SpecFence L5 (last)
Implement `choose_action` only after C says EV win; measure meta tax.

---

## 6. Critique of our current stack vs this methodology

| Current | Gap vs methodology |
|---------|---------------------|
| Final-RW FineGrain | L1 incomplete (no effect timeline, no warm bit policy) |
| Db-deep / journal RAW | Partially L1/L2 observes; definitions of warm/instance still fuzzy |
| Gross-work depth | Correct **primary** \(d\) — keep |
| Account-grain diag | Correctly demoted — keep as diag only |
| Producer readiness | Started (deeper pass) — must be **first-class L2 field** everywhere |
| M-A/M-D scalars | Too coarse; need **per-morphology** L3 simulators with explicit assumptions |
| HotSet / H_w | Skips Q4–Q6; fails methodology §2 |
| Chasing external RAW N | Violates “definitions first”; abandoned |

---

## 7. What “comprehensive” means for SpecFence next

**Stop:** adding hooks without a Q1–Q12 mapping; implementing `choose_action` from intuition; treating one block’s fan-out as universal.

**Do next (in order):**
1. **Freeze measurement spec** (this doc §4) in code `method` schema.  
2. **Complete L1 sequential effect timeline** with explicit warm/cold policy.  
3. **Unify L2 OCC@1/@8 traces** with producer_status + incarnation + \(d\).  
4. **Build L3 offline policy lab** on contiguous cores (597 / 599 / 097 + neighbors).  
5. **L4 prior predictivity** before shipping cross-block learning.  
6. Only then implement production `choose_action` and L5 sweeps.

---

## 8. Bottom line

Fine-grained deep instrumentation of historical blocks is not “log more SLOAD.” It is building an **oracle task graph \(G^\star\)**, a **discovery process trace**, and an **offline EV laboratory** so Wait/Bind/SpecRead/EarlyAbort are justified by **critical-path and waste accounting** under EVM cost and fixed commit order.

That is the CC + parallel-computing professional standard. Counts and depths are outputs of that lab — not goals in themselves.
