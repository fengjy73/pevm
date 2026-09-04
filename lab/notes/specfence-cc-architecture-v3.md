# SpecFence architecture from database concurrency control

**Status:** design only — no implementation in this note.  
**Constraint:** preset consensus total order + dynamic RW sets (EVM) + serial equivalence to that order. Aria-style commit reordering is forbidden.

---

## 1. Problem (CC terms)

Let block transactions be \(T_0,\ldots,T_{n-1}\) with **preset serialization order**. Final state must equal sequential execution in that order.

True conflicts are on **locations** \(x\) (account basic / code hash / storage slot):

| Edge | Meaning under preset order | Ideal resolution |
|------|----------------------------|------------------|
| \(T_i \xrightarrow{ww} T_j\) (\(i<j\)) | Both write \(x\); \(T_j\)’s write must follow \(T_i\) | Install versions in order; **no abort needed** if \(T_j\) never reads a stale mid-state incorrectly (Bohm) |
| \(T_i \xrightarrow{wr} T_j\) | \(T_j\) must read \(T_i\)’s write of \(x\) | Bind read-from \(i\); wait until version ready, or rebind after \(i\) publishes |
| \(T_i \xrightarrow{rw} T_j\) | \(T_i\) read old \(x\), \(T_j\) writes \(x\) | Under preset order with \(i<j\), this is **not** a serialization violation if \(T_i\)’s read-from is still the last writer \(<i\). It only hurts if \(T_i\) read a speculative wrong version |

Block-STM / PEVM OCC collapses almost every detected inconsistency into:

1. whole-tx validation abort  
2. **ESTIMATE on entire write set**  
3. suffix re-validation / dependent re-exec  

That is one point in a much larger CC action space. It is correct, but it maximizes **wasted work** whenever the true DAG width ≫ 1.

---

## 2. Why “only better ESTIMATE” is insufficient

ESTIMATE is a **poison marker for speculative readers** after a writer incarnation is aborted. Refining ESTIMATE (mark fewer locations) reduces false cascade, but:

- Does not create the **correct version** for waiting readers (Bohm’s “fill placeholder”).
- Does not **rebind** a reader to the right prior version without re-exec.
- Does not **avoid** the conflict via Wait / version selection **before** the bad read.
- Does not enable **partial retry** from last good region checkpoint.
- Does not merge commutative writes (PEVM lazy balance already does a special case).

So ESTIMATE tuning is a **cascade hygiene** tool, not the SpecFence conflict-resolution core.

---

## 3. Action space (from classic + learned CC, specialized to chain)

Polyjuice (OSDI’21) decomposes CC into per-access actions. Bohm / IC3 / Callas / PWV add deterministic and piece-level options. Under **preset order + MV memory**, SpecFence’s learnable / selectable actions per **region access** are:

### 3.1 Before / during access

| Action | DB analogue | Blockchain SpecFence meaning |
|--------|-------------|------------------------------|
| **NO_WAIT / WAIT_WRITER** | 2PL / Polyjuice wait / MVTSO wait-for-writer | Before read/write of \(x\), wait until last \(T_i\) (\(i<\mathrm{me}\)) that writes \(x\) has published a non-ESTIMATE version (or committed incarnation) |
| **CLEAN_READ** | OCC / Silo read committed | Read last **Validated/Committed** version \(< \mathrm{me}\) |
| **ORDERED_DIRTY_READ** | Polyjuice DIRTY_READ; IC3 piece visibility | Read latest **Executed** version \(< \mathrm{me}\) (preset order makes this the only dirty candidate worth reading) and record dependency |
| **VERSION_BIND** | Bohm read-from after write-set known | After a prior incarnation of \(\mathrm{me}\) or of \(T_i\) exposed write-set, bind \(x\) to exact `(tx_idx, incarnation)` |
| **PRIVATE_WRITE / PUBLIC_WRITE** | Polyjuice write visibility | Buffer write until region validation passes vs publish immediately into MV chain (Block-STM publishes early) |

### 3.2 Validation / repair (finer than whole-tx abort)

| Action | DB analogue | SpecFence meaning |
|--------|-------------|-------------------|
| **EARLY_VALIDATE_REGION** | Polyjuice early-validation | After touching \(x\), validate only origins for \(x\) (and maybe its connected component) |
| **PARTIAL_ABORT_TO_CHECKPOINT** | Polyjuice retry from last good validation | Invalidate only regions failed since last region-checkpoint; keep validated region versions |
| **REBIND_READ** | repair without full reexec | If writer republished a new incarnation of same logical write, switch origin pointer and continue if value-equivalent or re-exec **only dependent suffix of tx** |
| **INSTALL_PLACEHOLDER / FILL** | Bohm CC phase | When write-set of \(T_i\) becomes known (from dry-run / prior incarnation), pre-install version slots so \(T_j\) waits on slot ready instead of aborting |
| **SELECTIVE_INVALIDATE** | finer than full ESTIMATE | On abort of incarnation, poison **only** writes that (a) failed validation fan-in or (b) have known higher readers; leave unrelated writes as Data until safe retract |
| **COMMUTE_MERGE** | PEVM lazy sender/recipient | For known commutative locations, don’t create false WW edges |

### 3.3 Scheduling / waves

| Action | Meaning |
|--------|---------|
| **ADMIT_TO_WAVE** | Region-ready set forms a wave; txs enter for their ready regions only |
| **SPLIT_WAVE / MERGE_WAVE** | Connected components of live conflict graph become separate waves |
| **DEFER_REGION** | Push uncertain high-posterior-conflict regions to a later wave; run independent regions now |

Bayesian learning selects among these actions; it does **not** replace the resolution mechanisms.

---

## 4. Diagnosis of current SpecFence (v0–v3)

| Layer | What we have | Failure mode vs OCC |
|-------|----------------|---------------------|
| Unit | Location hash + Beta posterior | Good direction |
| Decision | P(conflict)≥τ → WAIT else SPECULATE | Over-Wait on hot blocks; binary action space too small |
| Resolution on misspeculation | Whole-tx reexec + full write-set ESTIMATE + fenced validation rewind | Dominant cost; learning cannot pay for itself |
| Wave | `wave_id` counter only | Not a real ready-queue / component scheduler |
| Inter-block | Beta carry with decay | OK as prior; unused for version-bind / placeholders |

**CC diagnosis in one line:** we learned *when* to Wait, but still resolve failures with the **coarsest OCC abort hammer**, so fine-grained prediction cannot beat coarse-grained OCC.

---

## 5. Target architecture

```
                    ┌─────────────────────────────────────┐
                    │  Inter-block Bayes / structure prior │
                    │  (per-location + co-access)          │
                    └─────────────────┬───────────────────┘
                                      ▼
┌──────────────┐   admit/defer    ┌───────────────────────┐
│ Region graph │◄────────────────►│ Wave controller       │
│ (live WW/WR) │   split/merge    │ ready regions → waves │
└──────┬───────┘                  └───────────┬───────────┘
       │                                      │
       ▼                                      ▼
┌──────────────────────────────────────────────────────────┐
│ Per-access policy π(state) → action                      │
│ state: (tx, location, posterior, wave, last writer status)│
│ actions: WAIT / CLEAN_READ / ORDERED_DIRTY_READ /        │
│          VERSION_BIND / PUBLIC_WRITE / EARLY_VALIDATE…   │
└────────────────────────────┬─────────────────────────────┘
                             ▼
┌──────────────────────────────────────────────────────────┐
│ MV region store (version chain per location)             │
│ + placeholders, selective invalidate, commute merge      │
└────────────────────────────┬─────────────────────────────┘
                             ▼
┌──────────────────────────────────────────────────────────┐
│ Repair kernel                                            │
│ early validate → rebind → partial abort → selective      │
│ invalidate → dependent-only revalidate (fence)           │
│ full tx reexec = last resort                             │
└──────────────────────────────────────────────────────────┘
                             ▼
              serial equivalence certificate (preset order)
```

### 5.1 Invariants (correctness)

1. For every committed read of \(x\) by \(T_j\), read-from is the last writer \(T_i\) with \(i<j\) in the final write history (or storage if none).  
2. Writes of \(x\) appear in increasing tx index in the final version chain.  
3. Learning never commits a tx that fails (1)–(2).  
4. Beneficiary / lazy commutative paths remain special-cased to avoid false WW.

### 5.2 Objective (performance)

Maximize useful work / wall time ≈ approach OPT (Bohm-with-perfect-write-sets / oracle DAG critical path). Secondary: abort_rate, wait_time, reexec_gas, cascade_revalidations. Not “linear cores”.

---

## 6. Phased implementation (after this design)

**Phase A — Resolution kernel (unblocks beating OCC)**  
1. **VERSION_BIND + placeholder:** after first incarnation, reuse observed write-set to pre-declare locations; readers WAIT_WRITER / bind instead of blind speculate. (Bohm-lite without static analysis.)  
2. **EARLY_VALIDATE_REGION** on hot locations during execute.  
3. **SELECTIVE_INVALIDATE** with a safe reader index (maintain location→readers concurrently as writes publish, not only at abort).  
4. Keep full reexec as fallback; measure fraction of aborts avoided by bind/wait.

**Phase B — Richer policy + Bayes**  
1. Expand action from binary Wait/Speculate to at least `{Wait, CleanRead, OrderedDirtyRead, Bind}`.  
2. Posterior over conflict **and** over best action (bandit / hierarchical Beta).  
3. Cost-aware decision: E[reexec] vs E[wait] vs E[bind miss].

**Phase C — True waves**  
1. Replace single `validation_idx` for SpecFence with per-wave ready queues on region components.  
2. Intra-block split/merge from live graph.  
3. Metrics: wave width vs oracle DAG width.

**Phase D — Partial reexec (hard)**  
1. Region checkpoints / interpreter re-entry, or tx decomposition for common EVM patterns (transfer, ERC20).  
2. Until then, Phases A–C must already reduce whole-tx aborts via avoidance + bind.

---

## 7. What we will *not* do next

- Retune τ alone to chase OCC on charts.  
- Claim SpecFence wins while repair = full ESTIMATE + full reexec.  
- Copy pevm-specfence-server research scaffolding.  
- Reorder commits (breaks chain semantics).

---

## 8. Immediate next coding milestone (only after sign-off on this design)

**Milestone M1:** Bohm-lite version bind + safe selective invalidate + early region validation, with Bayes choosing Wait vs Bind vs Speculate. Success criterion: on the 7 mainnet blocks, SpecFence @8 **≥ OCC @8 on average** *or* clear metric proof that remaining gap is only full-reexec (abort_rate↓, reexec_count↓, wait not exploding). If gap remains with aborts already near zero, revisit wave scheduler next—not more ESTIMATE tweaks.
