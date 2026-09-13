# Why SpecFence still loses to OCC — first principles

Evidence base: mainnet sweeps v3–v6 on seven Ethereum blocks; P1a REM; P2 semantic PartialRetry (`tx_full_retry=0`); cost-aware π (WaitHard −34%, SF/OCC still ~0.25–0.30).

---

## 1. What blockchain execution actually is

After consensus, a block is already a **fixed total order** \(T_0,\ldots,T_{n-1}\). The execution layer’s only job:

1. Apply each tx’s EVM effects to world state.
2. Produce receipts / logs / gas accounting.
3. Agree bit-for-bit with sequential apply (then state root).

There is **no freedom to reorder commits**. The only parallelism is evaluating updates whose **true state dependencies** form a DAG consistent with that order.

Empirically on our blocks (~400–1100 txs, ~30M gas):

- Wall time is dominated by **EVM interpreter work** (gas), not by hash-map lookups in the CC layer.
- A “retry” that re-enters the interpreter on a tx again pays nearly the same as the first run.
- Cores that are **waiting** or doing **metadata** do not shorten the DAG critical path.

So any protocol’s TPS is roughly:

\[
\mathrm{TPS} \propto \frac{n}{\underbrace{T_{\mathrm{crit}}(G^\star)}_{\text{true DAG critical path}} + \underbrace{T_{\mathrm{waste}}}_{\text{aborted EVM + idle wait + CC overhead}}}
\]

OCC (Block-STM) and SpecFence both chase \(G^\star\). They differ in how they spend \(T_{\mathrm{waste}}\).

---

## 2. CC first principles under this plant

### 2.1 The concurrency object

Conflicts live on **locations** (account/storage), not on “transactions” as opaque jobs. Ideal control grain = region access. That part of SpecFence’s thesis is correct.

### 2.2 Discovery vs enforcement

- **Enforcement** (given \(G^\star\)): Bohm-style version install + wait-for-writer; ww need not abort.
- **Discovery** (unknown RW sets): must speculate or predict. EVM forces discovery.

Block-STM’s bet: **discover by running**, fail fast with ESTIMATE, keep workers busy. SpecFence’s bet: **predict + avoid + finer repair**. Prediction only wins if avoidance/repair is **cheaper than OCC’s waste**.

### 2.3 Iron law

> Learning / fine policy cannot beat OCC on TPS unless it reduces **interpreter-seconds on the critical path** or **core idle from false waits**, by more than the policy’s own overhead.

Renaming FullRetry → PartialRetry without fewer interpreter runs does not move the iron law.

---

## 3. What OCC (PEVM Block-STM) actually does well

1. **Task = whole tx**, but the collaborative scheduler is extremely lean: execute / validate / steal work.
2. **Speculate first**: cores stay fed; conflicts resolved after the fact.
3. **ESTIMATE**: aborted writes poison dependents quickly; dependents abort/reexec without a heavy Bayesian decision path.
4. **Lazy beneficiary / transfers**: removes a false all-to-all WW clique that would otherwise serialize the block.

OCC’s \(T_{\mathrm{waste}}\) is mostly **extra EVM runs on conflicted txs**. On many mainnet blocks, conflict clusters are small enough that this waste < cost of waiting globally.

Abyss / CCBench lesson: at high contention OCC abort cost explodes; at moderate contention a lean OCC can look “too good” until you measure aborts. Our instrumented OCC aborts are real—but still cheaper than SpecFence’s waits + meta + same reexecs.

---

## 4. What SpecFence currently does (mechanically)

Pipeline today:

1. Per-read cost model / Bayes → Bind | WaitHard | SpecRead.
2. Still **one revm session per incarnation** (start from tx beginning).
3. On fail: semantic PartialRetry = selective invalidate + next incarnation **force Bind on certified prefix**.
4. Interpreter still re-runs the **entire** tx bytecode; prefix Bind only hopes validation is quieter.

Measured consequences:

| Knob we turned | Metric movement | TPS movement |
|----------------|-----------------|--------------|
| Selective invalidate + fence | cascade hygiene | weak |
| Bayes location Wait | Wait↑ or sticky over-serial | SF/OCC down (v3) |
| Semantic PartialRetry | `tx_full_retry` 0; `partial_retry≈aborts` | SF/OCC flat (~0.30) |
| Cost-aware less WaitHard | WaitHard −34% | SF/OCC **worse** on smoke (~0.25) |

**Interpretation:** we successfully changed the **labels and side effects of repair**, not the **dominant cost integral** (EVM seconds + idle). Cutting WaitHard without a better way to use freed cores (or without reducing reexec) can increase abortive speculation again while still paying SpecFence’s fatter read path—ratio dips.

---

## 5. First-principles diagnosis (root causes)

### R1 — Repair grain ≠ cost grain

Spec claims region repair; plant still charges **tx-level EVM**. PartialRetry is “partial” in MV poisoning / Bind hints, not in CPU. Hence `partial_retry` tracks aborts 1:1 like old FullRetry for wall-clock purposes.

### R2 — Wait converts parallelism into a pipeline

WaitHard shortens abort count only if producers finish soon **and** waiters would have aborted with high probability. Otherwise it **lengthens \(T_{\mathrm{crit}}\)** by inserting artificial chains. Cost model used a crude `cost_wait∈{0,1}` vs `1+3P`; it does not model **system-level** throughput (other ready txs those cores could run). Local EV ≠ global TPS.

### R3 — OCC’s discovery is already cheap

ESTIMATE + incarnation is a low-constant online discovery of \(G^\star\). SpecFence adds readers index, Bayes maps, cost metrics, selective paths on the **hot read path**. Even when decisions are “correct,” constant factors tax every location access on a 30M-gas block.

### R4 — First incarnation cannot Bohm

True Bohm needs write-sets **before** useful parallel exec. We learn WS after first run—the expensive run OCC also does. Second incarnation Bind helps SpecFence only when OCC would have needed many incarnations; on blocks where OCC often succeeds in one shot on the parallel bulk, SpecFence’s extra machinery is pure overhead.

### R5 — Objective mismatch during iteration

We optimized: abort taxonomy, WaitHard counts, fence skips. The objective that shows up in TPS is: **minimize sum over cores of (EVM time + forced idle) until the whole block certifies.** Those are not the same.

---

## 6. What would have to be true to beat OCC

Any winning SpecFence must satisfy at least one:

1. **Fewer EVM invocations than OCC** for the same block  
   - True mid-tx resume / piece pipeline; or  
   - First-pass avoidance so conflicted txs never run wrong the first time (needs accurate prior WS—rare for general EVM); or  
   - Rebind/repair **without** reexec when only origin is wrong and value path can be patched (narrow).

2. **Shorter critical path than OCC**  
   - Wave ready-queue that always feeds independent region work when a waiter blocks;  
   - Not “wait on hot ℓ while cores spin or run low-value retries.”

3. **Overhead ≪ waste saved**  
   - Decision path must be as lean as Block-STM’s validate, or run off the absolute hottest path.

If (1) stays false, (2) and (3) must be large. Cost-aware π alone cannot deliver (1).

---

## 7. Implications for the research claim

The VLDB claim (“adaptive fine-grained learning CC discovers DAG and approaches ceiling”) remains coherent **as a systems thesis**, but the **current PEVM-hook implementation is still an OCC-shaped plant with a smarter admission filter**. That can be a paper midpoint (“negative result + plant requirements”), not yet the positive performance claim.

Honest framing:

- **Shown:** location-level Bayesian control, selective invalidate, semantic partial retry, cost-aware admission are implementable on PEVM with sequential equivalence.
- **Not shown:** TPS > Block-STM OCC on mainnet execution.
- **Blocking principle:** without reducing interpreter-seconds or global idle below OCC, fine-grained learning will not win.

---

## 8. Directions consistent with first principles (for when you resume)

Ordered by leverage on the iron law:

1. **True piece/region execution** for common patterns (transfer, ERC20) — cut EVM reexec.  
2. **Global scheduler**: when WaitHard blocks a tx, immediately run ready independent txs/regions (wave ready-queue)—cut idle.  
3. **Prior WS from previous blocks / static hints only to Bind before first exec** — rare but high leverage when accurate.  
4. **Lean OCC-compatible fast path**: SpecFence meta disabled until contention signal trips (adaptive *engagement*), so low-conflict blocks match OCC bit-for-bit speed.

Not leverage: further τ tweaking, more metrics, or PartialRetry counters without fewer interpreter runs.

---

## 9. Bottom line

Blockchain execution TPS under a fixed order is a **critical-path + waste** problem on an **EVM-cost plant**. OCC wastes cycles on aborts but keeps the plant simple and busy. SpecFence today spends cycles on waiting and metadata while still paying abort-level EVM reexecs; therefore it loses even when its CC story is finer on paper. The next design move must change **where CPU seconds go**, not only **which CC verb we log**.
