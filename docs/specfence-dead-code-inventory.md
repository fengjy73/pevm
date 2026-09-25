# SpecFence dead-code inventory

**Date:** 2026-09-25 (Beijing)
**Tree:** `cursor/soft0-execute-inflation-dig-e5fa` at `2ddda42` (`measure(specfence): record Soft=0 execute inflation and step offsets`).
**Against upstream:** `origin/main` `e94b0e3`. Fork delta in `crates/pevm/src`, examples, `tests/specfence.rs`, and `scripts/`: **+70,822 / −241** lines across 76 files.
**This PR:** the plan only. Product code is unchanged.

Two other agents are editing SpecFence (a `seq≡par` fix and in-block learning). Delete after those land, and re-run `scripts/specfence_flag_inventory.py` first. Line numbers below will move.

## What stays

The kept engine is `run_sf_block` (`crates/pevm/src/specfence/worker.rs`) plus the AccessEvent primitives in `access_spine.rs`:

- **RAW** — `Recipe::WaitTrueVersion` (wait for the published tip).
- **WAW** — `Recipe::OrderedTip`.
- **WAR** — `Recipe::RetainHistory`.

Plus the learn objects the default SpecFence block actually consults at start and on the access: `AccessSpine` intra credit, `SpinePrior` / `InterBlockPrior` radar, `LiveLearner`, `CostPolicy`, `BayesMap`, `HotSet`, and `ArmTable::begin_from_prior`. `ConcurrencyMode::Occ` only has to stay correct. It does not have to stay instrumented.

The current harness, which this plan does not delete:

- `crates/pevm/examples/specfence_inflation_dig.rs` (722 lines)
- `scripts/soft0_percore_scan.sh` (309)
- `scripts/specfence_inflation_report.py` (962), which imports `scripts/specfence_step_ideal.py` (622)

## How a row was classed

- **KEEP** — on the default Soft=0 path, or required by that harness.
- **DELETE** — unreachable, only reached when a non-default flag is set, or a whole file whose only callers are dig tools this harness does not run.
- **DECIDE** — still executed on the default path, but the bench called it a failed or superseded experiment, or the file mixes a kept repair path with research-only jump/inspect. Deleting it changes Soft=0 behavior or needs a split. That is a product call, not a dead-code delete.

`cargo +stable check -p pevm --tests` (rustc 1.98.1) was run with `mod computer` and `#[cfg(test)] mod kernel` commented out. It finished clean (warnings only). Those two lines were reverted before this commit. A full `dead_code` pass was then read from that same check on the unmodified tree.

## Counts

| Bucket | Units | Lines (the unit itself) |
| --- | ---: | ---: |
| KEEP specfence modules | 39 | 36,651 |
| DELETE specfence modules | 8 | 4,099 |
| DECIDE specfence modules (must be split, not deleted whole) | 1 (`boundary.rs`) | 3,556 |
| KEEP examples / scripts | 1 example + 3 scripts | 2,615 |
| DELETE examples / scripts | 11 examples + 5 scripts | 10,238 |
| DECIDE mechanisms inside KEEP files | 7 | not a file delete |

Shared-path hook edits are extra and are listed in their own section. They are not in the module totals.

`mod.rs` (1,055) is the module root. It shrinks as children go, and it is not in the 39. 39 + 8 + 1 + `mod.rs` = 49 files, 45,361 lines.

## 1. Flags, env vars, cargo features, config knobs

`scripts/specfence_flag_inventory.py` prints every `SPECFENCE_*` token under `crates/`, `scripts/`, and `bins/`. This tree has **54**. Two more exist only on open PRs and are listed at the bottom.

Cargo features in `crates/pevm/Cargo.toml`: `defaults`, `rpc-storage`, and `global-alloc` are not SpecFence. **`inflation-alloc = []` is a SpecFence feature with no `cfg` use anywhere.** Delete the feature. `InflationAlloc` in `inflation.rs` is only reached if some binary installs it as the global allocator. The harness never sets `SPECFENCE_INFLATION_ALLOC`. Delete the allocator type with the feature. Keep the rest of `inflation.rs`.

Code constants that are not env vars:

| Knob | Default | Reached on default Soft=0? | Introduced |
| --- | --- | --- | --- |
| `ConcurrencyMode` | `Occ` | SpecFence is opt-in via `Pevm::with_concurrency_mode` | pre-#45 SF-PS (`43dbf46`, PR #44) |
| `Pevm::finegrain_enabled` | `false` | No. `set_finegrain_trace` / `_deep` / `_journal` | fine-grain dig examples |
| `Pevm::set_adaptive_params` | unused | Compiler: method never used | learner land |
| `THIN_SHELL_N` | `176` | Yes. Thin vs large admit | `policy.rs` (`6bd7fc2`) |

### 1.1 Default-on, and the default path does reach them

| Env | Unset means | Default path | Introduced |
| --- | --- | --- | --- |
| `SPECFENCE_GLOBAL_IDEAL_READY_POOL` | on, unless `0`/`false` | Yes. `runnable_set` pop law | `eeb3e37`, PR #57 |
| `SPECFENCE_IDEAL_TIMED_ADMIT` | on, unless `0`/`false` | Yes. Late-wave split inside each shard | `7b755e0`, PR #56 |
| `SPECFENCE_NESTED_BIND` | on, unless `0`/`false`/`off`/`no` | Yes, when a nested stash is armed. `tx_runner` checks the TLS on every `CALL` for OCC and SF | `2d5249c` (Iter29); default-on narrowed in Iter30 |
| `SPECFENCE_BIND_SNAP` | **`ResumePath`**, not off. `0` forces off. `1` is Mass | Yes, but only on SuffixRepair resume / `force_ordered_admit` / live capture. Not on every first execution | `3a9a945` (Iter19) |
| `SPECFENCE_BIND_SNAP_JUMP` | follows capture. Unset + ResumePath means **jump enabled**. `0` forces off. `1` forces on | Same repair window as snap | Iter24, same snap work |
| `SPECFENCE_ABSOLUTE_JUMP` | `absolute_jump_env_enabled()` is **false**. `suffix_repair_jump_env_ok()` treats unset as **allow** (only `=0` blocks it) | The suffix-repair gate is allow-by-default. The general jump gate is off | `f1a222f` (plant M1e) |

`build_evm` calls `handler_ordered_admit_snap_install_wanted()`, which is true whenever snap mode is ResumePath. **Unset `SPECFENCE_BIND_SNAP` therefore replaces the SLOAD handler on every Ethereum EVM, including OCC.** The SSTORE plant stays off unless inspect, absolute jump, or `SPECFENCE_HANDLER_CAPTURE=1`.

### 1.2 Default-off. The harness turns some of them on

Wall rows in `soft0_percore_scan.sh` leave these unset. `profile` / `probes` / `steptrace` modes set the ones marked harness.

| Env | Unset | Harness | Introduced |
| --- | --- | --- | --- |
| `SPECFENCE_INFLATION` | off | profile + probes | `2ddda42`, PR #61 |
| `SPECFENCE_INFLATION_DAG` | off | profile + probes | PR #61 |
| `SPECFENCE_INFLATION_OS` | off | profile + probes | PR #61 |
| `SPECFENCE_INFLATION_DUMP` | off | profile + probes | PR #61 |
| `SPECFENCE_INFLATION_READS` | off | probes (`--extra-probes`) | PR #61 |
| `SPECFENCE_INFLATION_PERF` | off | probes | PR #61 |
| `SPECFENCE_INFLATION_ALLOC` | off | never set | PR #61 |
| `SPECFENCE_INTERP_SPLIT` | off | probes | `3421771`, PR #55 |
| `SPECFENCE_STEP_TRACE` | off | one OCC workers=1 trace | PR #61 |
| `SPECFENCE_PIN_CPUS` | empty, `pin_worker` returns | every scan row | PR #61 |
| `SPECFENCE_COMPARE_CORES` | example default 4 | scan sets workers | compare example, reused by the dig |
| `SPECFENCE_INFLATION_WHICH` | `scan` | `scan` or `steptrace` | PR #61 |
| `SPECFENCE_INFLATION_K` | 10 | `--k` | PR #61 |
| `SPECFENCE_INFLATION_ORACLE_K` | 0 | `--oracle-k`, else 0 | PR #61 |
| `SPECFENCE_INFLATION_SEQ_CPU` | 0 | first pinned cpu | PR #61 |
| `SPECFENCE_INFLATION_SEED` | 1 | `1` | PR #61 |
| `SPECFENCE_INFLATION_OUT` | example default path | scan outfile | PR #61 |
| `SPECFENCE_INFLATION_BLOCKS` | one compare block | optional | PR #61 |
| `SPECFENCE_INFLATION_ENGINES` | all three | optional | PR #61 |
| `SPECFENCE_INFLATION_SEQCHECK_N` | 10 | seq≡par section of the dig | PR #61 |
| `SPECFENCE_COMPARE_BLOCK` | `15274915` | seqcheck | compare example |
| `SPECFENCE_PROFILE` | off | no | `dfa34ed` |
| `SPECFENCE_BUSY_STALL` | off | no (previous dig, PR #60) | `a9585b6` |
| `SPECFENCE_IDEAL_PROXIMITY_DIFF` | off | no | `7b755e0`, PR #56 |
| `SPECFENCE_IDEAL_PROX_DIFF` | off (alias) | no | PR #56 |
| `SPECFENCE_IDEAL_PROXIMITY_DIFF_OUT` | example only | no | PR #56 |
| `SPECFENCE_IDEAL_PROX_DIFF_OUT` | example only | no | PR #56 |

Flag-off bodies do not take `Instant`. The call still loads a `OnceLock` on the OCC read path (`inflation::reads_on`, `step_trace::note_read`, `busy_stall::lock_t0`).

### 1.3 Default-off research, or the reader is never called

| Env | Unset | Reached if unset? | Introduced |
| --- | --- | --- | --- |
| `SPECFENCE_ENABLE_INSPECT` | off | No. Inspect / jump research | `a855e14` (Adaptive CC R0) |
| `SPECFENCE_HANDLER_CAPTURE` | off | No | `28948dd` (Iter10) |
| `SPECFENCE_VALUED_CALL_CACHE` | off (`=1` or inspect turns it on) | No | `60d0cae` (plant M1h) |
| `SPECFENCE_JUMP_DIG` | off (presence-only) | No. eprintln traces | `4d13617` (Iter27) |
| `SPECFENCE_HANG_TRACE` | off | No. `worker.rs` eprintln | `dc4737c` |
| `SPECFENCE_DISABLE_SOFTWAIT` | off | **The function `softwait_disabled` is never called.** `specfence_g7_smoke` reads the env itself | `3a3e26b` |
| `SPECFENCE_DISABLE_AWAIT_AT_A` | off | **`await_at_a_disabled` is never called** | `5509788` (three-pillar) |

### 1.4 Example-only (die with the dig binaries)

| Env | Binary | Unset |
| --- | --- | --- |
| `SPECFENCE_G7_TAG`, `SPECFENCE_G7_ITERS`, `SPECFENCE_G7_XBLOCK` | `specfence_g7_smoke` | empty / default iters / off |
| `SPECFENCE_COMPARE_ITERS`, `SPECFENCE_COMPARE_JSON`, `SPECFENCE_COMPARE_CHECK`, `SPECFENCE_COLD_EACH_ITER` | `specfence_3356896_compare` | example defaults; check and cold are presence-only |
| `SPECFENCE_ALL_BLOCKS`, `SPECFENCE_ALL_BLOCK_IDS`, `SPECFENCE_ALL_ITERS`, `SPECFENCE_ALL_OUT`, `SPECFENCE_ALL_PROCESS_TOP`, `SPECFENCE_ALL_REUSE` | `specfence_all_blocks_sweep` | example defaults |
| `SPECFENCE_BUILD_HEAD` | `option_env!` in g7 and all-blocks | fallback sha string |

`SPECFENCE_COMPARE_CORES` and `SPECFENCE_COMPARE_BLOCK` are shared with the inflation dig. Keep those two names. Delete the rest of the compare/all/g7 knobs with those binaries.

### 1.5 Not in this tree

| Env | Where | Unset | Note |
| --- | --- | --- | --- |
| `SPECFENCE_REGION_LEARN_AVOID` | PR #58, `cursor/soft0-region-learn-avoid-1173`, `access_arm.rs` | **on**, unless `0`/`false` | Not an ancestor of `2ddda42` |
| `SPECFENCE_REGION_LEARN_AVOID_V2` | PR #59, `cursor/soft0-region-learn-avoid-v2-8c78`, `region_avoid.rs` (945 lines) | **on**, unless `0`/`false`/`off` | Same. `=0` builds a disabled table |

If either PR merges into the kept line, delete that arm before anything else. Both default on, and the v2 bench was recorded as FAIL (`0063b7c` on the v1 branch’s history). They are not dead code *here* because they are not code *here*.

### 1.6 Named mechanisms from the request

| Name | In this tree? | Class |
| --- | --- | --- |
| Estimate as an SF block | SF `live_writer_act` returns `Skip` on thin non-WaitOnce so SF does not Block on `MemoryEntry::Estimate`. The OCC estimate entry is upstream Block-STM | OCC estimate: **KEEP**. SF Estimate-block counters and `SfRead::Estimate`: **DELETE** slice (compiler: `SfRead` never used) |
| `Scheduler::next_task` | OCC `next_occ_task` still calls it. SF `run_sf_block` must not (test `specfence_true_spine_sources_never_call_next_task`) | Keep a plain OCC `next_task`. **DELETE** the wave/ready overloads once tests move |
| opcode-if | No symbol | Already gone |
| frame-suspend | `tx_runner` `YieldWait` resume. Successful RAW wait stays inside the host; yield is the deadlock release | **DECIDE**. It is the RAW escape, not an unused opcode experiment |
| IntraPatch | `ArmTable::apply_pending_patches` runs on every `schedule::pick` and again at release | **DECIDE**. Live on the default pick |
| Cross-block prior arms | `arms.begin_from_prior` and thin `plant_wait_edges` still run. `AccessSpine` docs say the prior is radar-only and must not install an Avoid arm | **DECIDE**. Two priors are both live |
| Region Learn Avoid v1/v2 | Not in this tree | Delete on merge of #58/#59 |
| Global Ideal-ready pool | Default-on pop law. Bench doc: FAIL | **DECIDE** |
| AdmitShard | `admit_deque.rs` + index bands in `runnable_set`. This *is* the indep queue | **DECIDE** to flatten the pop law. Do not delete the queue |
| IdleStealWake | Folded into `runnable_set` park/steal (`c272ea5`, PR #49) | **DECIDE** with the pop law |
| QuietExit | `worker.rs` `quiet_exit` / `block_quiet` on the idle arm | **DECIDE**. It is the default completion check |

## 2. Modules, binaries, scripts

Line counts are `wc -l` on this tree.

### 2.1 DELETE modules (4,099 lines)

| File | Lines | Why |
| --- | --- | --- |
| `finegrain.rs` | 1,980 | `finegrain_enabled` defaults false. Only dig examples call `set_finegrain_*`. Public re-exports in `lib.rs` exist for those examples |
| `busy_stall.rs` | 590 | `SPECFENCE_BUSY_STALL` default off. Not used by the inflation harness. Call sites still sit on OCC `next_task` and SF worker |
| `process.rs` | 511 | Writes a snapshot into `Pevm::last_process`. No schedule decision reads it during the block. Consumers are dig examples |
| `ideal_prox.rs` | 440 | `SPECFENCE_IDEAL_PROXIMITY_DIFF` default off. Compare example only |
| `decision_field.rs` | 368 | Only fed by `process.record_decision` |
| `kernel.rs` | 132 | `#[cfg(test)]` museum. Comment in `mod.rs`: rem-legal source of truth is `CertificateTable` |
| `heat.rs` | 64 | `update_heat` runs only for `ConcurrencyMode::Pcc`. `is_hot` is `dead_code` |
| `computer.rs` | 14 | A comment plus a source-text test. **cfg-out verified** |

### 2.2 DECIDE module

| File | Lines | Why |
| --- | --- | --- |
| `boundary.rs` | 3,556 | ResumePath snap, nested consume, and `try_apply_pending_pc_resume` are on the default repair / `tx_runner` path. Inspect, Mass snap, absolute jump, valued-call cache, and `SPECFENCE_JUMP_DIG` prints are flag-off. Deleting the file breaks RAW repair. Splitting it is the follow-up |

`rem.rs` (3,128) stays **KEEP**: `PartialRetryTable` is built for every parallel block and used by resolve. `research_apply_abort_repair` and the inspect-only continuation builders are a **DELETE** slice inside it (compiler already marks many of those methods unused: `apply_lean_abort_repair`, `plan_repair`, `note_effect`, and about thirty more).

### 2.3 KEEP modules (39 files, 36,651 lines)

These are on `run_sf_block` or the harness. Dead methods inside them are a later clippy pass, not a reason to delete the file.

| File | Lines | Role on the default path |
| --- | ---: | --- |
| `policy.rs` | 6,857 | `CostPolicy` at admit seed and on `schedule::pick` |
| `rem.rs` | 3,128 | `PartialRetryTable` on resolve. Inspect-only methods inside it are a DELETE slice (section 2.2) |
| `runnable_set.rs` | 2,430 | The only SF queue. Pop-law experiments live here |
| `ready_edge.rs` | 2,474 | Detect edges, gating, publish wake |
| `admit.rs` | 2,290 | `admit_seed_begin_block` |
| `metrics.rs` | 2,014 | Counters the worker and pick already bump. Many `record_*` methods are unused |
| `learner.rs` | 1,860 | `LiveLearner::begin_block_with_params`. Whole module is `allow(dead_code)`, so the compiler did not list its unused methods |
| `access_spine.rs` | 1,511 | RAW / WAW / WAR |
| `worker.rs` | 1,102 | `run_sf_block` |
| `sketch.rs` | 903 | Prior morph seed. Several canary fields are never read |
| `executor.rs` | 900 | `validate_to_plan`, `next_occ_task`, `validate_occ_stage` |
| `resolve.rs` | 847 | EV helpers used by bayes / resolve |
| `resolve_plan.rs` | 842 | `ResolvePlan::apply` |
| `sf_mv.rs` | 808 | Tips and WAR pin. `SfRead` itself is unused |
| `inflation.rs` | 822 | Harness probe |
| `arm_table.rs` | 739 | Prior arms + IntraPatch at pick |
| `access_arm.rs` | 751 | WaitOnce / `live_writer_act` |
| `bayes.rs` | 632 | Block-start seed and `update_bayes` |
| `wave.rs` | 585 | Park table. `SoftWaitSoft` and `EarlyAbort` variants are never constructed |
| `access_policy.rs` | 569 | `decide_access_queried`. Bare `decide` is unused |
| `dag.rs` | 463 | `wake_on_data`. Many SoftWait methods are unused |
| `edge.rs` | 962 | `edges.record` / `broadcast_avoid` from `vm.rs` |
| `step_trace.rs` | 434 | Harness |
| `collateral.rs` | 318 | Conflict class for resolve. `envelopes_disjoint` is unused |
| `engagement.rs` | 308 | `profile_timing_enabled`, `research_inspect_enabled`. Mode-flip methods are unused |
| `hotset.rs` | 322 | `begin_block` / `track_from_prior` |
| `certificate.rs` | 258 | Fence success strip |
| `schedule.rs` | 239 | `pick` |
| `prior.rs` | 205 | `RwPriorMap` updated at end of a SpecFence block |
| `ordered_admit_act.rs` | 150 | `act_wait_for` on the SF avoid path. `act_ordered_admit_has_data` is unused |
| `access_log.rs` | 136 | Per-tx access ordinal on the SF gate |
| `visibility.rs` | 120 | `VisibilityPolicy` on pick |
| `producer_stage.rs` | 107 | Stage reserve. `note_promote` is unused |
| `region.rs` | 103 | `RegionTable` on `MvMemory`. SF promote and PCC seed |
| `lane.rs` | 85 | Serial-lane grant. Only the PCC serial-lane caller uses `grant`; confirm before a later delete |
| `access_vis.rs` | 68 | `compose_unfinished` |
| `repair.rs` | 62 | Repair grain used by executor |
| `feeder.rs` | 56 | Admit / resolve feeder. `feeder_is_cold` is unused |
| `admit_deque.rs` | 191 | Chase-Lev deque under AdmitIndep |

`lane.rs` is the weak KEEP. Its only production caller found is `pcc_serial_lane`, which `maybe_wait` reaches only for `ConcurrencyMode::Pcc`. Left as KEEP until the PCC delete step proves no SpecFence call remains.

### 2.4 Examples and scripts

**KEEP**

- `specfence_inflation_dig.rs` (722)
- `scripts/soft0_percore_scan.sh` (309)
- `scripts/specfence_inflation_report.py` (962)
- `scripts/specfence_step_ideal.py` (622) — imported by the report

**DELETE examples (8,486)**

| File | Lines |
| --- | ---: |
| `specfence_3356896_compare.rs` | 1,273 |
| `specfence_all_blocks_sweep.rs` | 956 |
| `specfence_contiguous_segments_analysis.rs` | 928 |
| `specfence_effect_raw_deeper.rs` | 879 |
| `specfence_g7_smoke.rs` | 825 |
| `specfence_mainnet_sweep.rs` | 724 |
| `specfence_effect_raw_journal_stream.rs` | 667 |
| `specfence_all_blocks_upper_bound.rs` | 662 |
| `specfence_finegrain_analysis.rs` | 640 |
| `specfence_effect_raw_deep_analysis.rs` | 544 |
| `specfence_l1_l2_collect.rs` | 388 |

**DELETE scripts (1,752)**

| File | Lines | Why |
| --- | ---: | --- |
| `scripts/specfence_busy_stall_report.py` | 375 | PR #60 dig. Harness does not import it |
| `lab/scripts/l3_makespan_ev_lab.py` | 324 | offline lab |
| `lab/scripts/l3_offline_ev_lab.py` | 438 | offline lab |
| `lab/scripts/l4_prior_predictivity.py` | 324 | offline lab |
| `lab/experiments/scripts/plot_vldb.py` | 291 | plot helper |

`crates/pevm/tests/specfence.rs` (4,325) is **KEEP as a file** until the feature each test locks is removed. Many `#[ignore]` tests exist only for `SPECFENCE_ENABLE_INSPECT`. Delete those tests in the same commit as the inspect path. Do not delete the seq≡par tests.

`lab/notes/` is historical design notes, not executable SpecFence. Out of scope for this code deletion. Do not mass-delete notes in the code PR.

### 2.5 Dead items the compiler already named

`cargo +stable check -p pevm --tests` on the unmodified tree reports these as never used or never constructed. They are **DELETE** slices inside KEEP files. Duplicates between lib and lib-test were collapsed.

- `access_arm.rs`: `wait_once_peers_before`
- `access_policy.rs`: `decide`
- `access_spine.rs`: `VersionPointer`, `PriorAction`, `HandlerFault`, `ideal_lb`, `handler_preserves_frame`, `must_retain`, `prior`, `retained_for`, `score_loc`, `credit`, `action`
- `arm_table.rs`: variants `Defer`, `Seg`, `Full`; `from_loc`, `mark_under_covered`, `note_explore`, `arm_of`; field `n_pull`
- `bayes.rs`: fields `p_raw`, `p_ordered_admit`; several methods (warning says “multiple methods” at lines 277 and 308)
- `certificate.rs`: `clear`, `begin_block`
- `collateral.rs`: `envelope_addrs`, `envelopes_disjoint`
- `engagement.rs`: `softwait_disabled`, `await_at_a_disabled`, `from_u8`, `mode`, `is_storm`, `maybe_flip_mode`
- `executor.rs`: `specfence_partial_abort_validate`, `specfence_plant_is_occ`, `specfence_access_is_occ`
- `feeder.rs`: `feeder_is_cold`
- `hotset.rs`: `insert`, `record_location_hot_resolve`
- `mod.rs`: `AccountHints::{txs, is_value_transfer, is_pure_transfer, n_txs}`, `SpecFenceCtx::choose_resolve`; fields `tau`, `params` never read
- `ordered_admit_act.rs`: `act_ordered_admit_has_data`
- `policy.rs`: `seg_len`, `is_high_prepaid`, `leaves_occ_tail`, `sigmoid`, `update`, and further methods at lines 990 and 1140; fields `peer`, `refuse`
- `producer_stage.rs`: `has_reserved`, `next_reserved`, `note_promote`, `promote_count`; field `promote`
- `ready_edge.rs`: `gated_count`, `pending_gated_count`, `add_refuse_ns`, plus “multiple methods” at line 318
- `region.rs`: `clear_account_wait`
- `runnable_set.rs`: `force_push`, `mark_running`, `pick` (the live pick is `pick_in`); field `from`
- `sf_mv.rs`: enum `SfRead`; methods `inner`, `tips`, `read`, `read_tip`, `read_opt`, `read_released_or_tip`; field `value`
- `sketch.rs`: `predicted_writer`, `avoid_broadcasts`, `canary_probes`, `clique_gates`; constants `CANARY_GRANT`, `CLIQUE_UNFENCED_CAP`; canary fields never read
- `wave.rs`: variants `SoftWaitSoft`, `EarlyAbort`; “multiple methods” at line 157
- `pevm.rs`: `set_adaptive_params`, `adaptive_params`
- `vm.rs`: `take_pending_park_location`
- `scheduler.rs`: “multiple methods” at line 159
- `mv_memory.rs`: `DataNest`’s tuple field is write-only (the guard’s value is the side effect)

`learner.rs` will not show up until `#![allow(dead_code)]` is removed. Do that in the dead-method step, not before.

## 3. Hooks on the shared path

These run when `ConcurrencyMode` is `Occ`. Upstream `origin/main` does not have them.

| Hook | Where | Cost with SpecFence off | Remove? |
| --- | --- | --- | --- |
| Build every SF table before the mode branch | `pevm.rs` `execute_revm_parallel` (~lines 550–722): `SpecDag`, `RemCounters`, `PartialRetryTable`, `WaveParkTable`, `AccessOrdinalLog`, `CertificateTable`, `ReadyEdgeTable`, `ProducerStageTable`, `RunnableSet`, `ArmTable`, `AccessArmTable`, `SfTipTable`, `AccessSpine`, `LaneTable`, `EdgeTable`, `HotSketch`, `ProcessTrace`, `IdealProxLog`, `LiveLearner`, `MetricsInner` | Allocations on every OCC block | **Yes.** Build them only for `SpecFence` |
| `inflation::BoundGuard::enter` on `Pevm::execute` | `pevm.rs` | `OnceLock` load, then return | Keep while the harness stays. One load per block |
| OCC worker `Instant::now` around every pick and every validate, plus `metrics_inner.add_phase_*` | `pevm.rs` OCC loop (~783–847) | Two clocks and atomic adds per task, probes off | **Yes.** This is PR #54 phase timing, not Block-STM |
| `next_occ_task` increments `OCC_PICK_CALLS` | `executor.rs` | One atomic per pick | **Yes** for speed. Tests use it to prove SF did not enter OCC. Move the counter behind a test cfg |
| `next_task` idle arm always takes `Instant::now` and calls `busy_stall::charge_nested` | `scheduler.rs` ~332 | Clock plus `OnceLock` on every idle yield, including OCC | **Yes**, with `busy_stall.rs` |
| `try_execute_ready` calls `busy_stall::lock_t0` around the status mutex | `scheduler.rs` ~202 | `OnceLock` per execute claim | **Yes**, with `busy_stall.rs` |
| `VmDb::basic` / `storage` / `get_code_hash` call `step_trace::note_read` and `inflation::reads_on` | `vm.rs` | `OnceLock` per host read. No `Instant` when off | Keep until the harness is retired |
| `DataNest` around `MvMemory.data` lookups | `mv_memory.rs`, `vm.rs` | Thread-local increment per lookup, both modes. It exists to panic on DashMap re-entry (the 19807137 double-free) | **DECIDE.** It is a correctness guard, not a probe |
| Reader index, aborted-incarnation map, residual write sets, `retained_history` updated from `MvMemory::record` and consulted on the read walk | `mv_memory.rs` | Extra DashMap ops on every OCC publish and on some OCC reads | **Yes on the OCC path.** Gate the updates on `SpecFence`. WAR retain still needs them for SF. Do not delete `retained_history` itself |
| `PevmEthereum::Evm` is always `MainnetEvm<_, SpecFenceInspector>` and `build_evm` always constructs that inspector | `chain/ethereum.rs` | Inspector object on every EVM. `inspect_run` is not entered unless `use_inspect` | **Yes** for OCC: stock inspector. Type split |
| SLOAD opcode wrapper installed whenever snap mode is ResumePath | `chain/ethereum.rs` `build_evm` | Extra SLOAD prelude on OCC and SF, because the env default is ResumePath and `build_evm` does not see `ConcurrencyMode` | **Yes for OCC.** Pass a mode flag. SF install stays if ResumePath stays |
| SSTORE protocol plant | same function | Off unless inspect / absolute jump / `HANDLER_CAPTURE` | Already off. Delete with those flags |
| `tx_runner::NoBeneficiaryHandler` | `tx_runner.rs` (296, all new) | Every Ethereum tx, both modes: `pending_resume_armed()` once per tx, `nested_ordered_admit_stash_armed()` on every `CALL`, plus the yield-resume match | Beneficiary skip is required (pevm pays the reward outside). **Remove** the resume/yield/nested checks from the OCC handler. **DECIDE** whether SF keeps yield as the RAW deadlock release |
| `AccountHints::build` for every parallel block | `pevm.rs` | Envelope walk even for OCC | **Yes** for OCC. Hints are a SpecFence input |
| `chain.rs` `run_pevm_tx(..., use_inspect)` | +16 in the trait | Plumbing | Shrinks when the inspector split lands |

`rise.rs` gained 35 lines for the same inspector / handler pattern. Treat it like `ethereum.rs`.

## 4. Deletion order

Each step should leave `cargo +stable check -p pevm --tests` green. After step 2 also build `--example specfence_inflation_dig`. Do not start until the other two agents have merged.

| Step | What | Lines removed (estimate) | Check |
| --- | --- | ---: | --- |
| 1 | `computer.rs`, `kernel.rs`, their `mod` lines | 146 | **Already shown.** Commenting both mods left `--tests` green on 2026-09-25, then the edit was reverted |
| 2 | The 11 dig examples, `specfence_busy_stall_report.py`, and the four `lab/**/*.py` scripts listed above. No lib edit | 10,238 | Lib tests unchanged. `--examples` should build only `specfence_inflation_dig` |
| 3 | `busy_stall.rs`, `ideal_prox.rs`, `process.rs`, `decision_field.rs`, `heat.rs`, and their call sites. Drop `ConcurrencyMode::Pcc` only if step 3’s test compile says nothing else references it. One integration test and `specfence_mainnet_sweep` (already gone in step 2) mention `Pcc` | ~2,400 including call sites | `--tests`. Expect failures only in tests that named those types; delete those tests in this same commit |
| 4 | `finegrain.rs`, `Pevm` finegrain fields and setters, `lib.rs` re-exports of fine-grain types | ~2,100 | `--tests`. Grep `FineGrain` and delete the remaining test uses in this commit |
| 5 | Cargo feature `inflation-alloc` and `InflationAlloc`. Leave the other inflation counters | ~80 | `--tests` and the inflation example |
| 6 | Compiler-named dead methods in section 2.5. Remove `#![allow(dead_code)]` on `learner.rs` and delete what the compiler then names. Do not `deny(dead_code)` until the list is empty | ~1,000–1,500 (not measured line-by-line) | `--tests` |
| 7 | OCC-only cost from section 3, except `DataNest`: skip SF table build when mode is Occ, delete OCC phase `Instant`s, stop installing the SLOAD wrapper and the inspector for Occ, gate reader-index / residual / retained-history updates on SpecFence | a few hundred lines, plus a large drop in what OCC *runs* | Re-run the existing seq≡par tests. Behavior of OCC receipts must not change |
| 8 | Stop. The DECIDE list is not a silent delete | — | Needs an explicit call |

Steps 1–7 remove on the order of **16k lines** of source, examples, and scripts. The largest single files that would still be present are `policy.rs` (6,857), `boundary.rs` (3,556), `rem.rs` (3,128), `runnable_set.rs` (2,430), and `vm.rs` (the fork added 4,766 lines there; step 7 peels the OCC tax, it does not restore upstream `vm.rs`).

A later split of `boundary.rs` / inspect-only `rem.rs`, if the user wants jump and inspect gone, is about **2,000 lines** of `boundary.rs` plus the unused `rem` methods. That is not in the 16k, because ResumePath is still the default repair path.

## 5. DECIDE — needs a call before any of this is deleted

1. **Global Ideal-ready pool, Ideal-timed late split, AdmitShard pop law, IdleStealWake, QuietExit.** They are the default `runnable_set` / `worker` behavior. PR #57’s own note says the remote-Ideal gate is FAIL. Deleting them rewrites the pop law back to index bands (`both flags = 0`). That is a scheduler change, not dead code. `admit_deque.rs` still has to exist if AdmitIndep stays.
2. **IntraPatch** (`apply_pending_patches` on every pick and at release). Still runs. Removing it changes which tx is dropped from the indep queue.
3. **Cross-prior arm install** (`begin_from_prior`, thin `plant_wait_edges`) versus the spine rule that a prior is radar-only. Both are live. Pick one.
4. **`boundary.rs` ResumePath snap and `SPECFENCE_BIND_SNAP_JUMP` default-on**, including the SLOAD wrapper `build_evm` installs for every EVM. Keep the repair snap, or turn the unset default to Off and stop replacing SLOAD.
5. **`SPECFENCE_NESTED_BIND` default-on** and the per-`CALL` TLS check in `tx_runner`.
6. **`tx_runner` YieldWait.** It is the deadlock release for in-frame RAW wait. The successful wait does not use it. Say whether that release stays.
7. **`DataNest`.** Correctness guard on `MvMemory.data` for both modes. Removing it brings back the re-entrant DashMap panic unless the lookup pattern is also gone.
8. **`ConcurrencyMode::Pcc` and `lane.rs`.** Heat is safe to delete (step 3). PCC itself still has `maybe_wait_pcc` and a test. Delete the mode, or keep it as a third protocol.
9. **Region Learn Avoid v1/v2** if PRs #58 or #59 merge. They are not in this tree. Default-on, and v2 was a failed gate.

## 6. What was checked

- `scripts/specfence_flag_inventory.py` logic matches a full-tree scan: 54 `SPECFENCE_*` tokens under `crates/`, `scripts/`, and `bins/`.
- `cargo +stable check -p pevm --tests` with `mod computer` and `mod kernel` removed: finished, warnings only, rustc 1.98.1. The edit was reverted and is not in the diff.
- The same check on the restored tree produced the never-used list in section 2.5.
- No product `.rs` file is modified in this PR.
