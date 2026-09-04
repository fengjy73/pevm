# Plant v2 M2 status — Wave park / steal (L2)

**Date:** 2026-09-04 (Asia/Shanghai)  
**Branch:** `specfence` @ fengjy73/pevm  
**Scope:** SpecFence path only; OCC/PCC scheduling unchanged (wave deque unused)

## Design

| Step | Behavior |
|------|----------|
| **WaitHard** | `maybe_wait` returns `Blocking(writer)` after `wave.set_pending_park_location(ℓ)` — **no spin** inside `Vm::execute`. |
| **Park** | `try_execute` → `wave.park(waiter, writer, ℓ)` + `scheduler.add_dependency` → tx `Aborting`. Worker returns `None`. |
| **Steal** | `next_task_with_wave` pops **lowest TxIdx** from ready deque, else collaborative `execution_idx`/`validation_idx`. |
| **Wake** | Writer `finish_execution_with_wave` → dependents `ReadyToExecute` + `push_ready`; `wake_writer_done` clears location waiters + `wait_park_ns`. |
| **Soft edges** | Bayes/dag soft waits only `push_ready` reorder within ready set; not correctness TCB. |

### Grain (honest)

PEVM tasks remain **whole-tx**. Park is **tx-level Blocking** (Block-STM style), not a mid-effect live Interpreter continuation. Resume = new incarnation from tx head / M1 RewindTo force-bind. This still satisfies L2 “worker must not occupy a core spinning on WaitHard”.

## Files

- `crates/pevm/src/specfence/rem.rs` — `WaveParkTable` (ready min-heap, waiters by ℓ/writer)
- `crates/pevm/src/scheduler.rs` — `next_task_with_wave`, `finish_execution_with_wave`
- `crates/pevm/src/pevm.rs` — SpecFence Blocking park + steal loop
- `crates/pevm/src/vm.rs` — pending park location on WaitHard/Bind-wait
- `crates/pevm/src/specfence/metrics.rs` — M2 counters on `SpecFenceMetrics`
- `crates/pevm/tests/specfence.rs` — `specfence_m2_wait_hard_parks_and_steals`

## Metrics

| Counter | Meaning |
|---------|---------|
| `wait_park_count` | Times WaitHard/Blocking registered a park |
| `wait_park_ns` | Best-effort parked duration until writer wake |
| `ready_steal_on_wait` | Worker stole another ready tx immediately after park |
| `wave_width_mean` | Mean ready-queue depth at park/push samples |

## Gaps vs full wave REM

1. No mid-effect / live Interpreter park (rayon steal incompatible without serialize+FF).
2. Ready nodes are txs, not `(t,k)` region continuations.
3. Soft-edge reorder is `push_ready` only — no full Ĝ critical-path scheduler.
4. `PublishWrite` wake is approximated by writer `finish_execution` (incarnation publish), not per-effect mid-tx publish.
5. M1b true journal fast-forward **not** in this milestone.

## Success gate

- `cargo test -p pevm --release --config 'profile.release.lto=false' --test specfence -- --test-threads=1` passes.
- WaitHard does not block a rayon worker inside execute (returns Blocking → park → steal).
