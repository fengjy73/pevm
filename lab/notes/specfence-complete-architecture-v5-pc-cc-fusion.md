# SpecFence complete architecture v5 — PC⊗CC fusion (AUTHORITATIVE SoT)

**Date:** 2026-09-13 (Asia/Shanghai, UTC+8)  
**Status:** **AUTHORITATIVE** complete architecture — **diagnosis + design only; DO NOT implement yet**  
**Tip at write:** `4a91b5f`  
**Diagnosis:** `lab/notes/specfence-pc-cc-fusion-all-blocks-diagnosis.md`  
**Catalog:** `lab/notes/specfence-pc-cc-fusion-per-block-catalog.json`  
**Supersedes (plant + fusion SoT):**  
- `lab/notes/specfence-parallel-compute-architecture.md` — stages kept; **OccKernel/PccKernel incarnation fork demoted**  
- `lab/notes/specfence-clean-slate-architecture.md` — two-mode identity absorbed into one computer  
**π identity KEPT (fields):** `lab/notes/specfence-complete-architecture-v4-frozen-grain.md` — \(a\), \(e_{\mathrm{vis}}\), PE∨independence, exclude set, Soft=0  
**π/plant SUPERSEDED (see §0.3):** incarnation kernel fork as mode SoT; residual-k=1 PE train; prior-PE never Fires; Resolve-only-on-PccKernel  
**Vocab:** Spec = Region. Fence = Bind / WaitFor / serial-lane / ordered_admit on Region-**accesses**. Soft = **0 ban**. No P0/P1/P2 staging in this design.  
**Honesty bar to beat:** median SF/OCC **0.744** (committed) / **~0.80** (this Soft=0 N=1); quiet median ~1.0 with **stable** quiet tails; fan_out **14689597** still **0.37–0.61** — **not done**.

---

## 0. Essence (ONE paragraph)

**SpecFence v5** is one **preset-order parallel EVM computer** whose concurrency mode is an **access-local state machine**, not two grafted protocols. A block is a task graph of stages `Execute` / `Validate` / `Repair` over ready-set + steal; wall law is `wall = useful_EVM + idle + repair + meta`. Every storage/account/CALL boundary is access-event \(a=(t,k,\mathrm{depth},\ell,\mathrm{mode})\) with EdgeVisibility \(e_{\mathrm{vis}}\). Default mode for \(a\) is **Spec** (OCC-cost ESTIMATE→Blocking read + bool validate + B0 miss) — literally the same helpers as OCC for that access. Mode upgrades to **Fence** verbs (Bind / WaitFor / serial-lane / ordered_admit) only by **events** on that \(a\) or its access-class \((\ell,k,\mathrm{morph})\): published Data, executing producer, PredictedEssential with matching \(k\), or certified-prefix Repair — never by a tx-global Occ/Pcc kernel bit and never by SoftWait Soft. Learning writes **structure** (PE classes, serial-lane tokens, ready edges, prefix certificates), not OR-bool museum gates. Success = median SF/OCC↑ vs 0.744 **and** quiet≈1.0 **stable** **and** fan_out↑ **and** Soft=0; celebrating abort↓ while fan_out≪OCC is forbidden.

---

## 0.1 Why v4.1-frozen ⊗ parallel-compute failed (one screen)

```
v4.1-frozen:  Avoid/Resolve at a     — correct identity
parallel-compute: OccKernel|PccKernel per incarnation — wrong mode carrier

Meeting point: TryPcc → mark_pcc(tx) after intra-abort PE ∩ ROI
Miss path:     OccKernel validate → B0 only (Resolve dead)
Train path:    abort → PE(ℓ, k=1 residual)  ≠ true k≈6 on fan_out stars

Result: unfenced_occ_fast ≫ pcc_fire; full_restart ≡ occ_aborts; WaitFor≈0;
        parks = ESTIMATE Blocking; median 0.744 ≠ product bar.
```

Fusion failed because **mode was an incarnation fork gated post-abort**, while wall is decided by **access/edge schedule + repair class**.

---

## 0.2 Hard bans (held)

| Ban | Hold |
|-----|------|
| SoftWait Soft storms | **yes** |
| Tx-level sticky Wait / ForcePrefix-as-π / canary live / H-OR / `inc` Avoid / morph actuator / writer_validated Bind gate / flat EdgeKey SoT | **yes** (exclude set) |
| Unfenced access cost class > OCC | **yes** |
| Journal-less RebindThis / PrefixSkip | **yes** (Repair needs certificate) |
| P0/P1/P2 staging in design docs | **yes** — one coherent design |
| Celebrating 0.744 / abort↓ while fan_out weak | **yes** |
| EV Await doors / AdaptiveParams-as-θ OR-bools | **yes** |

---

## 0.3 What changes vs frozen π / PC plant

| Item | v4.1-frozen / PC plant | **v5** |
|------|------------------------|--------|
| Mode carrier | `KernelTable[tx]` Occ\|Pcc | **`Mode(a)` access-local SM** (+ optional certified-prefix journal strip) |
| When OCC↔PCC | first Fire / repair_armed | **event-driven** on \(a\) / class (see §4) |
| PE train on Spec abort | residual **k=1** | **`k` from bump_k / effect ordinal already on path** — no rem required |
| Prior PE | never TryPcc (`roi_skip`) | **may Fence when \(e_{\mathrm{vis}}\) says Data or Executing writer**; still no Bind-tax on quiet empty \(e_{\mathrm{vis}}\) |
| Resolve | PccKernel museum only | **Repair stage** keyed by fail grain + certificate; Spec miss stays B0 |
| Schedule | Block-STM + wave graft | **ready stages + steal + pipeline** (PC intent, completed) |
| π fields \(a,e_{\mathrm{vis}},gate\) | frozen | **kept** |
| Soft / exclude | frozen | **kept** |

---

## 1. System model

### 1.1 Objects

| Object | Meaning | SoT? |
|--------|---------|------|
| **Block** | txs `0..n-1`; commit order = preset order | yes |
| **Access-event \(a\)** | \(a=(t,k,\mathrm{depth},\ell,\mathrm{mode})\) | **yes — primary** |
| **EdgeVisibility \(e_{\mathrm{vis}}\)** | (writer?, published_Data?, edge_kind) | **yes** |
| **Gate** | PredictedEssential\((\ell,k,\mathrm{morph})\) ∨ independence_certified | **yes** |
| **Mode(a)** | Spec \| Bind \| WaitFor \| SerialLane \| OrderedAdmit | **yes — live verb** |
| **Stage** | Execute(t,inc) \| Validate(t,inc) \| Repair(grain) | **yes — computer** |
| **Certificate** | rem/CallEntry/first_k strip **only for Fenced accesses / prefix** | Repair only |
| **Incarnation `inc`** | bookkeeping | not Avoid key |
| **Ready set** | stages runnable without hang / preset violation | schedule |
| **Serial-lane token** | one PE access-class lane | Fence without fleet park |

### 1.2 Cost law

```
wall = useful_EVM + idle + repair + meta

OCC_wall ≈ useful_EVM + B0_reincarnation + bool_validate + STM_sched

SF_v5 ≈ useful_EVM
      + Σ_{a : Mode=Spec} (OCC_read_meta ≈ detect_atomic)
      + Σ_{a : Mode∈Fence} (timely_Fence_tax_on_a)
      + Σ_Validate (bool on Spec-only RS; Resolve walk only if certificate exists)
      + Σ_Repair (R1a/R1b if certified; else B0)
      + idle(ready, steal, pipeline)
```

**Invariants:**

1. Empty PE ∧ no Fence events ⇒ SF_wall ≡ OCC_wall (± detect atomics).  
2. Correct timely Fence on essentials ⇒ Fence tax ≪ avoided B0 cascades **and** sibling Spec accesses keep OCC width.  
3. Miss ⇒ B0 residual after certified prefix — never SoftWait / SuffixRepair-as-default / ForcePrefix.  
4. One Fenced access must not sticky-Wait the rest of the tx.  
5. Soft = 0.

### 1.3 Success metrics

**Primary:** nonempty all-blocks median SF/OCC TPS @8.  
**Bars:** beat **0.744** median; quiet heuristic median **≈1.0** with p10 quiet **≥0.85** (kill 2179522-class N=1 cliffs); named fan_out **14689597 ≥0.85**; Soft=0; exclude=0.  
**Falsifiers:** Spec-path rem/CallEntry > 0; Soft > 0; `inc`-keyed Avoid > 0; Fire without \(e_{\mathrm{vis}}\) or PE; Resolve without certificate; journal-less RebindThis > 0; median claim without JSON.

---

## 2. One computer — stages

```
                    ┌─────────────────────────────────────────┐
                    │ SpecFenceComputer                       │
                    │  ready ∪ steal ∪ wave ∪ pipeline         │
                    │                                         │
                    │  Execute ──publish──► Validate           │
                    │     │                   │               │
                    │     │                   ├─ ok ──► commit progress
                    │     │                   │               │
                    │     │                   └─ fail ──► Repair(grain)
                    │     │                         │         │
                    │     │            R1a rebind / R1b prefix│
                    │     │            / B0 reincarnate       │
                    │     └────────────◄──────────────────────┘
                    │                                         │
                    │  Mode(a) overlay on each Execute access │
                    │  shared revm + MvMemory                 │
                    └─────────────────────────────────────────┘

OCC mode: same computer with Mode(a)≡Spec always; zero Fence symbols on ticks.
```

### 2.1 Ready set + steal (complete the PC intent)

```
ready =
  { Execute(t)  | status=Ready ∧ admission_ok(t) }
∪ { Validate(t) | status=Executed }
∪ { Repair(g)   | validate_failed ∧ repair_plan(g) }

admission_ok(t):
  ∀ PE RAW edges into t: producer published ∨ serial-lane token held
  ∨ all inbound edges are Spec (OCC speculation allowed)

steal priority:
  1. Execute independent (max wave width)     # useful_EVM
  2. Validate Spec-only RS (bool, other core) # pipeline
  3. Repair R1a (no re-execute)
  4. Serial-lane progress / OrderedAdmit
  5. Repair B0
  never: SoftWait Soft wake storms
```

Hang-freedom = serial-lane progress ∨ Bind race ∨ steal independents.  
**Ban:** validation-first stampede that starves Execute when ready Execute exists.

### 2.2 Pipeline execute∥validate

After Execute publishes WS/RS, Validate is a **different stealable stage**. Spec-only read-sets use OCC bool walk (first mismatch). Fence-certificate read-sets may enter Resolve in Repair without forcing the Execute worker to run the museum inline.

---

## 3. Access-local mode state machine (fusion core)

### 3.1 States

```
Mode(a) ∈ {
  Spec,           # Unfenced ≡ OCC cost for THIS a
  Bind,           # read published Data origin
  WaitFor,        # block this access until Data (one writer) — Soft=0
  SerialLane,     # hold lane token for PE class (ℓ,k,morph)
  OrderedAdmit    # admit Execute only after producer publish
}
```

**Not a tx sticky bit.** Sibling accesses in the same incarnation may differ.

### 3.2 Decision (primary loop)

```
on access-event a = (t, k, depth, ℓ, mode):
  Detect.coverage_atomic()                    # always, cheap
  e_vis := observe(writer?, published_Data?, kind)
  # observe-only features → learning buffers (not live OR-bools)

  gate_pe := PredictedEssential(ℓ, k, morph)
  gate_ind := independence_certified(ℓ, k)

  if gate_ind:
      Mode := Spec
  else if published_Data:
      Mode := Bind                            # event: Data
  else if gate_pe ∧ writer_Executing:
      Mode := WaitFor | SerialLane            # event: PE ∧ live producer
  else if gate_pe ∧ writer_known ∧ ¬Data:
      Mode := OrderedAdmit | SerialLane       # event: PE ∧ known producer
  else if gate_pe ∧ ¬writer:
      Mode := Spec                            # PE but nothing to Fence yet — OCC miss cheap
  else:
      Mode := Spec                            # default OCC

  act(Mode)  # Spec → occ_read; Bind → bind origin; WaitFor → park THIS access only; …
  record_k(ℓ, k) into lightweight AccessOrdinalLog[t]   # NO rem; used on abort train
```

**Key supersession:** prior PE **may** Fence when \(e_{\mathrm{vis}}\) shows Data or Executing writer. Prior PE with empty visibility stays Spec — protects quiet Bind-tax without `roi_skip` theater that blocks all first-wave Avoid.

### 3.3 Event-driven transitions (not threshold salad)

| Event | Transition |
|-------|------------|
| Data published on \(\ell\) before reader \(t\) | Spec/WaitFor → **Bind** for this \(a\) |
| Writer becomes Executing ∧ PE(\(ℓ,k\)) | Spec → **WaitFor/SerialLane** |
| Validate fail on Spec read of PE class | arm PE intra; next access same class may Fence; Repair=B0 this inc |
| Validate fail with certificate (Fenced reads in RS) | Repair = R1a if value_stable; else R1b if prefix certified; else B0 |
| Abort Spec with AccessOrdinalLog k\* | `note_abort_access(ℓ, cascade, k\*)` — **true k**, not 1 |
| Quiet morph ∧ abort_events < ε ∧ ¬PE_prior_star | stay Spec (quiet law) |
| park_heat high on non-PE | **do not** disable PE Fence; instead steal independents (schedule fix) |

**Deleted as live gates:** `pcc_makespan_win` park≫abort kill-switch; `quiet_fence_off` blanket that zeroes TryPcc while PE empty-check still races; tx `mark_pcc` upgrade.

### 3.4 Certificate / journal scope

```
Spec accesses:     no rem, no CallEntry, no first_k, no FF
Fenced accesses:   journal only those accesses (or contiguous certified prefix)
Repair R1a/R1b:    requires certificate for the failing grain / prefix
Repair B0:         Spec miss path — OCC estimates + reincarnate
```

This preserves seq≡par (no journal-less RebindThis) **without** forcing whole-incarnation PccKernel.

---

## 4. Detect / Avoid / Resolve / Learning

### 4.1 Detect (always, cheap)

- Coverage: one Relaxed atomic (hold).  
- **New (required):** `AccessOrdinalLog[t]` records \((\\ell,k)\) for program reads **without** rem DashMap — bump_k already exists; persist enough to recover min k on abort.  
- Observe-only: H, morph prior, park history, gas/opcode, validate-fail reason — **learning inputs only**.

### 4.2 Avoid (live verbs from π only)

Verbs only: Bind / WaitFor / SerialLane / OrderedAdmit / Spec.  
Gate: PE\((\ell,k,\mathrm{morph})\) ∨ independence.  
Visibility: \(e_{\mathrm{vis}}\).  
**Never:** sticky Wait-on-tx, ForcePrefix π, canary, H-OR, morph actuator, `inc` Avoid.

### 4.3 Resolve (timely at grain)

```
validate fail:
  grains := invalid read locations with their Mode/certificate
  if all Spec:
      Repair = B0
      train PE(ℓ, k_from_AccessOrdinalLog)
  else:
      if value_stable ∧ certificate: R1a RebindThis
      elif certified_prefix: R1b PrefixSkip / resume at k*
      else: B0 residual after drop certificate
```

**Ban:** SuffixRepair-as-default; R2 body thrash when R1a possible.

### 4.4 Learning that drives **structure**

| Learns | Writes into | Live use |
|--------|-------------|----------|
| PE\((\ell,k,\mathrm{morph})\) | gate | Avoid |
| independence_certified | gate | Spec fast |
| serial-lane class heat | admission | SerialLane / OrderedAdmit |
| ready-edge residuals | schedule | steal preference |
| prefix certificate success | Resolve | R1b prior |
| HotSet / WŜ | observe → PE posterior only | **not** Wait OR-door |
| morph cluster | prior decay / warm PE seeds | observe + PE label |

**Deleted dead museums as control:** AEC EV OR-bools, AdaptiveParams-as-θ, Engagement Storm/Quiet as edge actuators, canary reopen theater.

---

## 5. OCC path purity

`ConcurrencyMode::Occ`: Mode(a)≡Spec always; zero Detect PE; zero wave; zero Fence; validate = bool + B0; metrics detect/pcc/unfenced = 0.

---

## 6. Module map (target; not implemented)

```
specfence/
  computer.rs      # ready/steal/pipeline/schedule loop
  mode.rs          # Mode(a) SM + event transitions
  access_log.rs    # lightweight (ℓ,k) ordinal log — Spec-safe
  access_policy.rs # decide from π + e_vis (rewrite gate)
  repair.rs        # R1a/R1b/B0 stage plans
  rem.rs           # certificate strips for Fenced only
  learner.rs       # PE/structure posteriors; kill park-kill-switch
  executor.rs      # validate stages
kernel.rs          # DELETE as SoT (optional debug mirror only)
```

---

## 7. Control loop (live)

```
begin_block:
  seed PE from InterBlockPrior if morph says non-quiet stars
  AccessOrdinalLog clear; certificates clear; ready = Execute(all admitted)

Execute(t):
  for each access a:
    Mode(a) := SM(a, e_vis, gate)
    act(Mode); log (ℓ,k) if Spec|Fence program read
  publish WS/RS; enqueue Validate(t)

Validate(t):
  if RS all Spec: bool OCC validate
  else: validate with certificate-aware origin check
  fail → enqueue Repair(grain)

Repair:
  R1a / R1b / B0 as §4.3; train PE with true k
```

---

## 8. Morph playbooks (how block info raises TPS)

| Morph | Structure | Fence surface | Repair |
|-------|-----------|---------------|--------|
| **fan_out** | wave-first steal; star class SerialLane/Bind at true \(k\) | only \((\ell_\mathrm{star},k_\mathrm{class})\) + true RAW sats | PrefixSkip satellites; B0 rare |
| **long_chain** | spine OrderedAdmit; steal off-spine | spine ℓ PE | R1a tip identity |
| **quiet** | OCC-identical | none | B0 only if any |
| **META_COLD** | minimize meta; no prior Bind | none until intra PE | B0 |
| **park-prone** | never fleet Blocking; lane tokens | PE only | — |

---

## 9. Implementation non-goals (this note)

- **Do not implement v5 code in this task.**  
- No P0/P1/P2 land plan in this SoT — when coding starts, land as one coherent computer cut with Soft=0 harness gates.  
- No resurrection of SoftWait Soft, canary live verbs, ForcePrefix π, AEC OR-bool π.

---

## 10. Success checklist (when eventually implemented)

1. Soft=0, await=0, exclude-set=0 on all-blocks.  
2. Nonempty median SF/OCC **> 0.744** with JSON.  
3. Quiet median ≈1.0 **and** quiet p10 ≥0.85.  
4. 14689597 SF/OCC ≥0.85 @8 N≥3.  
5. `pcc_fire` / Fence acts correlate with PE∧\(e_{\mathrm{vis}}\) at **true k** (597 star k≈6).  
6. `rebind_only + rewind` used when certificates exist; B0 not 1:1 with all aborts on fan_out.  
7. `wait_park` with `edge_wait_for=0` driven down (ESTIMATE stampede fixed by admission/steal).  
8. OCC mode counters remain zero for detect/pcc/unfenced.

---

## 11. Essence restated

Fusion = **same computer**; mode = **access-local state machine**; switch = **events on \(a\)/\(e_{\mathrm{vis}}\)/PE**, not incarnation kernel forks or post-abort threshold salad; learning writes **structure**; Repair is a **stage** with certificates — Spec miss stays OCC-cheap B0; Soft forever 0.
