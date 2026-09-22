# SpecFence region-learn land v2

**Date:** 2026-09-22
**Branch:** `cursor/specfence-region-learn-dag-51e8` (continues PR #46; no new PR)
**Prior land:** `lab/notes/specfence-region-learn-dag-land-v1.md` (`c3475a1`)
**Co-author:** `0xstride <fengjy73@users.noreply.github.com>`

Bar is **TPS SF/OCC ≥ 1.5 on both** focus blocks (primary = OCC wall / SF wall). **Not met.** This cut adds no execution path.

## Four-class status

Status after attempting the v1 next cut (suspended frame at the protected short-region read). The v1 read still releases only on `is_validated`.

| Class | 3356896 | 15274915 | Root if unsolved |
| RAW | yes (spine) | partial | Large RAW is secondary; no access-grain true-Data arm is the acceptance driver |
| WAW | **no** | **no** | The short region waits for a validated pred, and that wait still leaves `Database::basic`. A full=0 thin shell (best 1.491 ms) is already slower than OCC/1.5 (~0.92 ms) |
| WAR | **no** | **no** | The WAR counter fires when a later writer is in the learned list. There is no reader-reserve and no WAR edge list |
| Chain | partial | **no** | Thin wait is one hop at the read; workers are not pinned by an off-queue plant. Large L=77 is still the ≥32 crit hold, and the wall stays above OCC/1.5 |

## Ideal-DAG Diff

Bounds are the ideal-DAG note (LB = L_crit). Stubs are unchanged: `lab/notes/dag-3356896.json`, `lab/notes/dag-15274915.json` (`known_from_notes_only: true`). No new edges were emitted. No invented RAW/WAR endpoints.

### 3356896

Ideal: WAW spine on basic `dff71d59d972`, writers `4, 31, 66, 67, 69, 70, 93, 96, 103, 115, 131, 132, 135, 138, 141, 166, 171`, earliest k = 5 or 6, RAW = 0 on that spine, WAR = 18 (endpoints unknown), antichain proxy ~153, LB = 0.031 ms.

| | |
|---|---|
| Recognized | Reuse `region_avoid` on `0xdff71d59d972d654`, **n=17, head tx 4, tail tx 171**. Same ℓ and the same endpoints as the note. |
| Missed txs | Cold and the first reuse often see a **partial** writer list (observed n=9, head 69). Scope floor is 12, so that partial list is not armed. Those hops still Opt. |
| Missed edges | The 16 adjacent WAW hops are not installed until a snapshot contains the full 17. WAR=18 endpoints are still unnamed. |
| Wrong Avoid | Off-queue `plant_nearest_preds` on the 17 stays dropped (reuse TPS **0.53 / 0.43**). This cut does not plant it again. |
| Same-frame Avoid | **Not installed.** The successor’s prefix dies when `basic` returns `Blocking`. Resume in this tree is a later `Vm::execute`. |

### 15274915

Ideal: WAW hop on basic `abd6bb397881`, L=77, earliest k=3, RAW=35, WAR=107, work-weighted path tx 0 → tx 102 (13 txs), LB=1.19 ms, antichain proxy ~1111. Full writer list is not in the notes and was not emitted.

| | |
|---|---|
| Recognized | The ≥32 crit spine is still the existing hold. The short-region slot does not replace it. |
| Missed txs / edges | `abd6bb…` writers, RAW=35, and WAR=107 are not in a regenerated edge list. Later touchers are not region-armed before the first conflict. |
| Wrong Avoid | Length-8 side location `0x483a65c8273c8219` stays excluded (floor 12). It is not planted. |
| Same-frame Avoid | **Not installed.** Focus read k=3 is below the ResumeAtK prefix bar (`k < 8` returns 0), and that path is a new execute anyway. |

## Suspended frame: revm cannot keep this read

The v1 next cut asked for one thing: while the learned one-hop pred is still executing, return the worker to antichain work and keep the successor’s prefix, then continue **that same interpreter frame** once the pred is `is_validated`. A new `execute` from k=0 does not count.

revm’s `Database::basic` is a synchronous callback on the live interpreter (`vm.rs` `basic`, then `consult_ungated_wait_once` → `wait_protected_commit`). The protected path returns `Err(ReadError::Blocking(pred))`. That error leaves `Vm::execute`. PC, stack, memory, and the journal do not remain as a resumable frame.

The worker has one `Evm`. The next antichain transaction enters `execute`, which calls `set_tx` and `ctx.journal_mut().clear()` (`vm.rs` ~3296–3314) before `Handler` runs. There is no second interpreter to hold the successor while that worker runs another tx.

What the tree calls resume is a **later** `Vm::execute`:

- `arm_wait_for_dependency_checkpoint` (`rem.rs`) returns 0 when the snapped prefix has `st.k < 8`, because a tiny ResumeAtK pays more than an OCC full abort. The focus reads sit at k=5 or 6 (3356896) and k=3 (15274915), so the product path does not arm ResumeAtK. The v1 protected park sets `armed_at_k = 0` and `try_execute_sf` treats that as `cheap_defer`: `add_wait_for_dependency`, then a fresh `execute` after wake.
- Absolute jump (`boundary.rs`) snapshots PC/stack/memory with a stock Inspector and applies the snap on a **later** `inspect_run`. It is off unless `SPECFENCE_ENABLE_INSPECT=1` or `SPECFENCE_ABSOLUTE_JUMP=1`. The execute path records that a Lean absolute jump broke `seq≡par` (empty memory) and that production JUMP/SNAP stay off.

Enabling inspect, ResumeAtK, or another park kind would be a new execute plus a rem/jump strip. Those levers are already discarded (thin rewind, absolute jump, aborting park / wake-from-k=0 as the win path). This cut does not add one.

Release on the existing protected read stays `is_validated`. `is_done` still only covers the short validation spin inside the same `basic` call. `estimate_block` does not gate this path. `next_task*` is not restored. The spine is not planted off the queue.

A later change can claim a same-frame suspend only by showing an interpreter object that outlives `basic`’s `Blocking` return and survives another tx’s `set_tx` / `journal.clear()` on this worker without a new `Handler::run`.

## Soft=0 Instant-off N=5

No new binary. Numbers are the v1 final binary on this host (`nproc` = 4, harness `SPECFENCE_COMPARE_CORES=8`, `SPECFENCE_COMPARE_CHECK=1`, N=5, interleaved OCC/SF, primary = reuse median OCC/SF). `seq=par` on both. SpecFence `occ_schedule_picks=0`, `soft_wait_arms=0`, `explore=0`.

### Sample A (reported)

| Block | OCC ms | SF cold | SF reuse | TPS | vs ≥1.5 |
|------:|-------:|--------:|---------:|----:|:-------:|
| 3356896 | 1.380 | 2.187 | **2.202** | **0.63** | unmet |
| 15274915 | 5.834 | 12.326 | **6.956** | **0.84** | unmet |

3356896 reuse walls / full replay: 2.348/3, 1.491/0, 1.643/0, 2.202/0. Best full=0 shell is **1.491 ms**. At this OCC, TPS 1.5 needs SF ≤ **0.920 ms**. The full=0 shell already misses that need.

15274915 reuse full replay 6, 5, 7, 4. At this OCC, TPS 1.5 needs SF ≤ **3.889 ms**.

### Sample B (same binary)

| Block | OCC ms | SF reuse | TPS |
|------:|-------:|---------:|----:|
| 3356896 | 1.112 | 1.636 | 0.68 |
| 15274915 | 6.201 | 9.353 | 0.66 |

### Tip before the v1 change (same host, same harness)

| Block | OCC ms | SF reuse | TPS |
|------:|-------:|---------:|----:|
| 3356896 | 0.973 | 1.507 | 0.65 |
| 15274915 | 5.304 | 10.627 | 0.50 |

Host noise moves OCC by more than a millisecond between N=5 runs. Every sample is under 1.5.

## Stop

The named next cut cannot be landed inside revm’s `Database::basic`. The thin full=0 shell on this 4-CPU host is already above OCC/1.5, so another park kind or rem strip does not open the bar. Execution code is unchanged from `c3475a1`.
