# SpecFence REM Plant v2 — deep revm + dual levers

**Status:** FROZEN 2026-09-04 — implement M0→M1 next (supersedes Spec v1 Phase-2 PartialRetry as *semantic-only*)  
**Authority:** user 2026-09-04 — beat OCC requires **both** fewer interpreter entries **and** shorter critical path; **revm may be modified freely** to identify / record / learn dynamic RW sets and repair inside the interpreter.  
**Prior:** `specfence-rem-spec-v1.md`, `specfence-first-principles-bottleneck.md`  
**Repo:** `fengjy73/pevm` branch `specfence` (path vendor/patch revm as needed)

---

## 0. Dual performance contract (non-negotiable)

| Lever | Meaning | Success signal |
|-------|---------|----------------|
| **L1 Fewer interpreter entries** | Count of `Interpreter::run` / fresh `Evm::transact` starts per block **strictly below** OCC on same block@same cores when contention is non-trivial; equal OK on conflict-free blocks. | `evm_entries_sf < evm_entries_occ` (new metric) |
| **L2 Shorter critical path** | When a region waits, cores run other ready region/tx work; wall-clock ≈ true DAG \(T_{\mathrm{crit}}\) + small overhead, not wait-serialized pipeline. | `useful_time / (useful+wait+reexec)` ↑; `wave_width_mean` ↑ vs v1 |

Policy / Bayes alone cannot satisfy L1. Semantic PartialRetry (reexec from tx head + Bind prefix) **does not count as L1**.

Correctness TCB unchanged from Spec v1 §2 (sequential equivalence).

---

## 1. Plant shift: from “tx job + CC sidecar” to “effect-stream machine”

### v1 (what we built)
```
Scheduler steals Tx → new revm session → full bytecode → journal dump → validate → maybe Full/PartialRetry (new session)
```

### v2 (required)
```
Scheduler steals RegionContinuation | ReadyTx →
  resume OR start interpreter at piece boundary →
  on each world-state effect: record RW, resolve (Bind/Wait/Spec), maybe checkpoint →
  on Wait: park continuation, enqueue other ready work (L2) →
  on conflict of certified-prefix: rewind journal to last good checkpoint, rebind, resume (L1) →
  never restart from tx head unless prefix is empty or control-flow invalidated
```

Concurrency object remains region access \(a=(t,k,\ell,m)\). The **executable unit** becomes a **continuation** \((t, \textit{pc}/\textit{checkpoint\_id}, \textit{journal\_snap}, \textit{gas\_left}, \ldots)\), not only \(t\).

---

## 2. Deep revm surface (allowed & expected)

Fork/patch revm (or pevm’s EVM adapter) to expose:

### 2.1 Effect hooks (already partially present)
At every journal mutation that maps to `MemoryLocation`:
- `on_read(ℓ) → Resolution`
- `on_write(ℓ, value)`
- Emit `(t,k,ℓ,m)` into REM / Bayes / Ĝ

### 2.2 Checkpoints (new — L1 core)
After each **committed effect boundary** (or every N effects / CALL boundary — start with **every storage/account write + every external CALL entry/exit**):
- Snapshot: interpreter PC / call-stack frame, gas remaining, journal length, SpecFence read-origins map for this incarnation.
- ID: `cp_id = (t, inc, k)`.

### 2.3 Pause / resume (new — L1 + L2)
- **Pause:** resolution = WaitHard → freeze continuation; do **not** drop interpreter stack if possible; store continuation in REM wait-table keyed by `(ℓ, writer_t)`.
- **Resume:** writer publishes → wake continuation → **continue from checkpoint**, not `transact()` from scratch.
- If host cannot keep live Interpreter across park (thread steal), serialize minimal resume state and **replay journal prefix from checkpoint 0…k\* without re-interpreting bytecode** where values are already Bind-certified (fast-forward), then resume bytecode from `k\*`.

Preferred order of implementation:
1. **Journal fast-forward + bytecode resume from CALL/effect boundary** (practical mid-tx).
2. True live Interpreter park (harder with rayon steal) as stretch.

### 2.4 Dynamic RW set learning (new — feeds both levers)
Maintain per-tx (and optionally per-code-hash / per-entry-PC):

| Structure | Updated when | Used for |
|-----------|---------------|----------|
| `WŜ(t)`, `RŜ(t)` | each incarnation / each effect | Bind before first touch of ℓ; soft edges in Ĝ |
| `WŜ(code, entry)` | across blocks (process prior) | cold-start Bind / Wait before first exec |
| `next_write_pred(ℓ \| context)` | Bayes | WaitHard only when writer likely soon **and** L2 has other work OR wait EV wins globally |
| `control_flow_stable(t,k)` | same PC/path across incarnations | decide if rewind-to-k is valid |

Learning ∉ TCB: wrong prior → more SpecRead / PartialResume, still validate.

### 2.5 Repair ops inside plant (L1)

| Op | When | Interpreter cost |
|----|------|------------------|
| `Rebind(ℓ)` | origin wrong, value path still valid at same PC | no bytecode reexec; patch read-from + continue |
| `RewindTo(cp_id)` | certified prefix `[0..k*)` OK, suffix poisoned | restore journal+PC to cp; resume (not from tx head) |
| `InvalidateSelective(ℓ)` | writer aborted | only readers of ℓ; each may Rebind or RewindTo |
| `FullRestart(t)` | control-flow diverge / empty prefix | last resort = today’s FullRetry |

**Metric rename:** `tx_partial_resume` (rewind+continue), `tx_rebind_only`, `tx_full_restart`. L1 success ⟺ `evm_entries` tracks restarts+fresh starts, **not** resume-from-checkpoint.

---

## 3. Wave ready-queue (L2 core)

Ĝ nodes = region continuations / txs with outstanding work.

Ready set \(R\):
- No unresolved **hard** predecessor in Ĝ; or
- Waiting on ℓ but **other** `(t',k')` in \(R\) exist → worker **must steal those**, never spin on the waiter.

Scheduler contract (replace “sticky Wait while holding a worker”):
1. On WaitHard: `park(continuation)`; `worker.steal()` from global ready deque.
2. On PublishWrite: `wake_all(ℓ)` → push continuations to ready deque (priority: lower `t` or higher estimated remaining gas — pick one, document).
3. Soft edges (Bayes) only reorder within ready set; revocable.

Without (1)–(2), WaitHard lengthens \(T_{\mathrm{crit}}\) even if L1 exists.

---

## 4. Resolution algebra (v2 priority)

Same verbs as v1, **reordered for dual levers**:

1. **Bind** if version known (prior WS / published writer) — prefer before first interpret of that read.
2. **Rebind / RewindTo** on fail — not SpecRead+FullRestart.
3. **WaitHard** only if: writer not done **and** (global EV: wait + steal other work < speculate+likely rewind) **and** ready deque non-empty OR posterior very high.
4. **SpecRead** (OrderedDirtyRead) otherwise.
5. **FullRestart** last.

Cost model must include **system** term: `steal_ready_depth`, not only local `1+3P`.

---

## 5. Implementation phases (coding milestones)

### M0 — Metrics plant ✅ (landed)
- Instrument `evm_entries` (fresh `transact` / interpreter start) for OCC and SpecFence at `Vm::execute` before handler `run`.
- Counters: `resume_count`, `rebind_only`, `rewind_to_cp` (0 until M1); `full_restart` (OCC abort / SF FullRetry); `tx_head_reexec` (today’s semantic PartialRetry / EarlyVal head restart).
- Exposed in `SpecFenceMetrics` + `specfence_mainnet_sweep` JSON/CSV.
- Baseline expectation: today’s SF `evm_entries ≈ n_tx + head-reexecs` (PartialRetry ≡ head reexec), **not** better than OCC until M1 RewindTo.
- M2 landed: `wait_park_ns`, `ready_steal_on_wait`, `wait_park_count`, `wave_width_mean`.

### M1 — Checkpoints + RewindTo (L1 minimum) ✅ (partial)
- Patch revm/pevm adapter: checkpoint at CALL + storage write boundaries.
- On selective invalidate: RewindTo last certified cp; resume.
- Success gate: `evm_entries_sf < evm_entries_occ` on ≥1 hot conflict block@8 **or** clear gap only on FullRestart count; SF/OCC TPS moves toward 1.

### M1b — Journal fast-forward / boundary resume ✅ (partial)
- On checkpoint/incarnation: capture SpecFence journal + bound values (`ResumeContinuation`).
- On RewindTo: restore journal to `cp`; FF certified-prefix DB reads via value cache (skip MV lazy walks).
- True PC / nested CALL resume still TODO — see `lab/notes/specfence-plant-v2-m1b-status.md`.

### M1c — CALL/effect-boundary PC resume ✅ (partial)
- BoundarySnapshot on EffectBoundary; ResumeContinuation.boundary; metrics `pc_resume_count` / `prefix_opcodes_skipped`.
- SpecFenceInspector PC/stack restore unit-tested; production keeps Handler::run (seq≡par landmine).
- See `lab/notes/specfence-plant-v2-m1c-status.md`.

### M2 — Park/Wake + ready-queue (L2) ✅ (tx-grain landed)
- WaitHard parks continuation (**tx-level** Blocking + `WaveParkTable`); worker steals lower-TxIdx-first ready deque.
- Metrics: `wait_park_count`, `wait_park_ns`, `ready_steal_on_wait`, `wave_width_mean`.
- Success gate: `wait_time` down, `wave_width_mean` up, TPS↑ at 8 cores vs M1 (measure in next sweep).
- See `lab/notes/specfence-plant-v2-m2-status.md`.

### M3 — Online RW learning → Bind-before-touch
- Use completed incarnation / prior blocks to Bind predicted reads before interpret.
- Success gate: first-pass `region_validate_fail` ↓; `evm_entries` ↓ further.

### M4 — Adaptive engagement
- Low contention: OCC-fast path (SpecFence meta off) so conflict-free blocks match OCC.
- High contention: full REM plant.

**Do not** ship more τ-only Bayes without M1.

---

## 6. What this invalidates from v1 coding assumptions

| v1 assumption | v2 |
|---------------|-----|
| PartialRetry = next incarnation from tx head + Bind prefix | Insufficient for L1; demote to FullRestart sibling |
| revm treated as black box | **Fork/patch required** |
| WaitHard can block the worker | **Forbidden**; must park+steal |
| Beating OCC by fewer WaitHard counts | Insufficient; need `evm_entries` + wave width |
| Phase-2 PartialRetry “later” | **Now on critical path** as RewindTo/resume |

Spec v1 correctness + vocabulary still hold; performance contract and plant §§4–5 of v1 are **replaced** by this doc once frozen.

---

## 7. Research framing (VLDB)

Positive claim becomes:

> An effect-continuation REM over a patched EVM, with online RW-set learning, checkpoint rewind, and wave scheduling, approaches the preset-order DAG ceiling and **outperforms Block-STM-style OCC on interpreter entries and wall-clock TPS** under mainnet contention.

Negative interim claim (already earned) remains publishable as motivation: location Bayes + semantic partial retry on an OCC plant does not move TPS.

---

## 8. Frozen choices (2026-09-04)

1. Checkpoint grain: **CALL entry/exit + storage/account write** for M1.  
2. Park model: **serialize + journal fast-forward** (rayon-compatible); live Interpreter affinity is stretch.  
3. Ready-queue priority: **lower TxIdx first**.  
4. Prior learning: **process-local prior + deterministic tests** (Spec v1).

---

## 9. Bottom line

User mandate = **L1 ∧ L2** with **deep revm**. The implementation center of gravity moves from `specfence::{bayes,resolve}` into **revm journal checkpoints + continuation scheduler**. Bayes becomes a binder/waiter policy on top of a plant that can actually skip bytecode and keep cores busy.
