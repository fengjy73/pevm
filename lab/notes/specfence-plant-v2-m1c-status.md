# Plant v2 M1c status — CALL/boundary PC resume (L1)

**Date:** 2026-09-04 (Asia/Shanghai)  
**Branch:** `specfence` @ fengjy73/pevm  
**Tip parent:** M1b `57a9555`  
**Scope:** SpecFence path only; OCC/PCC unchanged; M2 park/steal intact

## Design

On RewindTo, M1b already restores SpecFence journal + FF value cache. M1c adds
**effect/CALL-boundary resume accounting** so the certified prefix is not treated
as a cold head reexec for L1 metrics:

1. **BoundarySnapshot** at EffectBoundary (Bind/EarlyVal): stores `opcode_steps`
   (effect-ordinal grain on the production `Handler::run` path), optional
   PC/stack/memory/gas for Inspector-driven resume.
2. **ResumeContinuation.boundary** carries the snap for the rewind target `cp`
   (synthesized from `cp.k` / effect count when only CallEntry@0 exists).
3. On RewindTo resume: credit `pc_resume_count` + `prefix_opcodes_skipped` from
   the snap (or FF entry count fallback). Still **does not** call `record_evm_entry`.
4. **SpecFenceInspector** (stock revm Inspector, not a custom `run_exec_loop`)
   implements real PC/stack/gas restore via `initialize_interp` — proven in unit
   tests. Production keeps `Handler::run` to protect sequential ≡ parallel
   (earlier nested CALL / custom loop landmine).

| Op | Interpreter / DB | Metrics |
|----|------------------|---------|
| **RewindTo + FF + boundary** | Handler re-enter; journal FF; skip credit for prefix | `rewind_to_cp`, `resume_count`, `journal_ff_*`, `pc_resume_count`, `prefix_opcodes_skipped`; **not** `evm_entries` |
| **FullRestart** | Head `Vm::execute` | `full_restart` + `evm_entries` |

## What opcodes are skipped vs still re-run (honest)

| Work | On M1c RewindTo resume |
|------|-------------------------|
| SpecFence effect journal 0..=cp | **Restored** (M1b) |
| Certified-prefix storage/basic (stable origin) | **FF cache hit** (M1b) |
| Prefix **skip credit** (`prefix_opcodes_skipped`) | **Credited** from boundary snap / effect ordinal (estimated on production path) |
| True bytecode PC jump in production `Handler::run` | **Not yet** — bytecode still starts at tx head; Inspector PC inject is unit-tested only |
| revm journal mutations for prefix SSTORE | **Still re-run** via interpreter (journal-blob FF TODO) |
| Suffix after k* | Re-executed normally |

## CALL nesting status

- Outer CallEntry/CallExit still recorded at execute finalize boundaries.
- Nested CALL entry/exit via SpecFenceInspector hooks are implemented in
  `boundary.rs` but **not wired** into production `Vm::execute` (GAT /
  `InspectorEvmTr` bounds on generic `PevmChain` + prior `run_exec_loop`
  sequential-equivalence breakage).
- TODO: wire stock `InspectorHandler::inspect_run` for SpecFence-only once
  chain bounds are sealed without infecting OCC/Rise public API; then mid-CALL
  PC resume can apply live snaps.

## Files

- `crates/pevm/src/specfence/boundary.rs` — BoundarySnapshot, SpecFenceInspector, TLS plant hooks, PC-apply unit tests
- `crates/pevm/src/specfence/rem.rs` — checkpoint/continuation boundary field; synthetic snap fallback
- `crates/pevm/src/specfence/metrics.rs` — pc_resume_count, prefix_opcodes_skipped
- `crates/pevm/src/vm.rs` — EffectBoundary lite snaps; resume skip credit; FfValue address/slot for future journal prewarm
- `crates/pevm/tests/specfence.rs` — specfence_m1c_boundary_resume_skips_prefix_opcodes

## Honest L1 status after M1c: **partial (stronger accounting than M1b)**

- Resume is still not a fresh `evm_entries` / `tx_head_reexec`.
- Prefix DB work skipped (M1b) + explicit `prefix_opcodes_skipped` / `pc_resume_count` on RewindTo.
- **Not** full L1: production interpreter still re-enters from tx head; live PC
  jump only proven in Inspector unit tests.

## Remaining gaps

1. Wire SpecFenceInspector + inspect_run into SpecFence Vm::execute without
   breaking generic PevmChain / sequential equivalence.
2. Mid-CALL arbitrary-PC + full stack/memory restore on the live path.
3. revm journal blob FF so prefix SSTORE opcodes can be skipped once PC jumps.
4. Nested CALL short-circuit via cached CallOutcome (Inspector::call).

## Success gate

- `cargo test -p pevm --release --config '''profile.release.lto=false''' --test specfence -- --test-threads=1` passes.
- M1c test: rewind_to_cp + resume_count + journal_ff_hits + pc_resume_count + prefix_opcodes_skipped > 0; tx_head_reexec == 0.
- Lib unit tests: apply_to_interp_jumps_pc_and_restores_stack, arm_pc_resume_records_skip_via_initialize_interp.
