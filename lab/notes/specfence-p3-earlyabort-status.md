# SpecFence P3 EarlyAbort fence — status

**Date:** 2026-09-07 (Asia/Shanghai)  
**Branch:** `specfence`  
**Authority:** `specfence-region-fence-adaptive-architecture.md` §P3, learn-from-blocks §4.1  
**Depends on:** P0–P2 (`8c875c4` tip at start)

---

## Goal

When π says the EarlyAbort niche — **heavy ∧ known `d ≤ D_EARLY` ∧ program ∧ unresolved producer** — **cut the incarnation** at first-cross instead of WaitHard SoftWait+park, using existing rem RewindTo / FullRetry / ESTIMATE paths, hang-free, seq≡par safe.

Block evidence: 14689597 heavy txs with \(d \lesssim 0.15\) at first program cross should EarlyAbort rather than WaitHard.

---

## What landed

### 1. `ResolveAction::EarlyAbort` + π arm

- `choose_action`: inside the WaitHard niche (`want_wait`), return `EarlyAbort` when `early_abort_candidate(ctx)`.
- `early_abort_candidate`: `tx_heavy_hint ∧ is_program ∧ writer_known ∧ !writer_done ∧ gross_work_depth.map(|d| d ≤ D_EARLY).unwrap_or(false)`.
- **Depth rule (no unsafe proxy):** if `gross_work_depth` is `None` (LeanOCC / no inspect), **never** EarlyAbort — keep WaitHard/SpecRead.  
  Documented rejection of `gas_used/gas_limit` as proxy: `used/limit ≤ used/tx_gas_used`, so proxy ≤ `D_EARLY` does **not** imply true gross-work \(d\) ≤ `D_EARLY` (false-positive EarlyAbort).

### 2. VM / scheduler path (`VmDb::maybe_wait`)

On `ResolveAction::EarlyAbort`:

1. Learn densify: Bayes conflict, HotSet `note_abort`, learner `note_abort`, promote mirror.
2. **`PartialRetryTable::arm_early_abort`**: RewindTo + journal FF when a real checkpoint (`cp.k > 0`) exists; else `RepairPlan::FullRestart`; always `set_force_bind(certified prefix)`.
3. **Hang-free sync:** `ReadError::Blocking(writer)` via existing `add_dependency` / WavePark (worker steals).  
   **Does not** `FenceGraph::arm_soft` — EarlyAbort is an **alternate** fence to SoftWait (P4 `(t,k)` park stays on WaitHard SoftWait only).
4. If writer raced to done: `InconsistentRead` → Retry (no SpecRead of the stale cross).

Production LeanOCC still passes `gross_work_depth: None` into `choose_resolve` → EarlyAbort never fires on default path (WaitHard unchanged). Unit / research PolicyCtx with known `d` arms the path.

### 3. Metrics

- `SpecFenceMetrics::early_abort_count` (+ `record_early_abort`), counted in `choose_resolve`.

### 4. P4 coordination (same tip)

Parallel P4 SoftWait `(t,k)` park landed in the same push (`lab/notes/specfence-p4-tk-park-status.md`): `WaveParkTable` / `FenceGraph` gain `set_pending_park`, `wake_on_publish_arms`, `try_arm_park_resume_at_k`. EarlyAbort deliberately **does not** SoftWait-arm so P4 resume-at-`k` is not entangled with EarlyAbort’s rem `arm_early_abort`. FenceGraph SoftWait API preserved for WaitHard arms.

---

## Tests

```
cargo test -p pevm --lib
# 63 passed

cargo test -p pevm --test specfence --test raw_transfers --test small_blocks --test mixed
# specfence: 23 passed, 13 ignored (M1* research; +p4 tk park)
# raw_transfers: 8 passed
# small_blocks: 2 passed
# mixed: 1 passed
```

New/updated `resolve::tests`:

| Test | Expectation |
|------|-------------|
| `early_abort_candidate_arms_early_abort` | heavy ∧ d=0.10 ∧ program ∧ unresolved → `EarlyAbort` |
| `early_abort_requires_known_depth` | d=`None` → WaitHard (not EarlyAbort) |
| `early_abort_not_when_late_depth` | d=0.90 → WaitHard |
| `early_abort_not_when_not_heavy` | !heavy → WaitHard |
| `early_abort_not_when_writer_done` | writer_done → not EarlyAbort |

Non-niche matrix (Bind / handler SpecRead / WAW / fanout WaitHard) unchanged.

---

## Hard constraints

- seq≡par TCB unchanged (integration green)
- Default LeanOCC: no inspect tax; EarlyAbort inert without known `d`
- Beneficiary / `basic_lazy` untouched (still never SoftWait / EarlyAbort)
- Learning ∉ TCB
- FenceGraph SoftWait API preserved for P4

---

## Out of scope / blockers

- **No production EarlyAbort without known `d`:** feeding true gross-work mid-tx needs final `tx_gas_used` or a proven-safe plant signal — not done here.
- **Offline L3** recalibration of `D_EARLY` / wasteΔ on 597 minority — measurement follow-up.
- pevm-specfence-server: **not touched**.

---

## Key APIs

| API | Role |
|-----|------|
| `ResolveAction::EarlyAbort` | π alternate to WaitHard |
| `early_abort_candidate` / `choose_action` | sole π choke point |
| `PartialRetryTable::arm_early_abort` | RewindTo / FullRestart + force_bind |
| `VmDb::maybe_wait` EarlyAbort arm | Blocking hang-free cut |
