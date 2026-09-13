# SpecFence OCC↔PCC fusion — switch-path audit

**Date:** 2026-09-13 (Asia/Shanghai, UTC+8)  
**Tip:** `4a91b5f` (`docs(specfence): parallel-compute impl map + sweep honesty 0.744`)  
**Read-only.** No code rewritten.  
**Plant SoT:** `lab/notes/specfence-parallel-compute-architecture.md`  
**π SoT:** `lab/notes/specfence-complete-architecture-v4-frozen-grain.md`  
**Honesty baseline at this tip:** nonempty median SF/OCC **0.744**; quiet median **1.020**; named fan_out still **0.27–0.53**; quiet tail 2179522 N=1 **0.234**. `pcc_fire_at_a` 3 636 vs `unfenced_occ_fast` 304 527 / `occ_kernel_execs` 63 597 (`specfence-parallel-compute-impl.md`).

**Question:** when does an incarnation become OccKernel vs PccKernel, is that switch *before* a conflicting access, and which produced learning signals actually move `decide()`?

---

## 0. One-paragraph verdict

The live switch is **two-stage and late**. Stage A (`access_policy::decide`) is a PE ∩ ROI gate that **never looks at \(e_{\mathrm{vis}}\)** (writer / published Data / executing). Stage B (`vm.rs::pcc_overlay`) may Bind or WaitFor, but **`mark_pcc` runs first** — a ROI miss still leaves the incarnation on PccKernel. PredictedEssential is planted **after OccKernel abort** (`validate_occ_kernel` → `note_abort_access`, residual \(k{=}1\)) or as inter-block seed that `pcc_makespan_win` **refuses to Fire**. So the first conflicting access of a block (and usually of a tx) stays Unfenced≡OCC; PCC can open only on a *later* access of the same \((\ell,k_{\mathrm{class}})\) after an intra abort, and only if morph is not quiet-fence-off. HotSet / WŜ / sketch H / independence_certified / Bayes are produced and almost unused by `decide()`. Ready/steal is Block-STM + park deque — **not** PE / Fence admission.

---

## 1. Exact state machine

### 1.1 Kernel flags

`kernel.rs::IncarnationKernel` = `{ Occ, Pcc }`. `KernelTable` is one `AtomicU8` per tx, default **OCC**.

| Event | File:fn | Transition | Condition |
|-------|---------|------------|-----------|
| Block construct | `kernel.rs::KernelTable::new` | `* → Occ` | all slots |
| Incarnation start | `kernel.rs::begin_execute` | `→ Pcc` if `repair_armed`, else **`→ Occ` (reset)** | `repair_armed` = rewind resume ∨ FF-head |
| First overlay entry | `kernel.rs::mark_pcc` ← `vm.rs::pcc_overlay` | `Occ → Pcc` | `decide` returned `TryPcc` **and** overlay entered — **before** Bind/WaitFor success |
| Next incarnation | `begin_execute(..., false)` | `Pcc → Occ` | unless repair still armed |
| Mid-incarnation downgrade | — | **none** | no `mark_occ` |

`inc` is not an Avoid key (held). The kernel bit is **per-tx, overwritten at every `begin_execute`**, not per-incarnation-id.

### 1.2 Who calls `begin_execute` (double reset)

1. `vm.rs::VmDb::set_tx` (`~189–192`) — before `get_code_hash`.  
2. `vm.rs::Vm::execute` (`~1984–1994`) — **after** `set_tx` returns.

`execute` therefore **wipes** any `mark_pcc` that fired during `set_tx`’s `get_code_hash` → `maybe_wait` → `pcc_overlay`, unless `repair_armed`. The local `occ_kernel` bool in `execute` is captured **after** this second reset. Later EVM accesses can `mark_pcc` again; that is the upgrade that validate/finalize actually see.

### 1.3 Upgrade / downgrade events (live)

```
begin_execute(t, rewind∨ff_head)  ⇒  PccKernel     # Repair already owns rem
begin_execute(t, else)            ⇒  OccKernel     # default computer

access a = (t, k, depth, ℓ):
  maybe_wait → specfence_access_gate
    empty PE ∨ quiet_fence_off ∧ ¬intra  → UnfencedOcc; kernel unchanged
    PE ∧ ¬pcc_makespan_win               → UnfencedOcc {roi_skip}; kernel unchanged
    PE ∩ ROI → pcc_overlay:
         mark_pcc(t)                     # UPGRADE HERE (even if overlay then Unfences)
         published Data                  → Bind; stay Pcc
         1 executing writer ∧ waitfor_win → WaitFor; stay Pcc
         else                            → occ_unfenced; kernel STAYS Pcc

validate(t):
  Occ mode                         → validate_occ_stage          # bool + B0
  SpecFence ∧ kernel.is_occ(t)     → validate_occ_kernel         # bool + B0 + learn PE
  SpecFence ∧ kernel.is_pcc(t)     → try_validate                # Resolve museum

next incarnation: begin_execute resets to Occ unless Repair still armed.
```

Repair-armed Pcc is **not** a PE Fire. It is a journal-legal resume (PrefixSkip / FF-head). OccKernel abort **clears** repair/FF (`validate_occ_kernel` `clear_repair` / `clear_ff_head`) and does B0 — so Occ abort never starts the next inc as Pcc.

### 1.4 Mode vs kernel

| `ConcurrencyMode` | Kernel table | Access | Validate | Schedule |
|-------------------|--------------|--------|----------|----------|
| `Occ` | unused | `maybe_wait` = `Ok(())` | `validate_occ_stage` | `next_occ_task` (no wave) |
| `Pcc` | unused | `maybe_wait_pcc` account Wait | `try_validate` | wave=None (`wave_for_mode`) |
| `SpecFence` | live | `decide` → Unfenced \| `pcc_overlay` | OccKernel vs PccKernel split | `next_sf_task` + wave |

---

## 2. Timeliness — when does the switch happen?

**At access, but PE is only available after a prior abort (or a prior-seed that cannot Fire).**

| Moment | What runs | Can kernel become Pcc? | Can verb be Bind/WaitFor? |
|--------|-----------|------------------------|---------------------------|
| Block start | `pevm.rs` seed PE from `InterBlockPrior` if `!quiet` | no | no — prior-only is roi_skip |
| Before first access of a cold tx | `begin_execute(false)` | no (Occ) | no |
| At `basic` / `storage` | `specfence_access_gate` → `decide` | yes, if PE ∩ ROI | yes, if overlay Bind/WaitFor |
| Mid-access (ESTIMATE seen) | Unfenced uses OCC ESTIMATE→Blocking | no (unless already Pcc) | no new π |
| At validate fail, OccKernel | `validate_occ_kernel` plants PE, B0 | **no** — this inc stays Occ | no |
| At validate fail, PccKernel | `try_validate` R1a/R1b/B0 | already Pcc; may arm Repair | n/a |
| Next inc start | `begin_execute` | Pcc only if Repair armed | — |

So:

- **Before conflicting access:** only if *this* \((\ell,k_{\mathrm{class}})\) already has **intra** PE *and* `pcc_makespan_win`. That requires a **previous** abort on that class in *this* block (or abort_events already ≥4 breaking quiet). First-wave RAW is never pre-fenced.
- **At the conflicting access:** yes, for a *later* reader/reincarnation whose `bump_k_only` class matches the abort template.
- **Only after abort:** the *first* conflict of a class. `decide` cannot see \(e_{\mathrm{vis}}\); a live executing writer on a cold \(\ell\) stays Unfenced.

Frozen π (`gate = PE ∨ independence_certified`, verb from \(e_{\mathrm{vis}}\)) is **not** what `decide` implements. `independence_certified` is hardcoded `false` on the Bind/WaitFor `DecisionFeat` records (`vm.rs::pcc_bind_published` / `pcc_wait_for_writer`).

---

## 3. Avoid miss — conflict exists, stay Unfenced / OccKernel

Places where a RAW/WAW is live (or just aborted) but π stays Unfenced≡OCC.

| # | Gap | File:fn | Why it misses |
|---|-----|---------|---------------|
| M1 | First-wave / first inc | `access_policy.rs::decide` + `learner.rs::has_any_predicted` | Empty PE table → Unfenced. First conflicting reader always OccKernel. |
| M2 | Prior-only PE | `learner.rs::pcc_makespan_win` (`!predicted_essential_intra` → false); test `pcc_makespan_win_requires_intra_abort_not_prior_seed` | Inter-block seed sets `predicted` but **not** `predicted_intra`. `decide` returns `UnfencedOcc { predicted:true, roi_skip:true }`. |
| M3 | Quiet fence-off | `learner.rs::quiet_fence_off` (morph.quiet≥0.45 ∧ abort_events<4 ∧ park_heat<2); `decide` lines 32–44 and `pcc_makespan_win` first return | Intra abort PE is **demoted**. Default morph is quiet-biased (`quiet: 0.60`). One/two/three aborts do not Fire. Test `quiet_lone_abort_stays_unfenced`. |
| M4 | \(k\)-class mismatch | `edge.rs::access_k_class`; `executor.rs::validate_occ_kernel` `loc_k.or(Some(1))`; `decide(learner, ℓ, bump_k)` | OccKernel Unfenced records **no** Edge / rem `first_k`. Abort plants PE at class of \(k{=}1\) (`1..=3`). Consumer `bump_k_only` at the real first-cross is often \(k{\ge}4\) (class 2+) after `basic` of caller/to. PE miss. |
| M5 | \(e_{\mathrm{vis}}\) not in `decide` | `access_policy.rs::decide` args = `(learner, location, access_k)` only | Published Data / executing writer / residual WŜ **cannot** open PCC without intra PE. Frozen π operand unused at the gate. |
| M6 | WaitFor refuses fleet / non-exec | `learner.rs::waitfor_makespan_win` (`unfinished>1 \|\| !writer_executing` → false); `vm.rs::pcc_overlay` | TryPcc then `occ_unfenced`. Estimate / Ready / Aborting / multi-writer spines stay OCC. |
| M7 | Beneficiary / lazy | `vm.rs::specfence_access_gate` | Skip gate entirely. |
| M8 | Empty-PE fast path | `executor.rs::specfence_plant_is_occ` | `!has_any_predicted` → no `bump_k`, no `decide`. |
| M9 | Writes never gated | `vm.rs::execute` finalize SSTORE | `decide` is read-only (`basic` / `storage`). WAW at publish is Occ unless a later read Fires. |
| M10 | Occ publisher does not Avoid-broadcast | `vm.rs::execute` finalize: `broadcast_avoid` / `wake_on_data_publish` **iff** `kernel.is_pcc` | First-wave Occ writers do not plant Avoid / Data-wake. Later readers cannot Bind from that signal. |
| M11 | `independence_certified` unused | frozen π gate; `sketch.rs::independence_certified`; `decide` never calls it | Cannot Unfence a false PE **or** certify a true independent — observe-only. |

M1+M2+M5 together: **the first conflicting access of a hot \(\ell\) in a new block is definitionally OccKernel**, even when the process prior, HotSet, and a live writer are all present.

---

## 4. False Fire — PCC / PccKernel without ROI

| # | Gap | File:fn | Why it is a false Fire |
|---|-----|---------|------------------------|
| F1 | `mark_pcc` before Bind/WaitFor | `vm.rs::pcc_overlay` `~526–528` then `~566–568` `occ_unfenced` | Kernel upgraded; `pcc_armed` then cleared. Validate sees `is_pcc` → `try_validate` museum (Vec + rem + R1/R2). Finalize writes rem / CallEntry / wake (`~2808–2865`) because `kernel.is_pcc`. **PccKernel tax without a Fire verb.** `record_pcc_kernel_exec` ticks; `pcc_fire_at_a` does not. |
| F2 | Repair-armed Pcc without PE | `kernel.rs::begin_execute(true)`; `vm.rs::execute` `repair_armed` | PrefixSkip/FF resume is Pcc even if this inc never hits PE ∩ ROI. Journal-legal, but not a makespan win. |
| F3 | Intra PE is loc-class sticky | `learner.rs::mark_predicted_essential` | One abort on \((\ell,k_{\mathrm{class}})\) opens TryPcc for **every later tx** touching that class — including independents that would Bind a non-conflicting published tip (`pcc_bind_published` takes **any** `last_data_before`). |
| F4 | Residual \(k{=}1\) over-plants class 1 | `validate_occ_kernel` `loc_k.or(Some(1))` | PE class `1..=3` Fires Bind on early `basic()` of later txs (caller/to), not the abort grain. |
| F5 | Bind records `essential_antidep: true` always | `vm.rs::pcc_bind_published` DecisionFeat | Decision-field “should_fence” is tautological on Fire — cannot score false Fire from that proxy. |
| F6 | `decide` ignores `independence_certified` | `access_policy.rs::decide` | A certified-independent \(\ell\) with leftover intra PE still `TryPcc` → F1 or cheap Bind tax. |

F1 is the protocol-level false Fire: **kernel bit ≠ Fire verb**. Plant SoT says “on first PCC Fire (Bind \| WaitFor)”; the impl upgrades on **overlay entry**.

---

## 5. Learning — produced vs consumed by `decide()`

`decide(learner, location, access_k)` (`access_policy.rs:27–58`) reads **only** `LiveLearner`:

```
has_any_predicted
quiet_fence_off          # morph.dominant_quiet ∧ abort_events<4 ∧ park_heat<2
predicted_essential(ℓ,k) # DashMap (ℓ, access_k_class(k))
predicted_essential_intra(ℓ,k)
pcc_makespan_win(ℓ,k)    # intra ∧ !quiet ∧ park ≯ 1.5·abort+8 ∧ ¬(r1_underfire ∧ park≥4)
```

### 5.1 Produced (this tip)

| Signal | Producer | When | Consumed by `decide()`? | Consumed elsewhere on the switch? |
|--------|----------|------|-------------------------|-----------------------------------|
| PE intra | `learner.rs::note_abort_access` → `mark_predicted_essential` | OccKernel (and Pcc) validate abort, \(k{>}0\) | **yes** (via intra + `pcc_makespan_win`) | — |
| PE prior | `pevm.rs` `seed_predicted_essential` from `InterBlockPrior.top_locations` if `!quiet` | block start | **as predicted=true only** → roi_skip | sketch templates (`seed_from_prior_morph`) |
| PE emptiness | `predicted_n` | seed / abort | **yes** (fast Unfenced) | `specfence_plant_is_occ` |
| Morph / quiet | `begin_block(prior_morph)`; abort/observe bumps | block + intra | **yes** (`quiet_fence_off`) | engagement mode label only |
| Park heat | `learner.rs::note_park_heat` | `fence_wait_for` park | **yes** (quiet + makespan) | `prefer_admit_heat` (not `decide`) |
| Abort \(k\) / dominant_k | `note_abort_access` `abort_k_*` | abort | **only via class of planted PE** | `sketch.mark_access_class` on Pcc finalize |
| HotSet H | `hotset.rs::note_writer` / `note_abort` | **both** kernels finalize + Occ abort | **no** | `maybe_early_val` (research/lean-off); `choose_resolve` retired |
| WŜ / RŜ | `prior.rs::observe_write_set` | both kernels finalize | **no** | `bind_on_data_lite` metric `prior_bind_hit`; retired EV |
| Sketch H / Avoid / residual / canary / `independence_certified` | `sketch.rs::seed_from_prior_morph`, `broadcast_avoid` (Pcc finalize only), `mark_access_class` | block start + Pcc finalize + abort | **no** | `fence_wait_for` leftovers; `pcc_overlay` does **not** call `choose_edge_action` |
| Bayes \(P_{\mathrm{conflict}}\) | `validate_occ_kernel` `observe_conflict_location_always` | Occ abort | **no** | retired `choose_resolve`; PCC-legacy seed |
| DecisionField | `process.record_decision` | Bind/WaitFor only | **no** (observe) | lab snapshot |
| Cross-block top-\(\ell\) | `learner.rs::pack_top_locations` → `InterBlockPrior::end_block` | end_block | **only as prior PE → roi_skip** | sketch H/templates if `!quiet` |
| Detect `last_k` | `note_detect` (off Unfenced hot path) | PCC / sampled | **no** (must not plant PE; test held) | pack_top `k_template=0` if abort_k_n=0 |

### 5.2 Consequence

The switch is **abort-class PE + morph quiet + park/abort ROI**. Everything else (HotSet, WŜ, sketch, Bayes, \(e_{\mathrm{vis}}\), independence) is **observe / museum / leftover overlay**. Cross-block learning **cannot Fire PCC**; it can only (a) make `has_any_predicted` true so every access pays `bump_k` + PE probe, and (b) roi_skip. That is a **tax without a verb**.

`waitfor_makespan_win` is consumed in `pcc_overlay`, not `decide`. A `TryPcc` that fails WaitFor still paid `mark_pcc` (F1).

---

## 6. Parallel-compute schedule vs CC

Plant (`specfence-parallel-compute-architecture.md` §2.3):

```
ready = { Execute(t) | Unfenced ∨ PE producers published ∨ serial-lane token }
      ∪ { Validate | Executed }
      ∪ { Repair }
steal: useful_EVM independents first; never SoftWait Soft
```

Live (`executor.rs::next_sf_task` → `scheduler.rs::next_task_with_wave`):

| Plant object | Live | Uses Fence / Wait / PE? |
|--------------|------|-------------------------|
| Ready set | `WaveParkTable` deque + Block-STM `execution_idx` / `validation_idx` | **No PE.** Ready = park-wake + OCC collaborative indices |
| Steal | `next_task_steal_after_park_prefer`: wave `pop_ready` then **one** `execution_idx.fetch_add` | After WaitFor `Blocking` only (`pevm.rs::try_execute`). Not PE-gated |
| Pipeline validate | Still **validation-first** when `validation_idx < execution_idx` | OccKernel validate is cheap; stampede unchanged |
| Serial-lane / ordered-admit | `sketch.in_serial_lane` / `admit_spine` | `admit_spine` only inside `pcc_wait_for_writer` / `fence_wait_for`. **`next_task_with_wave` never consults it** |
| FenceGraph | `fence_for_mode` → `finish_execution_with_wave_fence` | Clears SoftWait on finish. Soft=0 → wake is a no-op for π. `wake_on_data_publish` **PccKernel finalize only** |
| WaitFor park | `ReadError::Blocking` → `add_dependency` + `wave.park_with_kind` | Yes — **after** a Fire. Independents steal via wave/OCC, not via PE unpublished-RAW filter |
| Hinted account Wait | `hinted_wait_enabled` = PCC-legacy only | SpecFence `should_wait_account` = false |

**Fence/Wait signals do not admit or refuse `Execute(t)`.** A tx with unpublished PE-RAW into it is still fetched by `execution_idx`. PCC WaitFor is a **mid-execute park**, not a schedule-stage fence. OccKernel conflicts use OCC ESTIMATE→Blocking (same Block-STM dependency), not PE serial-lane.

Wave is a **park-steal graft** on OCC two-counters (`parallel-compute-impl.md` “Not claimed: work-stealing deques”). That matches the 0.744 honesty leftover: idle + validation-first, not missing Wait verbs on quiet.

---

## 7. Concrete “should switch here” gaps (file:fn)

These are switch-path holes vs frozen π + plant SoT, not new π fields.

1. **`access_policy.rs::decide` should take \(e_{\mathrm{vis}}\)** (writer?, published_Data?, kind). Today a live executing writer on \(\ell\) with empty/prior-only PE stays Unfenced. First-cross Avoid is the missing verb.

2. **`learner.rs::pcc_makespan_win` should treat a *high-confidence inter-block* PE as Fire-eligible** (or a weaker Bind-only ROI), not require intra abort. `pevm.rs` seed + `seed_predicted_essential` is dead for verbs.

3. **`executor.rs::validate_occ_kernel` should plant PE at the consumer’s actual access class**, not `loc_k.or(Some(1))`. Unfenced must leave a cheap \(k\) (or hash of first invalid origin) that `decide` will see on reincarnation. Pair with `rem.rs::bump_k_only` / `access_k_class`.

4. **`vm.rs::pcc_overlay` must `mark_pcc` only after Bind or WaitFor actually Fires.** ROI skip (`occ_unfenced` at `~567–568`) must leave OccKernel so validate stays `validate_occ_kernel`. Plant §3.3.

5. **`vm.rs::pcc_overlay` WaitFor miss (`waitfor_makespan_win` false) should be Bind-residual / serial-lane / Unfenced-without-upgrade**, not `mark_pcc` + Unfenced. Multi-writer / Ready writer is the fan_out spine case.

6. **`scheduler.rs::next_task_with_wave` should refuse `Execute(t)` when an unpublished PE-RAW into \(t\) exists** (or require serial-lane token), and prefer independent Unfenced. Today PE never touches admission.

7. **`vm.rs::execute` finalize should first-wave `broadcast_avoid` / Data-wake on OccKernel publish of a PE \(\ell\)** (or at least of a just-aborted class). Occ publishers are invisible to later PCC.

8. **`vm.rs::Vm::execute` must not `begin_execute` after `set_tx`** (or `set_tx` must not `begin_execute`). Double reset drops PCC Fire on `get_code_hash`.

9. **`learner.rs::quiet_fence_off` as a hard Fire ban for intra PE** delays switch until 4 aborts on quiet-labeled blocks. Morph default `quiet: 0.60` makes this the common case at block start — including mixed blocks that have not flipped yet.

10. **`decide` should consult `independence_certified` (Unfence) and must not Fire on class-1 residual Basic reads** (F3/F4). `sketch.rs::access_class_predicted` / `independence_certified` exist and are unused at the gate.

---

## 8. Top 8 gaps by TPS-impact hypothesis

Ranked for **nonempty median / named fan_out / quiet tail** at this tip (0.744 / 0.27–0.53 / 2179522=0.234). Hypothesis, not measured A/B.

| Rank | Gap | Why TPS | Mechanism |
|-----:|-----|---------|-----------|
| 1 | **First-wave switch is after abort only** (M1+M5; §7.1) | Fan_out 14689597-class: every first RAW pays full OCC B0 + ESTIMATE cascade. This is the dominant remaining SF≪OCC on named spines (`impl` 0.27–0.53). | `decide` has no \(e_{\mathrm{vis}}\); PE empty at first-cross. |
| 2 | **OccKernel abort plants PE at \(k{=}1\)** (M4; §7.3) | Reincarnation / sibling still Unfenced at the real first-cross (\(k{\approx}6\), class 2). Abort learning does not hit `decide`. | `validate_occ_kernel` residual \(k{=}1\); no Edge/`first_k` on Unfenced. |
| 3 | **Cross-block PE never Fires** (M2; §7.2) | Every block re-pays first-wave tax on the same token \(\ell\). Inter-block prior is roi_skip + `bump_k` tax. | `pcc_makespan_win` requires intra. |
| 4 | **`mark_pcc` before Fire** (F1; §7.4) | Quiet/mixed: TryPcc → no exec writer → Unfenced **and** PccKernel `try_validate`+rem. Meta on useful_EVM. Matches 2179522 “bind=0 but SF≪OCC” *if* a lone PE probe upgraded kernels; also fan_out Bind-residual tax. | `pcc_overlay` upgrade-then-maybe-Unfence. |
| 5 | **WaitFor only if unfinished==1 ∧ executing** (M6; §7.5) | Multi-writer / Estimate / Ready spines stay OCC → abort storm. Fan_out + WAW. | `waitfor_makespan_win`; no serial-lane admit. |
| 6 | **Schedule ignores PE / Fence** (§6; §7.6) | Validation-first stampede + execute of doomed consumers. Idle + cascade. Plant leftover #3. | `next_task_with_wave` is OCC indices + park deque. |
| 7 | **Quiet fence-off + default morph quiet** (M3; §7.9) | First 1–3 aborts on a quiet-labeled (or not-yet-flipped) block cannot Fire. Intra PE wasted; next accesses still B0. | `quiet_fence_off` in both `decide` and `pcc_makespan_win`. |
| 8 | **OccKernel publish does not Avoid / Data-wake** (M10; §7.7) | Even after PE exists, Occ publishers do not wake WaitFor or broadcast Avoid. Late readers still miss Bind-at-\(a\). | finalize rem/wake gated on `kernel.is_pcc`. |

**Not in top 8 (lower TPS or already held):** SoftWait Soft=0 (held); journal-less RebindThis on OccKernel (held — `validate_occ_kernel` is B0); HotSet-as-Wait-OR (correctly unused); decision_field (observe); PCC-legacy account Wait.

---

## 9. Control-loop map (live, this tip)

```
end_block:  pack_top + morph_hat → InterBlockPrior
            hotset.end_block; sketch.decay_warm_failures
begin_block:
            KernelTable = Occ
            if !quiet: seed PE + sketch H/templates     # PE cannot Fire
            if quiet:  revoke_prior_fences
access:
            detect atomic
            if !has_pe: occ_unfenced                    # OccKernel
            else decide(PE, quiet, intra, makespan)
                 Unfenced → occ_unfenced
                 TryPcc   → mark_pcc; Bind | WaitFor | occ_unfenced
validate:
            OccKernel → bool + B0 + note_abort(k=1)     # PE for NEXT access
            PccKernel → try_validate museum
schedule:
            next_sf_task = wave ready ∪ OCC validation-first ∪ execution_idx
            WaitFor → Blocking + park + steal (not PE admit)
```

---

## 10. Citations (primary)

| Topic | Path |
|-------|------|
| Kernel SM | `crates/pevm/src/specfence/kernel.rs` `begin_execute` / `mark_pcc` |
| Gate | `crates/pevm/src/specfence/access_policy.rs` `decide` |
| Occ validate + PE plant | `crates/pevm/src/specfence/executor.rs` `validate_occ_kernel` |
| ROI | `crates/pevm/src/specfence/learner.rs` `pcc_makespan_win` / `quiet_fence_off` / `note_abort_access` |
| Overlay + upgrade | `crates/pevm/src/vm.rs` `specfence_access_gate` / `pcc_overlay` / `execute` finalize |
| Dispatch | `crates/pevm/src/pevm.rs` worker validate split; seed PE `~450–459`; `pack_top` `~643–644` |
| Schedule | `crates/pevm/src/scheduler.rs` `next_task_with_wave`; `executor.rs` `next_sf_task` |
| Unused-at-gate | `hotset.rs`, `prior.rs`, `sketch.rs`, `decision_field.rs` |

---

*Audit only. Next land, if any, should fix F1 (`mark_pcc` after Fire) and M4 (\(k\) identity on Occ abort) before reopening \(e_{\mathrm{vis}}\) at `decide` — those two are protocol-bugs relative to the plant SoT, not new π.*
