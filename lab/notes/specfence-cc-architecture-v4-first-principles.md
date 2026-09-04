# SpecFence v4 — first principles (blockchain execution × concurrency control)

**Status: design revision. Supersedes v3’s “PEVM action menu” framing. No code.**

The user judgment stands: v0–v3 (and even the v3 architecture note) are **not** yet a truly fine-grained, dynamic, learning-based adaptive CC. They are Block-STM with finer *labels*. This note restarts from first principles.

---

## 0. What “not fine-grained dynamic adaptive” meant about prior versions

| Claim we made | What the system actually did | Why that fails the claim |
|---------------|------------------------------|---------------------------|
| Region < tx | Location hash used for Wait bit / Beta | Lifecycle still **tx incarnation**: schedule, validate, abort, ESTIMATE, reexec are whole-tx |
| Adaptive | τ on P(conflict) → Wait vs Speculate | Adaptation is a **static mode flip** for the rest of the block, not continuous re-planning of remaining work |
| Learning | Beta updates on abort/success | Learns a **scalar conflict rate**, not a posterior over the **dependency DAG / schedule**; does not choose among rich resolutions |
| Dynamic waves | `wave_id++` on promotion | Not a wave: no ready-set, no component split, no frontier of confirmed vs uncertain edges |
| SpecFence ≠ OCC | Cascade fence + Wait | Failure path identical to OCC hammer → cannot beat OCC when prediction is imperfect |

**Diagnosis:** we refined the *policy feature*, not the *control plant*. Fine-grained learning on a coarse plant yields over-Wait or wasted predict.

---

## 1. Blockchain execution first principles

### 1.1 What the execution layer is allowed to assume

1. **Consensus has already fixed a total order** \(T_0,\ldots,T_{n-1}\). Execution must produce state identical to sequential apply in that order (deterministic, replayable, agree across validators).
2. **Commit reordering is illegal** (unlike Aria). Parallelism may only exploit the **induced partial order** of true state dependencies consistent with the total order.
3. **RW sets are not known a priori.** EVM control flow, `CALL` targets, storage keys, and revert paths are data-dependent. Any protocol that needs perfect write-sets up front (classic Bohm / Calvin lock acquisition) is an **oracle upper bound**, not a deployable baseline—unless write-sets are discovered online.
4. **The scarce resource is useful CPU on the critical path of the true dependency DAG**, not “number of worker threads started.” Threads that chase ESTIMATE cascades or whole-tx retries do not move the frontier.
5. **Agreement cost:** every validator must be able to replay the same decisions. Learning that affects **commit** must be a pure function of the block (and maybe agreed prior state). Cross-block learning may warm priors but cannot make two validators diverge on the same block given the same priors snapshot at block start—or priors must be derived only from committed history already identical.

### 1.2 The true concurrency object

Define a **region access** \(a = (t, k, \ell, m)\) where:

- \(t\): tx index in the block  
- \(k\): execution step / trace ordinal inside \(t\) (finer than tx; the dynamic PC of EVM effects)  
- \(\ell\): location (account field / storage slot / transient / etc.)  
- \(m \in \{\mathrm{R},\mathrm{W}\}\)

The **true dependency DAG** \(G^\star\) has vertices = region writes (or accesses), edges = must-precede constraints induced by the preset order and colliding locations (wr / ww). Reads inherit wait-for on the unique last writer \(< t\).

**Theorem (informal):** wall-clock lower bound for parallel execution ≥ critical-path length of \(G^\star\) under per-access service times. SpecFence’s performance objective is to **discover \(G^\star\) online** and keep workers busy on the frontier of \(G^\star\), not on false edges or aborted ghosts.

Transactions are **containers** of region accesses, not the concurrency unit. A tx is “done” iff all its region accesses are certified under \(G^\star\).

### 1.3 Information is revealed causally

At block start, \(G^\star\) is unknown. Each executed access reveals:

- concrete \(\ell\) (and thus potential edges to prior writers / later candidates),
- success vs revert of that effect,
- optionally a better estimate of remaining accesses in the same tx (speculative residual write-set).

So the system state is not “tx Ready/Executing/Validated.” It is:

\[
(\hat G_t,\; \text{frontier},\; \text{beliefs},\; \text{certified prefix})
\]

updated after every region event. That is what **dynamic** means.

---

## 2. Academic CC lens: what must be learned / adapted

### 2.1 Classical dichotomy is the wrong top-level fork

OCC vs PCC is a **pre-1980s packaging** of policies. Modern analyses (Abyss, CCBench, Polyjuice, deterministic DB) treat CC as a **policy over interleavings**:

- when to wait,  
- which version to read,  
- when to expose writes,  
- when to validate,  
- how to abort/repair,  
- what to retry.

Polyjuice showed the policy space is large and **workload-dependent**; no single static OCC/PCC wins. SpecFence must be that policy class **under preset order + unknown RW sets + MV versions**, with **online Bayesian updating**, not offline evolutionary search over stored procedures only.

### 2.2 Deterministic MVCC insight (Bohm)

Given write-sets, ww never aborts: install version slots in order; readers wait for the right slot. Aborts are a symptom of **missing write-set / wrong version binding**, not of conflict itself.

### 2.3 Piece / early visibility insight (IC3, Callas, PWV)

Concurrency unit can be **smaller than tx**. Pipeline pieces; expose writes early under tracked dependencies; avoid cascading aborts by construction of enforceable pieces.

### 2.4 Learned CC insight (Polyjuice)

State → action at access granularity; early validate; retry from checkpoint; wait parameterized by dependency progress. **Learning without a repair/checkpoint plant still collapses to OCC.**

### 2.5 Blockchain-specific corollary

Because order is fixed, the only versions a read should ever consider are:

- last certified writer \(< t\), or  
- last speculative writer \(< t\) with an explicit dependency edge (ordered dirty read).

There is no “read any concurrent write and sort it out at commit” à la nondeterministic OCC serialization. Block-STM already uses preset order—but still **validates and aborts at tx grain**.

---

## 3. SpecFence thesis (restated precisely)

**SpecFence** is an **online Bayesian controller over region-access executions** that:

1. Maintains a posterior belief over missing edges / conflict likelihood / best action **per region and per structural pattern** (contract, slot template, co-access).  
2. Continuously **rebuilds a speculative DAG** \(\hat G\) from revealed accesses and beliefs.  
3. Schedules **region work** (not only txs) on waves = antichains / ready sets of \(\hat G\).  
4. On mismatch between belief and fact, applies **region-local resolution** (wait, rebind, early validate, selective invalidate, partial retry), escalating to whole-tx reexec only when the residual uncertain set is the whole tx.  
5. Uses inter-block learning only to shape **priors and initial \(\hat G\)**, never to bypass certification.

Correctness = every certified read-from equals the sequential last-writer; learning is not in the TCB beyond providing hints.

---

## 4. Architecture v4 (control plant first, then learner)

### 4.1 Plant: Region Execution Machine (REM)

Replace “tx incarnation loop” as the only plant with:

**Region task kinds**

- `ExecAccess(t,k)` — interpret EVM until next effect boundary (or until forced region fence)  
- `PublishWrite(ℓ,t)` — install / fill version  
- `ValidateRegion(ℓ,t)` — check read-from of this location  
- `RepairRegion(ℓ,t, kind)` — rebind / selective invalidate / resume  
- `FinalizeTx(t)` — all regions of t certified → receipt

**Effect boundary (pragmatic EVM):** each journal entry that touches world state (account/storage/logless balance) is a region event. Cold path: fewer boundaries (trace sampling) for low-posterior txs; hot path: every slot.

This is the minimal plant that makes “finer than tx” **real**. Without REM, Bayes has nothing fine to control.

### 4.2 Speculative DAG + waves

- Vertices: published writes + pending write placeholders.  
- Edges: observed wr/ww; predicted edges from posterior (soft edges).  
- **Hard frontier:** edges confirmed by execution.  
- **Soft frontier:** predicted waits (Bayesian).  
- **Wave:** maximal set of region tasks whose hard dependencies are certified and soft dependencies either satisfied or decided Speculate.

Dynamic = every Validate/Repair/Publish may add edges, remove soft edges, split/merge waves, wake waiters.

### 4.3 Resolution algebra (must exist as first-class ops)

Not “ESTIMATE ± Wait”:

| Op | When | Effect on \(\hat G\) / state |
|----|------|------------------------------|
| `WaitHard(ℓ)` | posterior high or writer known | block access until writer Publish |
| `Bind(ℓ → v)` | writer known from prior incarnation / placeholder | read exact version; add hard edge |
| `SpecRead(ℓ)` | uncertain | read best ordered dirty/clean; record soft origin |
| `EarlyVal(ℓ)` | after SpecRead or on pressure | validate one location early |
| `Rebind(ℓ)` | writer republished | switch origin if still sequential-correct |
| `InvalidateSelective(ℓ)` | writer repair | poison only ℓ for dependents of ℓ |
| `PartialRetry(t from k)` | region checkpoint | resume tx from last certified effect boundary |
| `FullRetry(t)` | residual unknown large | last resort (= today’s OCC) |

ESTIMATE ⊂ `InvalidateSelective` for the all-locations special case.

### 4.4 Learner: what is Bayesian, exactly

Prior versions learned P(conflict). That is necessary but **too low-dimensional**.

Maintain posteriors over:

1. **Edge existence** \(P(T_i \to T_j \text{ on } \ell)\) or factorized \(P(\ell \text{ contended} \mid \text{ctx})\).  
2. **Residual write-set membership** after seeing prefix of tx \(t\) (for placeholders / Bohm-lite).  
3. **Action value** or success probability for `{Wait, SpecRead, Bind}` under context (hierarchical Beta / Dirichlet; context = contract, selector, slot high bits, wave depth, core pressure).  
4. **Inter-block structural model:** co-access graphs, hotspot slots, “this router storage is width-1.”

Update on every region event (not only tx abort). Intra-block: filter. Inter-block: decayed prior + structure.

**Adaptation loop (true online control):**

```
while block not certified:
  wake region tasks on frontier(Ĝ, beliefs)
  for each scheduled access:
    a ← π(belief, Ĝ, pressure)
    execute a; observe result
    update beliefs; update Ĝ; requeue / cancel soft waits
```

This is an adaptive controller, not “promote location to Wait forever.”

### 4.5 Correctness TCB

- Sequential read-from certificate per location.  
- Wave/learner may only gate scheduling.  
- Any Bind/Rebind must check order \(i<j\).  
- Validators sharing the same inter-block prior snapshot + block must match; or run learner in shadow mode for metrics while commit path uses only on-block observations (safer for first impl).

---

## 5. Why this is finer / more dynamic / more adaptive than v3

| Dimension | v3 | v4 |
|-----------|----|----|
| Grain of schedule | tx task | region access task |
| Grain of validate/repair | tx | location + partial retry |
| Dynamics | mode bit sticky | Ĝ and waves after every event |
| Learning target | P(conflict) | edges + residual WS + action success |
| Failure path | FullRetry≈OCC | Wait/Bind/EarlyVal/Rebind/Selective/Partial before FullRetry |
| Wave | counter | ready-set of Ĝ |

---

## 6. Relationship to PEVM

PEVM remains a useful **MV memory + EVM** substrate. SpecFence v4 is not “a few hooks in `try_validate`.” It requires a **Region Execution Machine** beside (or above) the collaborative tx scheduler—eventually the tx scheduler becomes a thin FinalizeTx over region certificates.

Incremental implementation must still land REM pieces **before** claiming SpecFence≻OCC:

1. Effect-boundary journal hooks → region events  
2. Per-location validate + selective invalidate with live reader index  
3. Placeholder/bind from residual write-set belief  
4. Soft-edge waits from Bayes that can be **revoked** when posterior drops (not sticky Wait)  
5. Only then: richer bandit over actions + wave ready-queue  
6. PartialRetry when interpreter support exists

---

## 7. Evaluation implications (VLDB)

Primary: TPS vs cores with abort/repair breakdown stacked (useful / wait / reexec / cascade)—Abyss style.  
Secondary: \(\mathrm{TPS}/\mathrm{OPT}\), critical-path gap, region vs tx abort counts, wave width vs oracle DAG width.  
If region aborts ≪ tx aborts but TPS still flat, plant is still tx-bound (honest negative).

---

## 8. Open choices for the user (design, not coding)

1. **Effect boundary:** every state journal entry vs sampled boundaries for cold txs.  
2. **Shadow learning vs commit-affecting learning** for multi-validator determinism.  
3. **PartialRetry priority:** invest in EVM re-entry checkpoints soon, or first exhaust Bind/Wait/Selective on full reexec.  
4. Whether **OrderedDirtyRead** of non-validated prior is allowed (Block-STM-like) or only CleanRead+Wait/Bind (Bohm-like).

---

## 9. Bottom line

A truly fine-grained dynamic learning adaptive SpecFence is an **online Bayesian scheduler over a region-access plant that discovers \(G^\star\)**, not an OCC variant with location-level priors. Until the plant’s schedule/validate/repair grain is the region access, learning cannot be “SpecFence” in the sense required for VLDB.
