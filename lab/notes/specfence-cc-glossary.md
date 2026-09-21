# SpecFence concurrency-control glossary

**Status:** AUTHORITATIVE for live plant language (code, metrics, comments, honesty/impl notes).  
**Date:** 2026-09-14  
**Scope:** rename only. Protocol, Soft=0, and seq≡par are unchanged.

Historical lab notes and sweep JSON may still use banned nicknames. Those artifacts are frozen evidence. New code and new notes must use the names below. When an old note is the current honesty/impl plant, rewrite it in this vocabulary (old names may appear once, in parentheses, as a migration aid).

Product name **SpecFence** and crate path `specfence/` stay. SoftWait Soft stays named only as the **ban** (`soft_wait_arms = 0`); do not revive it.

## Canonical map

| Banned / old plant nickname | Live name | Role |
|-----------------------------|-----------|------|
| Pin / PinHold / PinWithoutThrow | `wait_for_dependency` / `WaitForDependency` (alias: BlockingWait) | Park a consumer behind an unfinished producer **without** Aborting / incarnation++. Wake is same-incarnation Ready. |
| R1 | `partial_abort` | Resolve that repairs a certified prefix instead of throwing the whole tx. |
| R1a | `PartialAbortRebind` | Value-stable rebind of invalid reads; no rewind. |
| R1b | `PartialAbortRewind` | Strip-covered fail → one RewindTo of the uncertified suffix. |
| FullRetry / FullRestart / B0 | `full_abort_reexecute` / `FullAbortReexecute` | Whole-tx abort and re-execute from head (OCC-identical when no certified prefix). |
| Unfenced / Mode Spec / SpecRead (verb) | `optimistic_read` / `OptimisticRead` | OCC-cost read: no wait, no ordered admit. Compiles to the shared OCC MV walk. |
| Fence (verb nickname) | `pessimistic_admit` | Admit only when a predicted essential anti-dependency is visible. Not the product name. |
| Bind (verb nickname) | `ordered_admit` / `OrderedAdmit` | Install a published producer version for this access. Rare; EV-gated. |
| schedule_refuse | `refuse_admit` / `dependency_aware_admission` | Do not admit a known consumer while its producer is still Ready or Executing. |
| waitfor_pin | `wait_for_dependency` | Metric: WaitForDependency parks. |
| waitfor_aborting | `wait_for_full_abort` | Metric: WaitFor that still took AbortingThrow (should stay rare). |
| r1_win / r1_attempt | `partial_abort_win` / `partial_abort_attempt` | Partial-abort success vs arm attempt. |
| SoftWait Soft | **banned (Soft=0)** | Must stay 0. Name kept only so the ban is auditable. |

## Verbs (π / decide)

Live access verbs after PredictedEssential / visibility:

| Verb | Meaning | Must not be confused with |
|------|---------|---------------------------|
| `OptimisticRead` | OCC-cost proceed | Not “Spec = Region”. Region is the control unit (`EdgeKey`). |
| `WaitFor` / `WaitForDependency` | Blocking wait on one unfinished producer | Not AbortingThrow. Not SoftWait Soft. |
| `OrderedAdmit` | Read the published producer version | Not Bind-after-Done theater. Not a Wait door. |
| `SerialLane` | Ordered admit / wait on a multi-writer PE class | Not a HotSet OR-door. |
| `refuse_admit` | Scheduler keeps the consumer out of ready | Mid-tx WaitFor is the fallback, not the first Avoid. |

`RegionMode::Speculate` / Bayesian “speculate vs wait” stay. Those are standard OCC/PCC words, not the banned Mode Spec nickname.

## Repair outcomes

| Outcome | Meaning |
|---------|---------|
| `PartialAbortRebind` | Invalid reads rebound to a value-stable published version. Incarnation stays. |
| `PartialAbortRewind` | Certified prefix kept; uncertified suffix rewound once (`suffix_repair_depth` gates a second train). |
| `FullAbortReexecute` | No honest prefix, or partial abort cannot arm → head re-execute (incarnation++). |

`PartialRetry` (the rem table) is the implementation surface for these outcomes. It is not a user-facing verb.

`RepairGrain::{PartialAbort, PartialAbortSelective, FullAbortReexecute}` classifies a validate fail before those outcomes fire.

## Metrics (hot-path names)

| Field | Counts |
|-------|--------|
| `wait_for_dependency` | WaitForDependency parks (no Aborting). |
| `wait_for_full_abort` | WaitFor that still converted to AbortingThrow. |
| `refuse_admit` | Dependency-aware admission refusals. |
| `partial_abort_win` | Partial abort that committed (rebind or rewind). |
| `partial_abort_attempt` | PartialAbortRewind arms (not theater: increment only when RewindTo arms). |
| `tx_full_abort_reexecute` | Whole-tx full-abort re-executes. |
| `full_abort_reexecute` | Full-abort decisions (OCC abort reexec or SpecFence full abort). |
| `park_resume_full_abort_reexecute` | Wait wake that fell back to full abort. |
| `prefix_skip_roi_full_abort` | Certified prefix existed but PrefixSkip lost to full abort. |
| `optimistic_read_count` / `edge_optimistic_read` / `optimistic_read_occ_fast` | Optimistic-read path. |
| `ordered_admit_hits` / `edge_ordered_admit` | Ordered-admit hits. |
| `soft_wait_arms` | SoftWait Soft arms. **Must be 0.** |

Sweep JSON writers emit these keys. Historical `lab/results/*.json` and frozen catalogs keep old keys.

## Process reasons

`ProcessReason` JSON keys (`as_str`):

| Old | New |
|-----|-----|
| `bind_published` | `ordered_admit_published` |
| `unfenced_*` | `optimistic_read_*` |
| `wait_for_*` | unchanged (already CC vocabulary) |

Snapshot fields: `bind_total` → `ordered_admit_total`; `unfenced_*` → `optimistic_read_*`.

## Comments and notes

- Say **wait for dependency**, **partial abort**, **full abort re-execute**, **optimistic read**, **pessimistic admit**, **ordered admit**, **refuse admit**.
- Do not write Pin, R1, B0, Unfenced, Mode Spec, or Bind as the primary term in new comments.
- SoftWait Soft may appear only as “Soft=0 / not armed”.
- Research-milestone headings like “R0 LeanOCC / R1 HotSet” in old tests are workstream IDs, not the partial-abort protocol. Do not rewrite those as `partial_abort`.

## SF-PS first-class objects (2026-09-21)

| Term | Meaning |
|------|---------|
| `RunnableSet` | Detect-driven ready set: AntiChain(independent) ∪ Released(dependents). SpecFence pick root. |
| `VisibilityPolicy` | `Opt` \| `WaitReleased` \| `OrderedTip`. Opt on an independent tx is Avoid=noop, **not** `ConcurrencyMode::Occ`. |
| `ResolvePlan` | `Commit` \| `PartialAbortRebind` \| `PartialAbortRewind` \| `OrderedReplay` \| `FullReplay`. Validate output on the SpecFence spine. |
| `Schedule.pick` | SpecFence main pick over RunnableSet. Must not call `next_occ_task`. |

`skip_ungated_*` is a leftover compat name for Avoid=noop Opt, **not** an architecture lever or Learn target.

## Invariants this rename does not change

- Soft=0 (`soft_wait_arms == 0`).
- WaitForDependency does not Aborting-convert or increment incarnation.
- PartialAbortRewind is one RewindTo; a second strip-cover without progress is honest full abort.
- Ordered admit requires published conflict-tip ∧ EV; Done producer is optimistic read (`cert=false`).
- seq≡par. No protocol redesign.
