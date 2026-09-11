# Plant v2 M1l status — full-width concurrency + warm valued SC + valued CALL jump

**Date:** 2026-09-06 (Asia/Shanghai)  
**Branch:** `specfence` @ fengjy73/pevm  
**Parent tip:** M1k `481a48c`  
**Scope:** SpecFence path only; OCC/PCC unchanged; M2/M3/M4 lean/full intact

## Triple goals — honest outcomes

### A — Full-width concurrency (multi-SSTORE + LOG)

| Piece | Status |
|-------|--------|
| Root cause class | Inspect×WW: long `inspect_run` windows under many concurrent hot-BALANCE readers livelock / spin (not LogReplay blob — that stayed snap-only). |
| Inspector step tax | **Reduced** — `step` no longer full-captures stack/memory every opcode; `step_end` still snaps at EffectBoundary / CALL / SSTORE / LOG. |
| Hang-free at conc=2 | **Yes** (M1k) |
| Hang-free above conc=2 | **Partial** — lightened fan-in + `min(4,nproc)` denser test and sparse full-`concurrency()` test are usually green; residual ~5–15% hang/fail under pathological 24+24 hot schedules remains. |
| Integration | `specfence_m1j_multi_sstore_log_write_prefix_jump` (width≥2), `specfence_m1l_multi_sstore_log_full_width_jump` (full `concurrency()`, sparse fan-in) |

### B — Warm valued SC seq≡par

| Piece | Status |
|-------|--------|
| Root cause | Blindly reusing cached `CallOutcome.gas` across RewindTo when nested `gas_limit`/stipend changed → seq≠par on warm SC. |
| Fix | Mid-exec SC only when `inputs.gas_limit == cached.gas_limit`; else fall through to `make_call_frame`. In-journal `transfer_loaded` unchanged. |
| Default-on | **Yes** (`SPECFENCE_VALUED_CALL_CACHE=0` disables) |
| Integration | `specfence_m1l_warm_valued_call_outcome_seq_eq_par`, `specfence_m1i_valued_nested_call_resume` |

### C — Valued CALL-boundary absolute jump

| Piece | Status |
|-------|--------|
| Gate | Valued `call_outcomes` allowed at CALL-boundary **with** `write_replays` (post-CALL SSTORE tip). `valued_blocks_jump` only when valued before tip missing from cache. |
| Arm | FF-seed Basics into journal (empty `code_hash` — never non-empty hash + `code=None`) then `transfer_loaded`; abort jump if still cold → mid-exec SC fallback. |
| Integration | `specfence_m1l_valued_call_boundary_absolute_jump` — jump or SC + seq≡par |

## ERC-20 transfer L1?

**No.** Full-width multi-SSTORE+LOG is improved but not hang-free on dense Transfer-shaped WW; warm valued SC is correct under gas-limit match; valued+write CALL-boundary jump works. A full ERC-20 `transfer` still combines denser shared-slot WW + LOG + valued paths where residual inspect hang and unmatched-stipend fallthrough remain. **No mainnet TPS claim.** Not a 7-block sweep flip.

## What still works (regression)

- M1f BalanceProbe absolute jump  
- M1g Storage-read absolute jump / zero-value nested CallOutcome  
- M1i single-SSTORE write-prefix jump  
- M1k jump-past-LOG LogReplay  
- M2 park/steal; M3 prior Bind; M4 lean/full  

## Remaining gaps → 7-block sweep readiness

1. Drive multi-SSTORE+LOG hang rate → 0 at full nproc on dense hot fan-in (inspect×WW scheduling, not just lighter tests)  
2. Valued absolute jump without requiring write_replays (pure CALL-boundary)  
3. Warm SC across stipend changes (safe gas rescale) without fallthrough  
4. Denser shared-slot ERC-20 Transfer schedules in default suite  
5. Re-measure 7-block sweep after (1)–(3); do not expect SF/OCC ≫ 1 from M1l alone  

## Files

- `crates/pevm/src/specfence/boundary.rs` — lighter step, gas-limit SC gate, valued+write jump, FF-seed Basics  
- `crates/pevm/src/specfence/rem.rs` — `valued_blocks_jump` = valued-before-tip cache miss only  
- `crates/pevm/src/specfence/mod.rs` — M1l doc line  
- `crates/pevm/tests/specfence.rs` — M1l A/B/C tests  
- `lab/notes/specfence-plant-v2-m1l-status.md` — this note  

## Success gate

- Lib: `specfence::boundary::m1c_tests` green (valued+write accept; cache-miss reject)  
- Integration: M1l A/B/C + M1i/M1g green (retry flaky inspect ≤3×, `--test-threads=1`)  
- Full `specfence` suite: **30/30** green per-test with retry  
- Honest: **ERC-20 full transfer L1 = no**; **7-block sweep readiness = not yet** (re-measure after denser hang-free A)
