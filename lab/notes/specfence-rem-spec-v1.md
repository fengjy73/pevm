# SpecFence REM Spec v1 — implementable contract

**Derived from:** `specfence-cc-architecture-v4-first-principles.md`  
**Status:** specification freeze candidate (no code in this doc)  
**Target repo:** `/workspace/specfence` (`fengjy73/pevm`, branch `specfence`)  
**Non-goal:** copy `pevm-specfence-server`

This is the **implementation contract**. If code disagrees with this doc, the doc wins until explicitly revised.

---

## 0. Frozen decisions (open choices from v4)

| Choice | Decision for Spec v1 | Rationale |
|--------|----------------------|-----------|
| Effect boundary | **Every world-state journal effect** (account basic / code / storage). Logs do not create dependency edges. | Matches MV locations PEVM already tracks; maximal fine grain without new EVM semantics. |
| Cold-path sampling | **Allowed later** as opt-in (`--region-sample`); default **off**. | Spec correctness first; sampling is a perf knob. |
| Learning vs consensus | **Commit path uses only on-block observations + block-local Bayes warm-started from prior snapshot carried in process.** For multi-validator identity: prior snapshot must be identical (same previous blocks). Document that cross-node prior sync is operator concern; tests use deterministic prior. | Avoid non-deterministic commit from live online-only noise; still allow inter-block learning in one process / agreed history. |
| OrderedDirtyRead | **Allowed** for SpecFence: read latest non-ESTIMATE version with `tx_idx < me` (Executed or Validated). Record origin. | Compatible with Block-STM; Bind/Wait preferred when posterior high. |
| PartialRetry | **Spec’d but Phase-2.** Phase-1 FullRetry remains; checkpoints recorded for metrics / future. | revm re-entry is hard; must not block Bind/Selective. |
| Beneficiary / lazy | **Never Wait/Bayes/InvalidateSelective on beneficiary gas location; lazy sender/recipient commute merge unchanged.** | Preserve PEVM EVM-specific invariant. |

---

## 1. Vocabulary

- **TxIdx** `t ∈ [0,n)` — preset consensus order.  
- **Location** `ℓ` — `MemoryLocation::{Basic(addr), CodeHash(addr), Storage(addr,slot)}`; hash `h = hash(ℓ)`.  
- **Region access** `a=(t,k,ℓ,m)` — `k` = monotonic effect ordinal inside tx execution; `m∈{R,W}`.  
- **Version** `v=(t,inc)` — incarnation `inc` of tx `t`’s write of some `ℓ`.  
- **Certified** — read-from of `ℓ` for `t` matches last writer `< t` in the final write history (same as sequential).  
- **Ĝ** — speculative dependency graph (hard + soft edges).  
- **Wave** — ready set of region tasks under Ĝ (see §5).  
- **REM** — Region Execution Machine (§4).

---

## 2. Correctness contract (TCB)

For every tx `t` that contributes to the block result:

1. For every read of `ℓ` by `t` that is used in the committed journal, the recorded read-from version is the greatest `t_w < t` that publishes a final (non-retracted) write to `ℓ`, or Storage if none.  
2. Final write history per `ℓ` is increasing in `t`.  
3. Block receipts / state equal `execute_revm_sequential` on the same inputs.  
4. Learning never commits without passing (1)–(3). Soft edges only affect scheduling.

**Test oracle:** existing sequential-equivalence tests must pass for `ConcurrencyMode::SpecFence`.

---

## 3. Performance contract (objectives)

Primary: maximize TPS on the 7 mainnet blocks at 1/2/4/8 cores; target SpecFence@8 **mean ≥ OCC@8** once Phase-1 plant lands (or prove gap is only FullRetry count).  
Secondary metrics (required in sweeps):

- `region_validate_fail`, `tx_full_retry`  
- `bind_hits`, `wait_hard_count`, `spec_read_count`  
- `selective_invalidate_count`, `cascade_revalidate_count`  
- `soft_edge_revokes` (posterior dropped → cancel Wait)  
- `wave_width_mean`, `useful_time / wait_time / reexec_time` (best-effort clocks)

---

## 4. Region Execution Machine (plant)

### 4.1 Tasks

| Task | Semantics |
|------|-----------|
| `RunTx(t)` | Drive EVM for tx `t` until exit or block; emit region events at each world-state effect. (Phase-1 still one interpreter session per incarnation.) |
| `PublishWrite(ℓ,t,inc,value)` | Install `Data(inc,value)` at `(ℓ,t)` in MV store; wake waiters. |
| `ValidateLocation(ℓ,t)` | Re-read origin for `ℓ` in `t`’s read set; success → mark location certified for `t`. |
| `Repair(ℓ,t,op)` | `Rebind` / `InvalidateSelective` / etc. |
| `FinalizeTx(t)` | All reads certified and writes installed for final incarnation → place receipt. |

Phase-1 may implement `RunTx` as today’s execute, but **must emit region events** and run **ValidateLocation** per read location (can batch at end of tx as AND of per-location validates—API must still be per-location).

### 4.2 Region events (instrumentation contract)

On each world-state effect during `RunTx(t)`:

```
on_read(ℓ):
  action ← π(ctx)
  perform action; append ReadOrigin to read_set[t][ℓ]
  update Ĝ + Bayes; maybe EarlyVal(ℓ)

on_write(ℓ, value):
  PublishWrite or buffer then Publish per π write-visibility
  update residual write-set belief for t
  register live readers index when others read
```

`k` increments per effect.

### 4.3 Checkpoints (Phase-2 prep)

After each successful `ValidateLocation` or after every `K` effects, record `(t,k, snapshot_ref)` if cheap; Phase-1 may record `k` only for metrics (`checkpoint_opportunities`).

---

## 5. Speculative DAG Ĝ and waves

### 5.1 Edges

- **Hard WR:** `t_w → t_r` on `ℓ` if `t_r` read version published by `t_w`.  
- **Hard WW:** `t_i → t_j` on `ℓ` if both write `ℓ` and `i<j` (order edge; does not imply abort).  
- **Soft WR/WW:** predicted by Bayes (`P(edge)>τ_soft`); used only for `WaitHard` / defer.

### 5.2 Wave / ready rule (Phase-1 minimal)

A tx `t` may **start/continue** speculation iff for every soft/hard Wait required by π on locations it is about to touch, producers are published (non-ESTIMATE) or decided SpecRead.

Phase-1 wave approximation (acceptable under this spec):

- Maintain `ready_queue` of txs whose **known** hard waits are clear.  
- On Publish/Validate/Repair: recompute affected txs; **revoke** soft waits when posterior `< τ_revoke`.  
- Do **not** use a single sticky `RegionMode::Wait` for the whole remaining block without revoke.

Phase-2: explicit ready-set over region tasks / connected components.

### 5.3 Cascade fence (kept)

On FullRetry of `t`, revalidation rewind only to min higher tx that read any selectively-invalidated location (v1 fence). Independent txs not force-revalidated.

---

## 6. Resolution algebra (API)

```text
WaitHard(ℓ, t)      -> block until last writer < t has non-ESTIMATE Data
                       OR producer aborted and no longer writer
Bind(ℓ, t, v)       -> read exact version v=(t_w,inc); require t_w < t
SpecRead(ℓ, t)      -> OrderedDirtyRead last Data < t (skip ESTIMATE→WaitHard/Dependency)
EarlyVal(ℓ, t)      -> ValidateLocation(ℓ,t); on fail Repair path
Rebind(ℓ, t, v')    -> switch origin to newer incarnation of same t_w if still last writer
InvalidateSelective(ℓ, t_w)
                    -> mark ESTIMATE only at ℓ for t_w; notify readers of ℓ
PartialRetry(t, k)  -> Phase-2
FullRetry(t)        -> abort incarnation; selective invalidate write set; requeue RunTx
```

### 6.1 Safe selective invalidate (Phase-1 requirement)

Maintain `readers[ℓ] = sorted set of TxIdx that currently record a read origin from some write on ℓ` updated when read_set recorded / cleared.

On FullRetry(`t_w`):

1. Let `W` = write locations of aborted incarnation.  
2. For each `ℓ ∈ W`: if `readers[ℓ]` contains any `t > t_w` **OR** validation failed involving `ℓ`, `InvalidateSelective(ℓ,t_w)`; else **keep Data until** a late reader registers (lazy poison on read hitting aborted incarnation number) — **simpler safe rule for v1:** invalidate all `ℓ ∈ W` that appear in `readers[ℓ]`, and for `ℓ` with empty readers keep Data but stamp `incarnation_aborted` so a late reader sees mismatch and Wait/Repair instead of silent wrong value.  
3. Never leave a reader with a dangling Data origin from an aborted incarnation without detection.

**Serial equivalence tests are the gate** for this rule; if ambiguous, fall back to full write-set ESTIMATE for that abort only and count `selective_fallback_full`.

---

## 7. Policy π and Bayesian state

### 7.1 Context features (Phase-1)

`ctx = (ℓ_hash, addr, is_storage, writer_known, posterior_conflict, posterior_bind_success, cores_pressure_bucket)`

### 7.2 Posteriors (Beta)

Per `ℓ` (and optionally per `addr` cold-start):

- `P_conflict` — SpecRead would fail validation  
- `P_bind_useful` — residual write-set prediction hit  

Inter-block: decay `(α-1),(β-1)*=0.95` at block boundary.

### 7.3 Action choice (Phase-1)

```
if writer_known && P_conflict >= τ_w:     WaitHard (or Bind if version ready)
elif residual_ws predicts ℓ && placeholder_ready: Bind
elif P_conflict >= τ_s:                   WaitHard
else:                                     SpecRead
maybe EarlyVal with probability p_ev(P_conflict)  // or if cores_pressure high
```

Defaults: `τ_w=0.35`, `τ_s=0.50`, `τ_revoke=0.20`, `p_ev` linear in P_conflict.  
**Revoke:** if location was Wait and `P_conflict < τ_revoke`, clear sticky wait for future accesses.

### 7.4 Updates

| Observation | Update |
|-------------|--------|
| ValidateLocation fail on ℓ | conflict++ on ℓ; maybe addr |
| ValidateLocation ok after SpecRead | success++ |
| Bind hit (read matched predicted writer) | bind_useful++ |
| Bind miss | bind_useful failure++ |
| Soft wait revoked without abort | (optional) slight success++ |

---

## 8. Mapping onto PEVM files (implementation guide)

| Module | Responsibility |
|--------|----------------|
| `specfence/rem.rs` (new) | Task enums, region event hooks, checkpoint counters |
| `specfence/dag.rs` (new) | Ĝ hard/soft edges, revoke, ready hints |
| `specfence/bayes.rs` | extend beyond single P_conflict; revoke thresholds |
| `specfence/resolve.rs` (new) | WaitHard/Bind/SpecRead/EarlyVal/Rebind/InvalidateSelective |
| `mv_memory.rs` | PublishWrite; readers index; selective ESTIMATE; aborted-inc detection |
| `vm.rs` | emit on_read/on_write; call π |
| `scheduler.rs` | fence + ready wake; avoid sticky global Wait |
| `pevm.rs` | wire SpecFence mode to REM loop; metrics snapshot |
| `tests/specfence.rs` | contracts below |
| `examples/specfence_mainnet_sweep.rs` | export new metrics |

OCC/PCC modes must remain unchanged behaviorally (OCC keeps abort metrics).

---

## 9. Test contract

1. **Sequential ≡ SpecFence** on raw / mixed / hot-recipient / mainnet disk subset.  
2. **Same tx, two locations:** force conflict on ℓ1 only; ℓ2 readers independent must not FullRetry solely due to ℓ1 (metrics: `tx_full_retry` vs `region_validate_fail`).  
3. **Bind path:** second incarnation / prior residual WS causes WaitHard/Bind and zero abort on that location in a crafted mock.  
4. **Revoke:** posterior drop clears Wait; speculative progress resumes (mock).  
5. **Selective invalidate:** abort writer with writes {ℓa,ℓb}; only reader of ℓa revalidates; reader of unrelated ℓc not rewind-forced.  
6. **OCC abort metric** still >0 on contended mock.

---

## 10. Phased delivery

| Phase | Deliverable | Exit criterion |
|-------|-------------|----------------|
| **P0** | This spec merged in `lab/notes/` + SPECFENCE.md pointer | User ack |
| **P1a** | Region events + per-location validate API + readers index + selective invalidate + revokeable Bayes Wait/SpecRead/Bind(placeholder from prior incarnation WS) | tests 1–6 green |
| **P1b** | Mainnet sweep v4 figures; SF@8 vs OCC | SF/OCC mean ≥ 1.0 **or** `tx_full_retry` explained residual |
| **P2** | Wave ready-queue by components; EarlyVal default; PartialRetry prototype for raw/ERC20 patterns | wave_width vs oracle report |
| **P3** | Richer structural Bayes (co-access); cost-based π | VLDB plot pack |

**Coding starts only after P0 ack.**

---

## 11. Explicit non-goals (P1)

- Interpreter-precise mid-tx PartialRetry for arbitrary bytecode  
- Cross-validator prior gossip protocol  
- Beating OCC by τ-tuning without P1a plant  
- Replacing PEVM lazy gas/transfer logic  

---

## 12. One-sentence freeze

SpecFence Spec v1 = **online Bayesian policy over a region-effect plant that discovers Ĝ and resolves with Wait/Bind/SpecRead/EarlyVal/SelectiveInvalidate before FullRetry**, under preset-order serial equivalence—not location-labeled Block-STM.
