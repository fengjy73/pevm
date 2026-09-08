# Plant v2 M1d status — live inspect_run / PC skip on production path

**Date:** 2026-09-04 (Asia/Shanghai)  
**Branch:** `specfence` @ fengjy73/pevm  
**Tip parent:** M1c `d48ce99`  
**Scope:** SpecFence path only; OCC/PCC unchanged; M1b journal FF + M2 park/steal intact

## Design

1. **Ethereum `MainnetEvm` carries `SpecFenceInspector`** (`build_mainnet_with_inspector`).
2. **`PevmChain::run_pevm_tx(use_inspect)`** — SpecFence uses stock `InspectorHandler::inspect_run` via a **MainnetHandler-shaped** `NoBeneficiaryHandler` (`EvmTrError` bounds). Earlier C-generic Handler+inspect **livelocked** conflict schedules.
3. **OCC/PCC** keep `Handler::run` (inspector idle).
4. SpecFence execute wraps `inspect_run` in **`with_plant_tls`**: `Inspector::step` counts real opcodes; `LAST_SNAP` holds live PC/stack captures; EffectBoundary cps stay **lite** (M1c-compatible) with `opcode_steps` filled from live step count when available.
5. On RewindTo: credit `pc_resume_count` / `prefix_opcodes_skipped` from boundary snap; **do not** `arm_pc_resume` on the default production path (attaching live snaps into RewindTo targets + absolute_jump livelocked some schedules). Unit tests still prove `apply_to_interp` / `initialize_interp` PC skip.

| Op | Interpreter / DB | Metrics |
|----|------------------|---------|
| **RewindTo** | `inspect_run`; journal FF; skip **credit** from live step count | `inspector_steps`, `inspector_steps_resume`, `pc_resume_count`, `prefix_opcodes_skipped`; **not** `evm_entries` |
| **FullRestart** | Head execute | `full_restart` + `evm_entries` |

## What still re-runs (honest)

| Work | On M1d RewindTo resume |
|------|-------------------------|
| SpecFence effect journal 0..=cp | **Restored** (M1b) |
| Certified-prefix storage/basic FF | **FF cache hit** (M1b) |
| Prefix opcodes (bytecode) | **Still re-run** on default path (credited, not jumped) |
| True PC jump / stack restore | **Unit-tested**; production arm deferred (livelock) |
| revm journal SSTORE for prefix | **Still re-run** (journal-blob FF TODO) |
| Nested CALL CallOutcome cache | **Not yet** |
| Rise `PevmChain` | **Handler::run only** — no SpecFenceInspector |

## Landmine avoided

Custom `run_exec_loop` / naive `InspectorHandler` on a pevm-C-generic Handler **broke sequential ≡ parallel** or livelocked. Fix: stock `inspect_run` + MainnetHandler-shaped error bounds; EffectBoundary cps remain lite (no live snap in repair targets).

## Files

- `crates/pevm/src/chain/ethereum.rs` — SpecFenceInspector Evm; `run_pevm_tx`
- `crates/pevm/src/tx_runner.rs` — NoBeneficiaryHandler + InspectorHandler
- `crates/pevm/src/chain.rs` / `rise.rs` — `run_pevm_tx` (Rise: run-only)
- `crates/pevm/src/specfence/boundary.rs` — inspector, plant TLS, PC apply unit tests
- `crates/pevm/src/vm.rs` — SpecFence inspect path + metrics
- `crates/pevm/src/specfence/metrics.rs` — live/inspector step counters
- `crates/pevm/tests/specfence.rs` — `specfence_m1d_live_inspect_resume_skips_prefix_opcodes`

## Honest L1 status after M1d: **partial (live inspect wired; PC jump not default)**

- Production SpecFence uses stock `inspect_run` with real `Inspector::step` counts.
- Resume skip **credited** from live step grain; `inspector_steps_resume < resume + skipped`.
- Absolute PC jump remains unit-tested / opt-in research (`arm_pc_resume` + journal-blob FF next).

## Remaining gaps

1. Safe production `arm_pc_resume` + journal-blob FF (write-prefix SSTORE).
2. Nested CALL short-circuit via cached `CallOutcome`.
3. Rise SpecFenceInspector wiring.
4. Control-flow validation so live snaps can be repair targets without livelock.

## Success gate

- `cargo test -p pevm --release --config 'profile.release.lto=false' --test specfence -- --test-threads=1` — **21 passed** (sequential ≡ parallel).
- M1d test: rewind + resume + `inspector_steps > 0` + skip credit; `tx_head_reexec == 0`.
