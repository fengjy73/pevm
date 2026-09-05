# First principles again — after plant v2 M0–M1f + M2

**Date:** 2026-09-05  
**Evidence:** mainnet SF/OCC ~0.25–0.30 (pre-plant-v2 sweeps); plant v2 commits through M1f `8d506e2` + M2 `d6ecdcf`.  
**Prior note:** `specfence-first-principles-bottleneck.md` (still valid as iron law).

---

## 1. Blockchain execution has not changed

Consensus still hands a **fixed order**. Execution still must match sequential EVM bit-for-bit (receipts, gas, logs, state root). Parallelism is only evaluating the true dependency DAG \(G^\star\).

Cost model still:

\[
\mathrm{TPS} \propto \frac{n}{T_{\mathrm{crit}}(G^\star) + T_{\mathrm{waste}}}
\]

with \(T_{\mathrm{waste}}\) = aborted/redundant **interpreter seconds** + **idle** + **CC meta**. Gas/opcodes dominate; Bayes maps do not.

What *did* change is our plant’s ability to attack \(T_{\mathrm{waste}}\) structurally (L1 resume, L2 park/steal) — and what that revealed about **EVM as a repair substrate**.

---

## 2. Two layers of the problem (must not confuse)

| Layer | Question | SpecFence intent |
|-------|----------|------------------|
| **CC / scheduling** | Which version to read, when to wait, whom to invalidate, which worker runs next? | Region REM, Bayes, Bind/Wait/SpecRead, wave ready-queue |
| **EVM plant** | After a wrong read, can we avoid re-paying prefix gas? | Checkpoint, journal FF, absolute PC jump |

Block-STM OCC almost only invests in the **CC layer** with a **dumb, cheap plant** (whole-tx reexec). SpecFence invested heavily in both. The post-M1f lesson:

> On Ethereum’s real plant (revm + pevm MvMemory), **fine CC without a general cheap plant cannot beat OCC**; and a **general cheap plant is itself a research-hard EVM systems problem**, not a scheduling tweak.

---

## 3. What plant v2 actually bought

### L2 (M2) — mostly solved at tx grain

WaitHard no longer spins inside `Vm::execute`. Park → steal lowest TxIdx → wake on writer finish. That matches the iron-law term “don’t idle a core on a wait.”

**Ceiling:** tasks are still **whole transactions**. Mid-effect wait still aborts the interpreter frame and resumes as a new incarnation (possibly with M1* help). True region-continuation scheduling (nodes = `(t,k)`) is not there. L2 removed **busy-wait**, not **tx-level pipeline bubbles** from mid-tx dependencies.

### L1 (M0–M1f) — existence proof, not mainnet lever

| Milestone | Real saving | Still paid |
|-----------|-------------|------------|
| M0 | Measurement (`evm_entries`) | — |
| M1 | Metric/class RewindTo vs head reexec | Full bytecode from head |
| M1b | SpecFence journal replay + certified-prefix MV/DB skip | Prefix opcodes + revm SSTORE journal |
| M1c–M1d | Inspect path + step accounting | Default no absolute jump |
| M1e–M1f | **Default absolute jump** when `jump_is_safe` | Only tiny Basic-only, depth≤1, ≤256B code, no Storage FF |

So L1 is **real** on a toy set (BalanceProbe-class), and **credit-only** on ERC-20 / Storage / nested CALL — i.e. on the mass of mainnet gas.

Iron law update:

> Interpreter-seconds fall only where absolute jump (or equivalent) fires. On mainnet-like txs, SpecFence still mostly pays OCC-like reexec **plus** inspect/Bayes/readers overhead.

That is why we should **not** expect SF/OCC TPS ≫ 1 from M1f alone without a new mainnet sweep — and why a sweep might still look worse than OCC if meta tax dominates.

---

## 4. Why EVM repair is harder than DB page repair (first principles)

Classical DB recovery / mid-tx abort can truncate a **page-level undo log** whose semantics are the storage model. EVM “undo” must reconstruct:

1. **Control state:** PC, stack, memory, MemoryGas, refund, call frames.  
2. **Journal state:** account/storage presents that must not shadow MvMemory.  
3. **Observable gas:** every skipped opcode’s gas must match sequential.  
4. **Logs / creates / selfdestruct / transient storage.**  
5. **Host coupling:** pevm’s multi-version read origins after a jump must ≡ sequential certified prefix.

M1f root causes map exactly onto these:

- Missing MemoryGas → wrong expansion gas (seq≠par).  
- Journal blob restore → SLOAD hits stale journal not MV (wrong CF / hang).  
- Storage-prefix jump → mid-tx frame ≢ sequential under MV.  
- CallEntry `k=0` without EffectBoundary → no snap to jump from.

**Conclusion:** SpecFence’s “region finer than tx” is correct as a **CC object**, but the **repair cost unit** remains “EVM continuation,” and EVM continuations are not free to materialize under MV+inspect. Narrowing `jump_is_safe` was not cowardice — it is the feasible set where (1)–(5) currently close.

---

## 5. OCC’s advantage, restated after our work

OCC’s waste is **simple and parallelizable**: wrong txs re-run; ESTIMATE notifies; scheduler stays lean. No need for continuation equality.

SpecFence’s waste is **heterogeneous**:

- Meta on every hot read (Bayes, cost π, readers, inspect steps).  
- Park/wake bookkeeping (necessary for L2).  
- Fallback resume that still reinterprets prefix (most mainnet repairs).  
- Occasional jump only on tiny Basic-only.

So even when SpecFence’s **decisions** are finer, its **average cost per decision** is higher. Beating OCC requires either:

**A.** Jump set covering a **large fraction of aborted gas** (ERC-20-class), or  
**B.** Avoiding the first wrong run (prior Bind / accurate WŜ before interpret), or  
**C.** Meta so cheap that residual waste < OCC aborts (adaptive engagement / M4).

M1f is a foothold for A on a tiny set. M3 targets B. M4 targets C. M2 helps \(T_{\mathrm{crit}}\) idle but does not substitute for A/B.

---

## 6. Dependency structure of mainnet blocks (why A is the bottleneck)

Typical mainnet contention is not BalanceProbe. It is:

- ERC-20 `transfer` / DEX routes → **Storage** slots + nested CALLs.  
- Hot accounts with many writers → Basic + Storage.  
- Depth > 1 for almost every interesting contract call.

The default-safe jump set (no Storage FF, depth≤1, tiny bytecode) is **almost orthogonal** to where abort gas lives. Hence:

\[
\mathbb{E}[\text{gas saved by jump}] \ll \mathbb{E}[\text{gas in aborted ERC-20-like txs}]
\]

until Storage-safe jump or CallOutcome-cache nested resume exists.

---

## 7. Research claim recalibration

| Claim | Status after M1f/M2 |
|-------|---------------------|
| Region-level CC + learning is implementable on pevm | **Shown** |
| WaitHard need not pin a worker (tx-grain park/steal) | **Shown** |
| Mid-tx absolute resume can be seq≡par on a restricted class | **Shown** (existence) |
| SpecFence TPS ≥ OCC on 7 mainnet blocks | **Not shown**; still should not claim |
| Approaches DAG ceiling on real EVM | **Open**; blocked on Storage/nested L1 + prior Bind + meta tax |

Honest VLDB arc: **systems paper on adaptive fine-grain CC + EVM continuation repair**, with negative result that naive plant hooks lose to Block-STM, and positive restricted L1/L2 mechanisms — *or* finish A/B enough that TPS flips.

---

## 8. What first principles say to do next (ordered by iron law)

1. **Expand safe jump to Storage-prefix / ERC-20 without Db poison**  
   - Likely: never materialize storage presents into revm journal; keep FF only via SpecFence/MV bind; restore MemoryGas always; maybe CALL-boundary (not mid-SSTORE) jumps first.  
2. **Nested CALL via cached CallOutcome** (depth>1) — mandatory for real contracts.  
3. **M3 prior Bind** — cut *first* incarnation waste (often larger than repair).  
4. **M4 adaptive engagement** — SpecFence meta off when conflict prior low so conflict-free blocks match OCC.  
5. **Re-measure** 7-block sweep with `evm_entries`, `absolute_jump_applied`, `journal_ff_hits`, SF/OCC — without this, further M1* is flying blind.

Not leverage: more Bayes τ, more credit metrics without jump coverage, or claiming L1 from `prefix_opcodes_skipped` alone.

---

## 9. Bottom line

Blockchain TPS under fixed order is still critical-path + waste on an **EVM-cost plant**. We proved:

- L2 park/steal works at **tx** grain.  
- L1 absolute jump works on a **tiny Basic-only** set and teaches why general EVM mid-tx repair fights MvMemory, gas, and journals.

Until the jump (or prior-Bind) set covers **where mainnet abort gas actually is**, SpecFence remains a richer CC story on a plant that still mostly re-pays OCC’s bill — plus interest. The next first-principles move is not another scheduler verb; it is **closing the EVM continuation equality gap for Storage and CALL**, or **avoiding the wrong first run**.
