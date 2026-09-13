# SpecFence parallel-compute architecture — execute / validate / repair as stages

**Date:** 2026-09-13 (Asia/Shanghai, UTC+8)  
**Status:** **AUTHORITATIVE plant SoT** for this cut. Frozen **π** is unchanged.  
**Branch:** `cursor/specfence-parallel-compute-68a3` (from `cursor/specfence-clean-slate-8598` @ `9bc253f`)  
**π SoT (unchanged):** `lab/notes/specfence-complete-architecture-v4-frozen-grain.md`  
**Supersedes (plant only):** `lab/notes/specfence-clean-slate-architecture.md` — two-mode identity kept; **execute/validate graft is not the Unfenced SoT**.  
**Honesty baseline:** clean-slate nonempty median **SF/OCC = 0.464**; quiet heuristic **0.716** (not ≈1.0).  
**Vocab:** Spec = Region. Fence = Bind / WaitFor / serial-lane / ordered-admit on a Region-**access**. Unfenced ≡ OCC **literally** (same helpers **or** a shared pure read/validate kernel). CC = frozen π OCC⊗PCC — **one layer inside** the parallel computer, not the computer.

This document specifies SpecFence as a **preset-order parallel EVM computer**: a task graph, a ready set, work-stealing, and a pipeline of compute stages. Concurrency control (frozen π) decides *which read* an execute-stage access takes. It does **not** own the schedule loop, the validate walk, or the rem journal.

---

## 0. Essence (one paragraph)

A block is an ordered list of transactions that must **commit** in preset order. Parallelism is a **compute** problem: keep P cores on useful EVM work. Tasks are `execute(a)` (one access / one incarnation body), `validate(tx)` (read-origin check), and `repair(grain)` (B0 reincarnation or certified PrefixSkip). A ready set + steal keeps cores off idle; execute∥validate pipeline where a completed incarnation’s validate can run on another core while independents execute. Frozen π (OCC default per access, thin PCC on PredictedEssential) is the **CC overlay** on the execute-stage memory op — not a second Block-STM grafted through rem-write and `collect_invalid_reads`. `SpecFenceExecutor` owns Db/rem/validate **for SpecFence mode**. The OCC path has **zero** SpecFence symbols on schedule / execute / validate ticks. Unfenced incarnations compile to the **OCC kernel** (same `occ_read` + same `validate_read_locations` + B0). PCC overlay is thin and only when PE ∩ ROI. Learning stays off the Unfenced critical path. Success = median SF/OCC↑ vs 0.464, quiet≈1.0, fan_out↑, Soft=0.

---

## 1. Why CC-only redesign failed

Clean-slate unified Unfenced **reads** with OCC helpers. Median 0.436 → **0.464**. Quiet heuristic **fell** to 0.716 (20/48 ≥1). The leftover tax was not “PCC verbs still learning.” It was **shared execute/validate as OCC-shaped grafts**.

### 1.1 Wall is not Fence-verb cost

```
wall = useful_EVM + idle + repair + meta
```

| Term | What it is | CC-only view | Parallel-compute view |
|------|------------|--------------|------------------------|
| **useful_EVM** | Interpreter + MV read/write | assumed dominant | the only work that should scale with P |
| **idle** | park, yield, validation-first stampede, wave miss | SoftWait / WaitFor verbs | **ready-set + steal + pipeline** |
| **repair** | reincarnation, PrefixSkip, RebindThis | Resolve ladder | **repair stage**; B0 = OCC residual |
| **meta** | rem journal, CallEntry, `collect_invalid_reads` Vec + repair museum, HotSet, success-path revoke walk | “needed for seq≡par” | **graft**; Unfenced must not pay it |

Quiet 2179522 N=1 **0.405** / N=3 **0.385** with bind=0 is the smoking gun: **no PCC fired** and SF still lost to OCC. That cannot be a Fence-verb problem. It is idle + meta on a shared OCC skeleton.

### 1.2 The graft that kept quiet SF≪OCC

Clean-slate **left** rem-write / CallEntry / `collect_invalid_reads` on every SpecFence incarnation because a naive strip failed ERC-20 / Bind seq≡par. Diagnosis of that failure (not a reason to keep the graft):

1. They skipped rem-write (or gated it on `quiet_fence_off` / empty-PE at a racy time).  
2. They **still** ran SpecFence validate: `collect_invalid_reads` → RebindThis / PrefixSkip / first_k / CallEntry plan.  
3. Journal-less repair **is not OCC**. RebindThis without this-incarnation identity can accept a stale origin → **seq≠par**. PrefixSkip with only a synthetic CallEntry at k=0 is the same class.  
4. Conclusion they drew: “must leave the graft.”  
5. Correct conclusion: **Unfenced incarnations must never enter SF repair.** Validate of an OCC-kernel incarnation is OCC validate + B0. Repair is a **stage** entered only when this incarnation journaled PCC / PrefixSkip.

`collect_invalid_reads` is OCC-shaped: allocate a Vec of every invalid origin, then walk it for a Resolve museum. OCC only needs a **bool** (first mismatch). SpecFence should pay the Vec **only** on the PCC/repair fail path.

### 1.3 Shared `try_validate` / `Vm::execute` is the wrong module boundary

`pevm.rs::try_validate` is one function for OCC, PCC-legacy, and SpecFence. SpecFence success still:

- walks `read_locations` for Wait revoke  
- `rw_prior.observe_write_set`  
- `rem.note_checkpoint_opportunity`  
- clears force_bind / FF / repair DashMaps  

SpecFence execute still:

- `push_checkpoint(CallEntry)` on every incarnation  
- `note_write_replay` on every SSTORE  
- `note_access` + AccountWrite/StorageWrite/CallExit checkpoints  
- HotSet `note_writer` + `wake_on_data_publish`  

Those run even when `decide()` is UnfencedOcc and `pcc_armed` never flipped. That is **meta on useful_EVM**, paid on the quiet cohort.

### 1.4 Schedule is still OCC Block-STM + wave intercept

`Scheduler::next_task_with_wave` is OCC collaborative indices with a park deque bolted on. SpecFence did not own:

- a ready set of **stages** (execute vs validate vs repair)  
- pipeline admission (validate of completed OccKernel while independents execute)  
- steal that prefers useful_EVM over repair storms  

CC-only WaitFor/Bind cannot fix **validation-first stampede** or **idle after park** if the computer is still OCC’s two counters.

---

## 2. Parallel compute model (preset-order EVM blocks)

### 2.1 Objects

| Object | Meaning |
|--------|---------|
| **Block** | txs `0..n-1`; **commit order = preset order** (seq≡par TCB) |
| **Incarnation** | one execute of tx `t` (bookkeeping `inc`; **not** Avoid key) |
| **Access-event \(a\)** | frozen π identity \(a=(t,k,\mathrm{depth},\ell,\mathrm{mode})\) |
| **Stage** | `Execute` \| `Validate` \| `Repair` — parallel **compute** stages |
| **Kernel** | `OccKernel` \| `PccKernel` — which **computer** this incarnation is |
| **Ready set** | stages that may run now without violating hang-freedom / preset commit |
| **Steal** | idle worker takes a ready stage from another deque / global frontier |
| **Wave** | set of incarnations with no unpublished predicted-essential RAW into them |
| **Serial-lane** | one logical lane on a predicted-essential access class (not fleet park) |

### 2.2 Task graph (not a dataflow rewrite of EVM)

```
execute(t, inc)  ──publish WS/RS──►  validate(t, inc)
                      │
                      │ invalid ∧ OccKernel
                      └──► repair = B0 reincarnate(t, inc+1) ──► execute(t, inc+1)

                      │ invalid ∧ PccKernel ∧ value_stable PE edges
                      └──► repair = R1a RebindThis ──► (re)validate

                      │ invalid ∧ PccKernel ∧ certified prefix
                      └──► repair = R1b PrefixSkip / E1 residual ──► execute from k*

                      │ else
                      └──► repair = B0
```

Edges that **admit** a later `execute(t')`:

- no predicted-essential unpublished RAW `t → t'` (ordered admission / serial-lane)  
- or Unfenced: OCC speculation (ESTIMATE → Blocking), same as Block-STM  

This is **not** “tx waits.” Admission is per **access class / edge**, then the **incarnation** is either ready or parked on that grain.

### 2.3 Ready set and work-stealing

```
ready = { Execute(t) | status=Ready ∧ (Unfenced ∨ PE producers published ∨ serial-lane token) }
      ∪ { Validate(t) | status=Executed ∧ (lazy-skip rules as OCC) }
      ∪ { Repair(t)   | validate failed ∧ kernel chose a repair stage }

steal:
  1. local Execute of independent ready (wave-first)     # useful_EVM
  2. Validate of any Executed (pipeline)                 # cheap if OccKernel
  3. Repair B0 of aborted (OCC residual)
  4. serial-lane progress on PE class                    # PCC without fleet park
  never: SoftWait Soft wake storms
```

Hang-freedom = serial-lane progress **or** Bind race **or** steal from independents. Same frozen law; now a **schedule** law.

### 2.4 Wave / pipeline parallelism

**Wave:** at time τ the independent width is the set of Ready executes with no unpublished PE-RAW. Occupancy ≤ `min(P, wave_width)`.

**Pipeline execute∥validate:**

- After `execute(t)` publishes, `validate(t)` is a **different stage** and **may run on another core immediately**.  
- OccKernel validate is the OCC bool walk — cheap enough that pipeline hides it.  
- PccKernel validate may RebindThis without re-execute (repair stage, still not execute).  
- Do **not** force the same worker to execute-then-validate-then-repair as one OCC `try_validate` museum.

**Core utilization law:**

```
idle_frac ≈ 1 - useful_EVM / (P × wall)
```

A CC redesign that cuts Fence tax but leaves validate-meta on every tx, or parks the fleet on one WaitFor, **cannot** raise occupancy. The computer must keep independents in the ready set.

### 2.5 Stages vs CC layer

```
┌─────────────────────────────────────────────────────────────┐
│ SpecFenceExecutor  (parallel computer)                      │
│  ready queue · steal · wave · pipeline                      │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐                   │
│  │ Execute  │  │ Validate │  │ Repair   │   stages          │
│  └────┬─────┘  └────┬─────┘  └────┬─────┘                   │
│       │             │             │                         │
│       ▼             ▼             ▼                         │
│  ┌─────────────────────────────────────┐                    │
│  │ CC overlay (frozen π OCC⊗PCC)       │                    │
│  │  Unfenced → OCC kernel              │                    │
│  │  PE ∩ ROI → Bind / WaitFor / lane   │                    │
│  └─────────────────────────────────────┘                    │
│       │                                                     │
│       ▼                                                     │
│  shared revm + MvMemory                                     │
└─────────────────────────────────────────────────────────────┘

OCC path: Block-STM computer. Zero SpecFence symbols on ticks.
```

CC answers: *for this access, Bind / WaitFor / Unfenced?*  
The computer answers: *which stage runs on which core now?*

---

## 3. Full split

### 3.1 `SpecFenceExecutor` owns Db / rem / validate for SF mode

| Owned | SpecFence | OCC |
|-------|-----------|-----|
| Schedule loop | `executor::next_task` (ready + steal + wave) | `Scheduler::next_task` — **no** wave / fence |
| Execute wrap | kernel begin; rem **only** if PccKernel | `Vm::execute` with **zero** rem / CallEntry / SF metrics grafts |
| Access (Db) | `access_policy` → `occ_read` \| `pcc_overlay` | `occ_read` only (`maybe_wait` = `Ok(())`) |
| Validate | OccKernel → OCC bool walk; PccKernel → Resolve | `validate_read_locations` (bool, no Vec) |
| rem / CallEntry / FF / first_k | PccKernel + Repair only | **never constructed as SoT** (inert alloc OK) |
| Learning | abort / end_block / PCC | **zero** calls |

### 3.2 OCC path: zero SpecFence symbols on ticks

Law: `ConcurrencyMode::Occ` worker does not call `wave_*`, `fence_*`, `decide_access`, `partial_retry`, `collect_invalid_reads` repair, `note_write_replay`, or learner. Inert `SpecFenceCtx` construction is a type convenience, not a license to dereference on the OCC hot path.

### 3.3 Incarnation kernel (the seq≡par cut)

```
begin_execute(t, inc):
  if rewind_resume ∨ ff_head ∨ PrefixSkip armed:
      kernel[t] := PccKernel          # Repair already owns rem
  else:
      kernel[t] := OccKernel          # default = OCC computer

on first PCC Fire (Bind | WaitFor) for this incarnation:
  kernel[t] := PccKernel              # upgrade; rem journal from finalize

validate(t):
  if kernel[t] == OccKernel:
      OCC validate_read_locations     # bool; no Vec; no RebindThis
      fail → Repair = B0 (estimates + reincarnate)
      success → done
  else:
      SF Resolve (R1a / R1b / B0)     # Vec + rem legal — journal exists
```

**Why this keeps seq≡par and quiet≈OCC:**

- OccKernel execute does not write rem / CallEntry / FF. OccKernel validate does not RebindThis / PrefixSkip. There is **no journal-less repair** — the ERC-20 / Bind failure mode of the naive strip.  
- Bind tests fire PCC → upgrade to PccKernel **before** finalize → rem journal exists → Resolve is allowed.  
- Quiet / empty-PE: every incarnation stays OccKernel → **same machine path class as OCC** after the detect atomic + PE-empty gate.  
- Mid-block PE flip: already-finished OccKernel incarnations **stay** OccKernel for that inc. New executes start OccKernel and upgrade only if **this** incarnation Fires PCC.

**Ban:** `quiet_fence_off` as the rem-write gate while validate still runs SF repair. That is the previous strip.

### 3.4 Unfenced ≡ OCC literally

Unfenced is not “OCC-ish.” Two allowed implementations (same observable):

1. **Call the same functions** OCC uses: `occ_read_*` + `validate_read_locations` + `convert_writes_to_estimates` + `finish_validation`.  
2. **Shared pure read/validate kernel** extracted so both modes compile to the same helper (no `if SpecFence` inside it).

This cut uses (2) for validate (`MvMemory::validate_read_locations` = first-mismatch bool, no Vec) and (1) for reads (`occ_unfenced` → OCC `storage`/`basic`). ESTIMATE → `Blocking`. No FF, no OrderedDirtyRead, no rem, no residual-Bind on OccKernel.

### 3.5 PCC overlay remains thin

Invoked only on `PredictedEssential(ℓ,k,morph)` ∩ `pcc_makespan_win` (intra abort evidence; quiet_fence_off stays). Verbs: Bind / WaitFor / serial-lane / ordered-admit **for this \(a\)**. Edge / rem / process / FF only here and on PccKernel finalize / Repair.

### 3.6 Learning off Unfenced critical path

Detect coverage = one Relaxed atomic on SpecFence access. `note_detect` / process DashMap / decision fields: PCC Fire, abort, end_tx, end_block. OccKernel abort still `note_abort_access` (trains PE) — that is the **repair** stage, not execute-read.

---

## 4. Cost model

```
OCC_wall ≈ useful_EVM + reincarnation_recovery + occ_validate_bool + occ_sched

SF_clean_slate ≈ useful_EVM
               + Σ_tx (CallEntry + rem_write + HotSet + wake)
               + Σ_validate (collect_invalid_reads Vec + repair museum or success revoke walk)
               + idle (OCC indices + wave graft)
               + Σ_PE timely_PCC

SF_parallel ≈ useful_EVM
            + Σ_access (detect atomic [+ bump_k + PE probe if has_pe])
            + Σ_{a : PE ∧ ROI} timely_PCC_tax_on_a
            + Σ_OccKernel_validate OCC_bool_walk
            + Σ_PccKernel_fail Resolve
            + Σ_miss B0
            + idle(ready, steal, pipeline)
```

**Invariants:**

1. Empty PE table ⇒ every incarnation OccKernel ⇒ SF execute/validate ≡ OCC (± detect atomics).  
2. ¬PCC-Fire incarnation ⇒ OccKernel even if the block’s PE table is nonempty (sibling cold txs).  
3. Quiet / META_COLD ⇒ quiet median **≈ 1.0**.  
4. Miss ⇒ B0, not SuffixRepair / SoftWait / ForcePrefix.  
5. One PE access must not sticky-Wait the rest of the tx.

---

## 5. Frozen π (held — do not reopen)

```
a = (t, k, depth, ℓ, mode)                    # inc NOT in Avoid key
e_vis = (writer?, published_Data?, edge_kind)
gate  = PredictedEssential(ℓ, k, morph) ∨ independence_certified
verb  = Bind | WaitFor|serial-lane|ordered_admit | Unfenced≡OCC
        # THIS a only — never sticky Wait-on-tx
        # Unfenced≡OCC means OccKernel, not a SpecFence fast-path
```

Hybrid: OCC default **per access**; learned timely PCC on predicted-essential Region-accesses. Spec = Region. Full exclude set from frozen §0 holds.

---

## 6. Hard bans (plant)

| Ban | Why |
|-----|-----|
| rem-write / CallEntry / FF / `first_k` on OccKernel | Unfenced > OCC |
| `collect_invalid_reads` + RebindThis/PrefixSkip on OccKernel | journal-less repair = seq≠par **or** quiet tax |
| Shared `try_validate` museum as Unfenced SoT | graft |
| `quiet_fence_off` as rem gate while SF repair stays on | previous strip |
| SoftWait Soft / ForcePrefix / canary / H-OR / `inc` Avoid / morph actuator | frozen |
| New π fields | frozen |
| P0/P1/P2 staged land | one package |
| Celebrating abort↓ while quiet≪1 | wall/TPS vs OCC is the bar |

---

## 7. Success metric

**Primary:** nonempty all-blocks median SF/OCC TPS (and wall) @8.  
**Honesty baseline:** **0.464** (`9bc253f` clean-slate).  
**Goals:** median **↑**; quiet heuristic **≈ 1.0**; fan_out **↑**; Soft=0; await=0; exclude-set=0.  
**Do not claim ≥0.7** without JSON.

Falsifiers: OccKernel rate on quiet ≈ 1; PccKernel only where `pcc_fire_at_a`>0; OCC mode detect/pcc/unfenced = 0; mixed_verb > 0 on fan_out when morph says so.

---

## 8. Module map

```
crates/pevm/src/
  mv_memory.rs          # validate_read_locations = first-mismatch bool (shared kernel)
  pevm.rs               # mode → occ_worker | specfence::executor worker
  scheduler.rs          # Block-STM status words; SF ready/steal via executor
  vm.rs                 # OCC reads + thin gate; rem/CallEntry only if PccKernel
  specfence/
    kernel.rs           # OccKernel | PccKernel per incarnation
    executor.rs         # schedule loop, validate OccKernel stage, steal/pipeline
    access_policy.rs    # frozen π gate (unchanged)
    rem.rs              # PccKernel / Repair only
    edge.rs / learner.rs / process.rs   # PE / abort / end_block
```

---

## 9. Control loop

```
begin_block(mode):
  OCC:       MvMemory + Scheduler
  SpecFence: seed PE from InterBlockPrior if !quiet; KernelTable = Occ

OCC worker:
  next_task()                  # no wave
  execute: occ_read; no rem
  validate: validate_read_locations; B0

SpecFence worker:
  next_task = ready ∪ steal ∪ pipeline validate
  execute:
    kernel := Occ unless Repair armed
    access: detect; Unfenced → occ_read; PE∩ROI → pcc (mark PccKernel)
    finalize rem/CallEntry/HotSet  iff PccKernel
  validate:
    OccKernel → OCC bool + B0 (+ note_abort for learning)
    PccKernel → R1a / R1b / B0
  steal independents on WaitFor; serial-lane on PE class

end_block SpecFence:
  learner EMA; falsifiers (quiet≈1, median↑, Soft=0)
```

---

## 10. Correctness

1. Preset-order commit. Seq ≡ par TCB.  
2. OccKernel safety = OCC safety (same read + same validate + same B0).  
3. PccKernel wrong prediction = performance only.  
4. Journal-less Repair is a **protocol bug**.  
5. Hang-freedom: serial-lane or Bind or independent steal; no SoftWait Soft.  
6. Detect honesty: coverage atomic on SpecFence accesses.  
7. Grain honesty: mixed verbs inside one tx expected; exclude-set live OR = protocol bug.

---

## 11. Single-iteration land (no P0/P1/P2)

1. This SoT.  
2. `KernelTable` + OccKernel execute (no rem/CallEntry/HotSet).  
3. OccKernel validate = shared OCC bool kernel (no `collect_invalid_reads` tax).  
4. PccKernel / Repair only when PCC Fire or PrefixSkip armed.  
5. SpecFenceExecutor owns SF schedule + validate dispatch.  
6. OCC worker: no wave/fence/SF validate.  
7. Tests + all-blocks / focus sweeps vs **0.464**. Honesty required.  
8. Impl map `lab/notes/specfence-parallel-compute-impl.md`.

Partial land (kernel flag that still rem-writes, or OccKernel that still RebindThis) is a **non-land**.

---

## 12. Supersedes (plant only)

| Doc | Role |
|-----|------|
| `specfence-complete-architecture-v4-frozen-grain.md` | **π AUTHORITATIVE** |
| `specfence-clean-slate-architecture.md` | two-mode parent; graft leftover absorbed here |
| `specfence-clean-slate-impl.md` | historical; quiet 0.716 lesson |
