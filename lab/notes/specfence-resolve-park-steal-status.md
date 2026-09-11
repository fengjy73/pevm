# SpecFence resolve — critical-path park→steal / wake status

**Date:** 2026-09-07 (Asia/Shanghai)  
**Branch:** `specfence`  
**Base tip:** `ade501d` (profile-strip landing)  
**Authority:** `specfence-native-resolve-protocol.md`, `specfence-resolve-profile-strip-status.md`, `lab/results/resolve-profile-597.json`

---

## Mandate

Cut **critical-path park idle** without restoring SoftWait→SpecRead strips / Wait storms / SpecRead-through-writer.

1. Instrument `softwait_park_idle` subtypes  
2. Steal when parked (`ready_steal_on_wait` ↑ vs `wait_park_count`)  
3. Wake latency: publish/status → waiter ready immediately  
4. Hang-free SoftWait resume (SuffixRepair / RewindTo+FF; no FullRestart-from-head when cp exists)  
5. `maybe_wait` meta second (only after park idle moves)

---

## What shipped

### 1. Park subtype instrumentation

`ParkKind` on `PendingPark` / `ParkedWait`:

| Kind | Source | Metrics |
|------|--------|---------|
| `SoftWaitSoft` | FenceGraph SoftWait Soft arm | `park_count_softwait` / `park_ns_softwait` |
| `EarlyAbort` | P3 EarlyAbort Blocking | `park_count_early_abort` / `park_ns_early_abort` |
| `BlockingOther` | Cold/hint WaitHard, ESTIMATE Blocking, k=0 | `park_count_blocking_other` / `park_ns_blocking_other` |

`wait_park_ns` = sum of subtypes. G7 smoke exports the split.

### 2. Park→steal

- After Blocking park: immediate `next_task_steal_after_park` (wave ready first, then one cautious `execution_idx` fetch_add).  
- `next_task_with_wave`: when `steal_after_park_pending`, prefer that path; count Execution steals on wave **or** collaborative Ready (not validation).  
- Re-check wave ready before idle `yield_now`.  
- Avoided tip-CAX / validation-as-steal stampede (regressed wall in an earlier attempt).

### 3. Wake latency

`finish_execution_with_wave_fence`: drain dependents → **set Executed/Validated** → then `set_ready` / `push_ready` / `wake_writer_done` / FenceGraph clear (still under writer lock). SoftWait `is_done` is true before waiters proceed.

### 4. Hang-free resume

Unchanged native path: `try_apply_park_resume` → `try_arm_park_resume_at_k` (RewindTo+FF when cp exists; else FullRetry). Absolute jump stays off.

### 5. maybe_wait meta

No further DashMap strip this PR. Profiled `maybe_wait` fell ~125→~92ms as a side effect of fewer thrashy parks; further Bind-on-Data cold-path collapse remains next if wall stuck.

**Not restored:** SpecRead-through-writer, Bind→SpecRead SoftWait-strip, Wait storms, whole-block inspect.

---

## Park subtype split (597 @8, unprofiled last iter)

| Subtype | parks | idle ms (worker sum) |
|---------|------:|---------------------:|
| SoftWait Soft | 52 | **38.2** |
| EarlyAbort | 0 | 0 |
| BlockingOther | 293 | **148.9** |
| **Total** | **345** | **187.0** |

Baseline profile lump `softwait_park_idle` ≈ **391ms**. Unprofiled last-iter total ≈ **187ms** (~52% down). **BlockingOther owns the remaining park idle**, not SoftWait Soft.

Steal: `ready_steal_on_wait` **196** (baseline profile capture **120**) vs `wait_park_count` 345 (ratio ≈0.57).

---

## Validation

| Check | Result |
|-------|--------|
| `cargo test -p pevm --lib` | **92 passed** (+ `park_kind_splits_idle_ns`) |
| `cargo test -p pevm --test specfence` | **23 passed**, 13 ignored |
| SoftWait 597 median | **52** ≪428 |
| Hang 597/599? | **No** |
| Aborts | median ~254 (schedule-noisy; not storming vs OCC order on fair runs) |

### 597 @8 — N=7 vs `ade501d`

| Metric | ade501d profile | **park-steal unprofiled** | **park-steal profiled** |
|--------|----------------:|--------------------------:|------------------------:|
| wall median | **30.6** | **30.0** | **27.0** |
| wall min | — | **26.6** | **23.3** |
| SoftWait median | 42 | **52** | **51** |
| park idle (last / lump) | 391 | **187** | ~397 (Instant-noisy) |
| maybe_wait ms | 125 | — | **~92** |
| ready_steal | 120 | **196** | **240** |

Stretch &lt;15 **not** met. Park idle no longer solely owns the lump as SoftWait Soft; **BlockingOther** + residual meta remain the gap. Wall clearly down under profiled capture (27.0 vs 30.6); unprofiled median ≈ flat with lower park sum and higher steal.

---

## Artifacts

- `lab/results/resolve-park-steal-597.json` — diagnosis + subtype split  
- `lab/results/resolve-park-steal-sf-occ.json` / `*-smoke7.run.log` — unprofiled N=7  
- `lab/results/resolve-park-steal-prof-sf-occ.json` / `*-prof-smoke7.run.log` — `SPECFENCE_PROFILE=1` N=7  
- `lab/results/resolve-park-steal-flip.json`

## Code

- `crates/pevm/src/specfence/rem.rs` — `ParkKind`, subtype ns, `park_with_kind`  
- `crates/pevm/src/scheduler.rs` — status-before-wake; `next_task_steal_after_park`  
- `crates/pevm/src/pevm.rs` — park kind wiring; immediate post-park steal  
- `crates/pevm/src/vm.rs` — SoftWait / EarlyAbort pending kinds  
- `crates/pevm/src/specfence/metrics.rs` — subtype snapshot fields  
- `crates/pevm/examples/specfence_g7_smoke.rs` — export split  

## Next lever

1. Cut **BlockingOther** idle (ESTIMATE / cold Blocking wake path) without SoftWait storms.  
2. Further collapse `maybe_wait` DashMap on Bind-on-Data / cold SpecRead if wall stuck.  
3. Do **not** restore SpecRead-through-writer.
