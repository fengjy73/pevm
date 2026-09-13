# SpecFence clean-slate architecture — two modes, one EVM/MV store

**Date:** 2026-09-13 (Asia/Shanghai, UTC+8)  
**Status:** **AUTHORITATIVE implementation SoT** for the plant. Frozen **π** is unchanged.  
**Branch:** `cursor/specfence-clean-slate-8598` (from `cursor/specfence-frozen-grain-3175` @ `d3a7ae1`)  
**π SoT (unchanged):** `lab/notes/specfence-complete-architecture-v4-frozen-grain.md`  
**Cost-class lesson (why this cut exists):** `lab/notes/specfence-occ-cost-pcc-roi-impl.md` — graft Unfenced≢OCC; all-blocks N=1 nonempty median **0.436**.  
**Vocab:** Spec = Region. Fence = Bind / WaitFor / serial-lane / ordered-admit on a Region-**access**. Unfenced ≡ OCC **code path**, not “OCC-ish with fewer DashMaps.”

This document **diagnoses why pevm-graft fails Unfenced≡OCC** and specifies a **clean-slate plant**: two concurrency modes, one EVM / multi-version store. It does **not** reopen frozen π, CostGate-as-face, Fence-first, or exclude-set fields.

---

## 0. Essence (one paragraph)

SpecFence is not a set of intercepts bolted into Block-STM. It is a **second executor** that shares **revm + `MvMemory`** with OCC and nothing else on the Unfenced path. `ConcurrencyMode::OCC` is the pristine pevm Block-STM runner: **zero** SpecFence calls on execute / validate / schedule ticks. `ConcurrencyMode::SpecFence` has its own scheduler ticks, its own access-policy module, and its own validate/resolve. Every access is an event \(a=(t,k,\mathrm{depth},\ell,\mathrm{mode})\). If ¬PredictedEssential, the SpecFence executor **calls the same OCC read helpers** OCC uses — no SpecFence branch, no Edge SM, no rem journal, no process DashMap, no OrderedDirtyRead, no FF, no `maybe_note_value`. PredictedEssential opens a **thin PCC overlay** (Bind / WaitFor / serial-lane / ordered-admit) for **this \(a\)** only. Learning consumes observe streams **off** the Unfenced critical path. Miss ⇒ OCC residual reincarnation. SoftWait / ForcePrefix / canary / H-OR / `inc` Avoid / morph actuator stay banned.

---

## 1. Diagnosis — why pevm-graft fails Unfenced≡OCC

Honesty bar this cut must beat: cost-class tip median **SF/OCC = 0.436** (`d3a7ae1`). Quiet heuristic was **0.989** (16/33 ≥1), not ≡1.0. That is not “PCC ROI still learning.” It is **shared-path tax**.

### 1.1 The graft is a shared `Vm` / `VmDb`

OCC and SpecFence share one `Vm`, one `VmDb`, one `maybe_wait`, one `storage` / `basic`, one `try_validate`, one `SpecFenceCtx` constructed **even for OCC**. SpecFence is not an executor. It is a **museum of `if mode == SpecFence` branches** on the OCC interpreter.

Consequence: every SLOAD / BALANCE / CALL-adjacent read pays a mode check **and**, in SpecFence mode, a full intercept stack whether or not the access is PredictedEssential.

### 1.2 Intercepts on every SLOAD (even ¬PE)

Cost-class tried to cheapen this (`unfenced_occ_fast`). It still ran on **every** access:

| Graft | Where | Why it is not OCC |
|-------|--------|-------------------|
| `rem.note_effect()` | `maybe_wait` before the mode split | global atomic on **all** SpecFence accesses |
| `record_detect_access` | `maybe_wait_specfence` | OK if it is the *only* extra (one Relaxed atomic) |
| `current_k` + `has_any_predicted` + `predicted_essential` | every access | DashMap probe whenever PE table nonempty |
| `note_detect` sampled 1/16 | Unfenced | **DashMap** `locs.entry` on the Unfenced path |
| `note_access_k_only` | Unfenced + lazy/beneficiary | rem state lock + `first_k` HashMap insert |
| residual-Bind in `unfenced_occ_fast` | reincarnation ∧ sketch/force_writer | **PCC Bind on the Unfenced path** (correctness workaround for leftover FF) |
| sampled `process.record` / `record_decision` | Unfenced 1/8 when PE exists | **DashMap** on the default path |
| metrics soup (`edge_unfenced`, `spec_read`, `unfenced_occ_fast`, …) | Unfenced | atomics OCC never pays |
| `try_ff_storage` / `try_ff_basic` | **all** SpecFence `storage`/`basic` | rem FF lookup before MV read |
| **OrderedDirtyRead** (skip ESTIMATE → prior Data) | **all** SpecFence MV walks | **semantic ≠ OCC** (OCC Blocks on ESTIMATE) |
| `maybe_note_value` | all SpecFence reads | rem HashMap write |
| `maybe_early_val` | all SpecFence reads | lean early-out, still a call + HotSet/Bayes smell |
| `record_db_heavy_op` | **OCC too** | metrics graft on the OCC baseline |
| `hinted_wait_blocker` | every execute | SpecFence walks `from`/`to` then stub-returns |

`unfenced_occ_fast` removed Edge SM / PreferAdmit / canary from ¬PE. It did **not** unify the read. Unfenced remained a **different function** than OCC `maybe_wait` + OCC `storage`.

### 1.3 DashMap Edge / process / sketch on the hot path

Even when `classify_edge` was not called, Unfenced still touched:

- `learner.note_detect` → `DashMap` `locs`
- `process.record` / `record_decision` → `DashMap` `txs` + `locs`
- residual-Bind → `sketch.residual_bind` + `force_writer`
- PCC path (correct) → `edges.record` DashMap

The Edge table as **control SoT on PE** is fine. Edge / process / sketch as **Unfenced observers** are the tax the 99-block field table already forbade (“Unfenced that is more expensive than OCC”).

### 1.4 Shared validate / resolve

`try_validate` always receives `SpecFenceCtx`. OCC uses `validate_read_locations`. SpecFence, on **any** invalid read (including Unfenced-only txs), walks `first_k`, EdgeKey `min_k`, `plan_partial_retry`, identity/FF snaps, R1a, then B0/PrefixSkip. That is a **resolve overlay on OCC aborts**, not “PCC only when PredictedEssential.” Quiet / META_COLD txs that abort once pay the full repair museum.

### 1.5 Why patches on `maybe_wait_specfence` cannot win

Each cost-class patch added a **faster SpecFence branch** (`unfenced_occ_fast`, sampled Detect, ROI skip). The branch still:

1. lives in the shared `VmDb` intercept;
2. diverges from OCC ESTIMATE / FF / rem;
3. accumulates leftover correctness grafts (residual-Bind so FF does not seq≠par).

**Unfenced≢OCC is structural.** The plant must stop grafting and **call OCC**.

### 1.6 What is *not* the diagnosis

- Frozen π is wrong. Grain \(a=(t,k,\mathrm{depth},\ell,\mathrm{mode})\) stays.  
- PCC ROI (`pcc_makespan_win`) is the wrong idea. It stays as the PE Fire gate.  
- “Need more Edge OR-salad.” Banned.  
- Quiet < 1.0 is “N=1 noise only.” 2179522 N=3 was 1.05; the **path** was still SpecFence Unfenced, not OCC.

---

## 2. Clean design — two concurrency modes, one EVM/MV store

```
                    ┌─────────────────────────┐
                    │   Pevm orchestrator     │
                    │   ConcurrencyMode       │
                    └───────────┬─────────────┘
                                │
              ┌─────────────────┴─────────────────┐
              │                                   │
    ┌─────────▼─────────�─┘
                                │
              ┌─────────────────┴─────────────────┐
              │                                   │
    ┌─────────▼─────────┐               ┌─────────▼──────────────┐
    │ ConcurrencyMode   │               │ ConcurrencyMode        │
    │ ::OCC             │               │ ::SpecFence            │
    │                   │               │                        │
    │ pristine Block-STM│               │ specfence/executor     │
    │ scheduler ticks   │               │   scheduler ticks      │
    │ occ validate      │               │ specfence/access_policy│
    │ ZERO SpecFence    │               │ specfence/resolve      │
    │ calls             │               │ learning (off Unfenced)│
    └─────────┬─────────┘               └─────────┬──────────────┘
              │                                   │
              │         ┌─────────────────────────┤
              │         │ PE? thin PCC overlay    │
              │         │   Bind / WaitFor /      │
              │         │   serial-lane / admit   │
              │         └────────────┬────────────┘
              │                      │ ¬PE
              └──────────┬───────────┘
                         │
              ┌──────────▼──────────┐
              │ Shared plant        │
              │   revm interpreter  │
              │   MvMemory          │
              │   OCC read helpers  │  ← Unfenced calls these
              └─────────────────────┘
```

`ConcurrencyMode::Pcc` remains a **legacy third mode** (hinted account Wait). It is not the SpecFence executor and must not grow.

### 2.1 `ConcurrencyMode::OCC` — pristine Block-STM

**Law:** when `mode == Occ`, the execute / validate / schedule ticks make **zero** SpecFence calls.

| Tick | OCC |
|------|-----|
| Schedule | `Scheduler::next_task` — no `WaveParkTable`, no `FenceGraph` |
| Execute start | no `hinted_wait_blocker`, no park-resume, no rem reset / FF replay |
| Access (`storage` / `basic`) | **OCC read helper only** — ESTIMATE → `Blocking`, no FF, no rem, no metrics graft |
| `maybe_wait` | `Ok(())` — Block-STM does not Wait on unfinished writers; it speculates |
| Validate | `MvMemory::validate_read_locations` — no `collect_invalid_reads` repair museum |
| Abort | Block-STM reincarnation — no PrefixSkip / residual-Bind / Edge |

Construction: OCC may allocate inert `SpecFenceCtx` pointers for a single `Vm` type **only if** the OCC hot path never dereferences them. Prefer not constructing learner / edges / sketch / process / wave / rem when mode is OCC. Inert construction is a compile-time convenience, not a license to call.

### 2.2 `ConcurrencyMode::SpecFence` — first-class executor

Own modules (not `vm.rs` intercept soup):

| Module | Responsibility |
|--------|----------------|
| `specfence/executor` | worker ticks: wave schedule, execute wrap, validate/resolve dispatch |
| `specfence/access_policy` | frozen π gate → `UnfencedOcc` \| `Bind` \| `WaitFor` \| serial-lane \| ordered-admit |
| `specfence/learner` | PredictedEssential(\(\ell,k,\mathrm{morph}\)); **writes off Unfenced** |
| `specfence/edge` | Edge SM **behind PE only** |
| `specfence/rem` | \(k\) / PrefixSkip / FF **behind PE / Resolve only** |
| shared `occ_read_*` | the only Unfenced read |

Shares with OCC: **revm** (one interpreter), **`MvMemory`** (versions), **`Scheduler` status words** (Ready/Executing/Executed/Validated/Aborting). Does **not** share: access intercepts, Edge, rem journal, process DashMap, WavePark (OCC never parks).

### 2.3 Unfenced compiles to OCC reads

```
SpecFence access_tick a:
  detect_coverage += 1                    # one Relaxed atomic; Detect ≠ Fence
  if !has_any_predicted:                  # one Relaxed atomic
      return occ_read(ℓ)                  # SAME helper OCC storage/basic uses
  k = bump_k()                            # per-tx usize++; NO first_k / journal
  if ¬PredictedEssential(ℓ, k) ∨ ¬ROI:
      return occ_read(ℓ)                  # SAME helper; no SF branch inside it
  else:
      return pcc_overlay(a, e_vis)        # thin; Bind | WaitFor | serial-lane
```

**`occ_read` laws (both modes):**

1. No `try_ff_*`.  
2. ESTIMATE / aborted incarnation → `ReadError::Blocking` (Block-STM).  
3. No `maybe_note_value`, no `maybe_early_val`, no rem journal.  
4. No `record_db_heavy_op` / Edge / process / sketch.  
5. No OrderedDirtyRead (skip-ESTIMATE). That verb is **PCC-only** if ever kept; default Unfenced does not.

Residual-Bind on Unfenced reincarnation is **deleted**. It existed to paper over stale FF after B0. B0 = clear FF / residual / force_writer and **OCC-read**. PrefixSkip FF is a PCC Resolve actuator, not an Unfenced crutch.

### 2.4 PCC overlay is a separate thin layer

Invoked **only** when `PredictedEssential(ℓ,k,morph)` **and** `pcc_makespan_win` (intra abort evidence; quiet_fence_off stays).

| Verb | When | Effect | Scope |
|------|------|--------|-------|
| Bind | PE ∧ published Data | read certified version; rem journal + checkpoint **here** | this \(a\) |
| WaitFor | PE ∧ single executing writer ∧ ROI | park this reader; admit writer only | this \(a\) |
| serial-lane / ordered-admit | PE star/chain | scheduler tick, not `maybe_wait` OR-salad | access class |
| UnfencedOcc | otherwise | `occ_read` | this \(a\) |

`edge.rs` SM, `process.record`, `note_detect` DashMap, rem `first_k` / journal / FF, sketch residual — **PCC or abort/end-block only**.

### 2.5 Learning off the Unfenced critical path

Frozen Detect law (“always on, cheap”) is **coverage + optional sample**, not DashMap-on-SLOAD.

| Signal | When | Where |
|--------|------|-------|
| `detect_accesses` atomic | every SpecFence access | access_policy (one Relaxed add) |
| `note_detect` DashMap | **not** Unfenced; abort / PE / end-tx / 1/N **PCC** | learner |
| `note_abort_access` | validate abort | already off execute-read |
| `pack_top` / morph EMA | `end_block` | learner |
| process / decision DashMap | PCC verbs + end-tx mixed_verb flush | process |
| mixed_verb_intra_tx | end of **tx** if both PE-fire and Unfenced counted | Cell flush, not per-SLOAD DashMap |

Live Avoid still reads only π (`PredictedEssential`, `e_vis`, independence). Observe fields never OR into Unfenced.

### 2.6 Validate / resolve split

```
validate_tick:
  OCC:     validate_read_locations → abort/reincarnate
  SpecFence:
    valid → done
    invalid ∧ value_stable on failing PE edges → RebindThis (R1a)
    invalid ∧ certified prefix (PCC-journaled) ∧ prefix_skip_beats_b0 → PrefixSkip
    else → B0 OCC reincarnation (clear FF; next exec uses occ_read)
```

Unfenced-only abort (no PE journal) **must** take B0, not PrefixSkip. PrefixSkip without a journaled prefix is the graft that made Unfenced pay rem `first_k` on every read.

### 2.7 Single-iteration land (no P0/P1/P2)

One coherent cut — all required together:

1. Architecture SoT (this file).  
2. OCC hot path: zero SpecFence calls.  
3. SpecFence Unfenced = `occ_read` (same function).  
4. PCC overlay only on PE ∩ ROI.  
5. Learning / process / Edge / rem journal off Unfenced.  
6. Delete residual-Bind / OrderedDirtyRead / FF from Unfenced.  
7. Tests + all-blocks / focus sweeps. Honesty vs **0.436**.  
8. Impl map `lab/notes/specfence-clean-slate-impl.md`.

Partial land (new modules that still call `unfenced_occ_fast`, or OCC that still `record_db_heavy_op`, or Unfenced that still skip-ESTIMATE) is a **non-land**.

---

## 3. Frozen π (held — do not reopen)

```
a = (t, k, depth, ℓ, mode)                    # inc NOT in Avoid key
e_vis = (writer?, published_Data?, edge_kind)
gate  = PredictedEssential(ℓ, k, morph) ∨ independence_certified
verb  = Bind | WaitFor|serial-lane|ordered_admit | Unfenced≡OCC
        # THIS a only — never sticky Wait-on-tx
        # Unfenced≡OCC means occ_read, not a SpecFence fast-path
```

Hybrid: OCC default **per access**; learned timely PCC on predicted-essential Region-accesses. Spec = Region.

---

## 4. Hard bans (unchanged)

All frozen §0 bans hold. Additional **plant** bans from this SoT:

| Ban | Why |
|-----|-----|
| Shared intercept as the Unfenced implementation | graft tax |
| `unfenced_occ_fast` as the Unfenced SoT | still a SpecFence function |
| rem `first_k` / journal / FF / `maybe_note_value` on ¬PE | Unfenced > OCC |
| OrderedDirtyRead on ¬PE | semantic ≠ OCC |
| residual-Bind on ¬PE reincarnation | PCC on Unfenced |
| `note_detect` / `process.record` DashMap on Unfenced | learning on critical path |
| Edge SM on ¬PE | frozen already; restated |
| SpecFence calls on OCC execute/validate/schedule | OCC not pristine |
| New π fields / exclude-set OR-doors | frozen |
| P0/P1/P2 staged land | one cut |

---

## 5. Cost model (updated)

```
OCC_wall ≈ useful_EVM + reincarnation_recovery

SF_graft ≈ useful_EVM
         + Σ_access (mode_branch + rem_k + PE_probe + sampled_DashMap)
         + Σ_access (FF_lookup + OrderedDirtyRead + note_value)
         + Σ_¬PE_reincarnation residual_Bind
         + Σ_PE timely_PCC

SF_clean ≈ useful_EVM
         + Σ_access (1 atomic detect + 1 atomic has_pe [+ bump_k + PE probe if has_pe])
         + Σ_{a : PE ∧ ROI} timely_PCC_tax_on_a
         + Σ_miss OCC_residual_reincarnation
```

**Invariants:**

1. Empty PE table ⇒ SpecFence `storage`/`basic` ≡ OCC `storage`/`basic` (detect atomic only).  
2. ¬PE access ⇒ **same machine code path** as OCC read after the gate (call `occ_read`).  
3. Quiet / META_COLD ⇒ SF_wall ≡ OCC_wall (± detect atomics). Goal: quiet median **≈ 1.0**.  
4. Miss ⇒ B0 OCC reincarnation, not SuffixRepair / SoftWait / ForcePrefix.  
5. One PE access must not sticky-Wait the rest of the tx.

---

## 6. Success metric

**Primary:** median SF/OCC TPS and wall @8 on all-blocks nonempty set.  
**Honesty baseline:** **0.436** (`d3a7ae1` cost-class).  
**Goals (do not claim without JSON):** quiet heuristic median **≈ 1.0**; full-set median **↑ toward OCC** (0.5 then 0.7 then 1.0).  
**Falsifiers:** soft=0, await=0, exclude-set=0, `unfenced_occ_fast` may remain as a **counter alias** for Unfenced-gate hits but the **code path** is `occ_read`; OCC mode detect/pcc_fire/edge = 0; mixed_verb > 0 on fan_out consumers when morph says so.

N=1 OCC pathology (e.g. 19434587) still inflates mean — report nonempty median + quiet/fan_out splits.

---

## 7. Module map (target)

```
crates/pevm/src/
  vm.rs                 # OCC read helpers + thin mode dispatch; no π soup
  pevm.rs               # mode → OCC worker | specfence::executor worker
  scheduler.rs          # Block-STM tasks; wave hooks only via executor
  mv_memory.rs          # shared versions
  specfence/
    access_policy.rs    # NEW — frozen π gate; Unfenced → occ_read
    executor.rs         # NEW — SpecFence ticks + validate/resolve dispatch
    learner.rs          # PE table; note_detect not on Unfenced
    edge.rs             # PE-only SM
    rem.rs              # PE journal / PrefixSkip / bump_k
    process.rs          # PCC + end-tx flush
    metrics.rs          # counters; OCC does not increment hot-path grafts
    …                   # historical modules stay; not Unfenced SoT
```

`vm.rs::maybe_wait_specfence` / `unfenced_occ_fast` **cease to be SoT**. They may remain as wrappers that call `access_policy` + `occ_read` during the cut; the Unfenced body must not grow.

---

## 8. Control loop (clean)

```
begin_block(mode):
  OCC:        MvMemory + Scheduler only
  SpecFence:  seed PE from InterBlockPrior if !quiet
              (no force_prefix, H-OR, canary, morph actuator)

OCC access:
  occ_read(ℓ)                                 # ESTIMATE → Blocking

SpecFence access a:
  detect_coverage += 1
  if !has_any_predicted ∨ ¬PE(ℓ,k) ∨ ¬ROI ∨ quiet∧¬intra:
      occ_read(ℓ)                             # identical helper
  else:
      PCC: Bind | WaitFor | serial-lane       # this a only
      # rem journal / Edge / process / FF only here

SpecFence validate:
  R1a RebindThis if value_stable on PE edges
  R1b PrefixSkip iff PCC-journaled prefix ∧ cheaper than B0
  else B0 (clear FF; OCC reincarnate)

end_block SpecFence:
  learner EMA; mixed_verb flush; falsifiers
```

---

## 9. Correctness

1. Preset-order commit. Seq ≡ par TCB.  
2. OCC safety unchanged (OCC runner is pevm).  
3. SpecFence Unfenced safety = OCC safety (same read).  
4. PCC wrong prediction = performance only (abort / B0).  
5. Hang-freedom: serial-lane or Bind or independent steal; no SoftWait Soft.  
6. No new speculation: Unfenced is OCC; Fence is a barrier on grain.  
7. Detect honesty: coverage atomic on every SpecFence access; missing PE Detect on a **PCC** access is a protocol bug.  
8. Grain honesty: mixed verbs inside one tx expected; exclude-set live OR = protocol bug.

---

## 10. Worked example (597 tx203) — plant, not π

| Tick | Path |
|------|------|
| \(k{=}0..5\) cold | `has_pe` may be true (star class exists); `¬PE(ℓ,k)` → **`occ_read`** |
| \(k{\approx}6\) \(\ell^\star\) | PE ∧ ROI → Bind if Data, else WaitFor(38) / serial-lane |
| \(k{=}7..\) other \(\ell\) | **`occ_read`** (same as OCC) |
| miss on \(k{=}6\) | RebindThis / PrefixSkip from journaled prefix / B0 |
| quiet block | `has_pe=false` → every access `occ_read` after one atomic |

Anti-pattern: `unfenced_occ_fast` + residual-Bind + skip-ESTIMATE on \(k{=}0..5\).

---

## 11. Supersedes (plant only)

| Doc | Role after this SoT |
|-----|---------------------|
| `specfence-complete-architecture-v4-frozen-grain.md` | **π AUTHORITATIVE** — not replaced |
| `specfence-occ-cost-pcc-roi-impl.md` | historical graft attempt; diagnosis absorbed |
| `specfence-frozen-grain-impl.md` | π land map; plant replaced |
| `specfence-v5-first-principles-clean-slate.md` | 2026-09-07 shovel note; **not** this executor split |

---

## 12. Implementation contract

- Write this SoT first in the same PR as the reimplementation.  
- Impl map after land: `lab/notes/specfence-clean-slate-impl.md`.  
- Sweeps: `lab/results/clean-slate-*.json` (gitignored) + `lab/notes/clean-slate-sweep-summary.json`.  
- Honesty (this cut): nonempty all-blocks N=1 median **0.464** vs 0.436; quiet median **0.716** (not ≈1.0). **Do not claim ≥0.7.**
