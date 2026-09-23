# SpecFence revm frame-suspend land v1

**Date:** 2026-09-23
**Branch:** `cursor/specfence-revm-frame-suspend-5bf9` (new PR; base `cursor/specfence-region-learn-dag-51e8`)
**Start:** `374977dd823b3b89bc36b87b77e45224de5cc815` (PR #46 tip, land v2)
**Co-author:** `0xstride <fengjy73@users.noreply.github.com>`

Bar is **TPS SF/OCC ≥ 1.5 on both** focus blocks (primary = OCC wall / SF reuse wall). **Not met.** A rewind-safe interpreter can outlive `Database::basic` → `Blocking` and sit in a spare `Evm` while the same worker runs another transaction. The focus protected reads are not that opcode, so the hold never armed (`frame_suspends=0` on both).

## Four-class status

Status after the same-frame cut land-v2 named. Release on the existing protected read is still `is_validated`.

| Class | 3356896 | 15274915 | Root if unsolved |
| RAW | yes (spine) | partial | Large RAW is secondary; no access-grain true-Data arm is the acceptance driver |
| WAW | **no** | **no** | The short region still leaves `basic` as `Blocking`. On these blocks that call is `CALL` (thin) or pre-frame (large), so the rewind-safe hold does not keep the frame. The thin shell stays above OCC/1.5 |
| WAR | **no** | **no** | The WAR counter fires when a later writer is in the learned list. There is no reader-reserve and no WAR edge list |
| Chain | partial | **no** | Thin wait is one hop at the read; workers are not pinned by an off-queue plant. Large L=77 is still the ≥32 crit hold, and the wall stays above OCC/1.5 |

## Ideal-DAG Diff

Bounds are the ideal-DAG note (LB = L_crit). Stubs are unchanged: `lab/notes/dag-3356896.json`, `lab/notes/dag-15274915.json` (`known_from_notes_only: true`). No new edges were emitted. No invented RAW/WAR endpoints.

### 3356896

Ideal: WAW spine on basic `dff71d59d972`, writers `4, 31, 66, 67, 69, 70, 93, 96, 103, 115, 131, 132, 135, 138, 141, 166, 171`, earliest k = 5 or 6, RAW = 0 on that spine, WAR = 18 (endpoints unknown), antichain proxy ~153, LB = 0.031 ms.

| | |
|---|---|
| Recognized | Reuse `region_avoid` on `0xdff71d59d972d654`, **n=17, head tx 4, tail tx 171** on later iters. Same ℓ and the same endpoints as the note. |
| Missed txs | Cold still sees a partial writer list (`chain_n=8`, head 66). Scope floor is 12, so that partial list is not the armed region. |
| Missed edges | WAR=18 endpoints are still unnamed. |
| Wrong Avoid | Off-queue `plant_nearest_preds` on the 17 stays dropped. This cut does not plant it. |
| Same-frame Avoid | **Installed only for a rewind-safe opcode** (`BALANCE` / `EXTCODESIZE` / `EXTCODEHASH` / `SELFBALANCE`). This block’s protected `Blocking` that reached the interpreter was **`CALL` 0xF1** (opcode 241, 23/23). `CALL` pops the stack before `basic`. Those requests are rejected and `catch_error` drops the frame. Holds = 0. |

### 15274915

Ideal: WAW hop on basic `abd6bb397881`, L=77, earliest k=3, RAW=35, WAR=107, work-weighted path tx 0 → tx 102 (13 txs), LB=1.19 ms, antichain proxy ~1111. Full writer list is not in the notes and was not emitted.

| | |
|---|---|
| Recognized | The ≥32 crit spine is still the existing hold (`chain_n` 60 then 77 on `0xabd6bb3978815b97`). The short-region slot does not replace it. |
| Missed txs / edges | `abd6bb…` writers, RAW=35, and WAR=107 are not in a regenerated edge list. |
| Wrong Avoid | Length-8 side location stays excluded (floor 12). It is not planted. |
| Same-frame Avoid | **Not used.** All 557 suspend requests were pre-frame (`frame_pre_frame=557`, `unsafe_ops=[]`). `basic` returned `Blocking` in validate / pre-execution or `frame_init`, before an interpreter halt exists to rewind. Holds = 0. ResumeAtK still does not arm at k=3 (`k < 8`). |

## How pevm wires suspend / resume

Stock revm still cannot do this by itself. `Database::basic` is a sync callback. A fatal `Blocking` goes through `checkpoint_revert` and `Handler::catch_error` (`local.clear`, `journal.discard_tx`, `frame_stack.clear`). The worker has one `Evm`; the next antichain tx calls `set_tx` and `journal.clear()`. `Evm` is `!Send` (`LocalContext` is `Rc<RefCell<_>>`), so a parked frame has to stay on the owning worker.

This tree keeps the frame only when the halting opcode did not pop or resize before `basic`:

1. `Vm` sets `VmDb.allow_frame_suspend` for a known protected toucher whose predecessor is not `is_validated`, while no frame is already parked, and only when inspect is off. The spare `Evm` is **not** built until a hold actually happens.
2. `wait_protected_commit` on `Blocking` calls `frame_suspend::request(pred)`. It does not `add_wait_for_dependency`.
3. Execution stays on `run_plain`. After a `FatalExternalError`, `hold_if_rewind_safe` rewinds PC by −1. If the opcode is `0x31` / `0x3b` / `0x3f` / `0x47`, it refunds static gas, `take_error`s, `mark_held`, and `Handler::run` returns without `catch_error`. Any other opcode (including `CALL` 0xF1) is put back at PC+1 and takes the ordinary fatal path.
4. `Vm::park_if_suspended` builds the spare `Evm` and swaps. The live interpreter, journal, and DB move together into `spare`. The next tx’s `set_tx` / `journal.clear()` hits the empty `Evm`.
5. `Scheduler::frame_held` makes `recover_executing_waiter` return false, so heal cannot turn the ghost `Executing` into `Ready` (that would fresh-execute on another worker).
6. Before `schedule::pick`, the owner polls: if the pred is `is_validated` and the tx is still `Executing`, it resumes locally. `resume_pevm_tx` continues `pump_frames` plus the saved `InitialAndFloorGas` and EIP-7702 refund (`post_execution`). It does not call `Handler::run`, `validate`, `pre_execution`, or a new root `frame_init`. Resume also skips `set_tx`, `journal.clear`, tip install, and suffix-jump seeding.
7. If the tx is no longer `Executing`, the spare is discarded (`take_error`, `local.clear`, `journal.discard_tx`, `frame_stack.clear`) and the pin is dropped.
8. A request that never reaches an interpreter halt is counted as `frame_pre_frame` and dropped. It is not a held frame.

One parked frame per worker. Large-block protected admission is **not** skipped when the slot is free: on these blocks the `Blocking` `basic` is not rewind-safe, and entering early only pays a prefix that `catch_error` drops. Thin blocks (`n ≤ 176`) still skip that admission gate.

Discarded levers stay off: off-queue plant of the 17-writer spine, Estimate Block gate, ordinal strip, Win Fence prepaid, inspect / absolute jump as the win path, Aborting park, `next_task*`.

A later change can claim the focus-path win only by showing `frame_suspends > 0` and a resume of **that** frame. Holding `CALL` needs the popped stack values (a PC rewind is not enough; `pop` truncates the `Vec`). Holding the pre-frame `basic` needs a snapshot before a frame exists. Neither is in this PR.

## Soft=0 Instant-off N=5

New binary. `cargo +stable run -p pevm --release --config profile.release.lto=false --example specfence_3356896_compare`. Host `nproc` = 4, harness `SPECFENCE_COMPARE_CORES=8`, `SPECFENCE_COMPARE_ITERS=5`, `SPECFENCE_COMPARE_CHECK=1`. No `SPECFENCE_DISABLE_SOFTWAIT`, inspect, or absolute-jump env. `seq=par` on both. SpecFence lines: `occ_picks=0`, `soft_wait_arms=0`, `est_block=0`.

| Block | OCC ms | SF cold | SF reuse | TPS | vs ≥1.5 | suspends | resumes | requests | unsafe | pre-frame |
|------:|-------:|--------:|---------:|----:|:-------:|---------:|--------:|---------:|-------:|----------:|
| 3356896 | 0.973 | 1.599 | **1.445** | **0.67** | unmet | 0 | 0 | 23 | 23 (`CALL` 241) | 0 |
| 15274915 | 4.964 | 8.694 | **14.293** | **0.35** | unmet | 0 | 0 | 557 | 0 | 557 |

### 3356896 walls

OCC: 1.608, 1.000, 0.794, **0.973**, 0.847. SF: 1.599, 1.445, 1.332, **1.265**, 2.062.

Focus chain `dff71d59d972d654`. Reuse `chain_n` 15, 17, 17, 17. `full` / `full_from_0`: 4/0 (fail_k=6), 3/3 (none), 1/0 (fail_k=9), 0/0 (none). `waw_c` 4, 2, 1, 0. `war_ab` 3, 21, 15, 30. `war_c=0`. `raw_c` mostly 0. `est_block=0`.

TPS 1.5 at this OCC needs SF ≤ **0.649 ms**. Best reuse is **1.265 ms** (`full=1`). The full=0 iter is **2.062 ms** (`wait_for_dependency=17`). Land-v2’s best full=0 thin shell was 1.491 ms against a slower OCC (~0.92 ms need). This sample’s OCC is faster, so the need is tighter, and the shell is still about 2× the bar.

### 15274915 walls

OCC: 6.876, 4.838, 4.906, **4.964**, 5.462. SF: 8.694, 21.471, **14.293**, 13.816, 7.666. Median of the four reuse walls is noisy on 4 CPUs / 8 workers.

Focus chain `abd6bb3978815b97`. `chain_n` 60 then 77. `fail_k_min=3`. `chain_c` 120, 62, 19, 5. `waw_c` stays 2 after cold. `war_c=0`, `war_ab` large. `raw_c` 1–2. `est_block=0`. Cold `explore=1` is not on the reuse iters.

TPS 1.5 at this OCC needs SF ≤ **3.309 ms**. Best reuse is **7.666 ms**. Land-v2 reuse median was 6.956 ms. This sample’s median is worse; the best is in that same band and still misses 1.5.

## Stop

The mechanism land-v2 asked for exists for a rewind-safe account read: the interpreter outlives `Blocking`, the spare `Evm` survives another tx’s `set_tx` / `journal.clear()` on the same worker, and resume is `pump_frames` rather than `Handler::run`. The two focus blocks do not take that opcode. Thin protected reads that reached the interpreter are `CALL`. Large protected reads return `Blocking` before a frame exists. Holds stayed 0, so the shell is the same class of miss as land-v2, and TPS stays under 1.5 on both.
