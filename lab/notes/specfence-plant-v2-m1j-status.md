# Plant v2 M1j status — multi-SSTORE/LOG write-prefix + valued CallOutcome

**Date:** 2026-09-05 (Asia/Shanghai)  
**Branch:** `specfence` @ fengjy73/pevm  
**Parent tip:** M1i `a2ce360`  
**Scope:** SpecFence path only; OCC/PCC unchanged; M2/M3/M4 lean/full intact

## Dual goals — honest outcomes

### A — Multi-SSTORE + LOG write-prefix jump

| Piece | Status |
|-------|--------|
| Multi-SSTORE `write_replays` + gas tip gate | **Landed** — `write_prefix_jump_is_safe`; refuse early post-SSTORE tip when `gas_remaining > last post-SSTORE gas` (avoids applying all replays then re-exec later SSTOREs → seq≠par); `opcode_steps` cap 512 |
| LOG without storage blob poison | **Partial** — `LogReplay{pc,log}` capture (not live_boundaries blob — hang-prone). **Jump-past-LOG absolute jump refused** in `try_arm`. Integration lands jump **before** LOG; LOG runs after tip → receipts seq≡par |
| No `present_values` dump / Db poison | **Preserved** — write_replays + in-journal slot update + MV republish (M1i) |
| Integration | `specfence_m1j_multi_sstore_log_write_prefix_jump` — `absolute_jump_applied > 0`, seq≡par, resume < cold; runs at **concurrency=2** (full-width inspect_run hangs more often on multi-SSTORE — same WW class as M1i notes) |

### B — Default-safe valued CallOutcome

| Piece | Status |
|-------|--------|
| Root cause (M1i) | Shared outer/inner: `load_account` in Inspector → pevm `maybe_wait` → WW livelock |
| Fix | In-journal-only `transfer_loaded` (never `load_account`); skip SC when accounts not warm; keep cached outcome gas |
| Default-on | **No** — residual seq≠par on some RewindTo schedules; remains **opt-in** `SPECFENCE_VALUED_CALL_CACHE=1` |
| Absolute jump + valued CallOutcome | **Still forbidden** |
| write_replays + nested CallOutcome on one jump | **Forbidden** (unit gate) |
| Integration | Hang-free RewindTo via `specfence_m1i_valued_nested_call_resume` under default (valued off) |

## ERC-20 transfer L1?

**No.** Multi-SSTORE write-prefix jump with trailing LOG (after tip) is real. Jump-past-LOG restore and default-on valued CALL are not. Full ERC-20 `transfer` is **not** claimed. **No mainnet TPS claim.** Not a 7-block sweep flip.

## What still works (regression)

- M1f BalanceProbe absolute jump  
- M1g Storage-read absolute jump / zero-value nested CallOutcome  
- M1i single-SSTORE write-prefix jump  
- M2 park/steal; M3 prior Bind; M4 lean/full  

## Remaining gaps → 7-block sweep readiness

1. Hang-free **jump-past-LOG** with correct single-emit log replay under full concurrency  
2. Default-on valued CallOutcome with seq≡par on shared RewindTo schedules  
3. Safe combine: write-prefix jump + nested CallOutcome  
4. Multi-SSTORE write-prefix hang-free at full worker width  
5. Re-measure 7-block sweep after (1)–(4); do not expect SF/OCC ≫ 1 from M1j alone  

## Files

- `crates/pevm/src/specfence/boundary.rs` — multi-SSTORE gas tip gate, LogReplay, refuse jump-past-LOG, valued opt-in  
- `crates/pevm/src/specfence/rem.rs` — `LogReplay` / `log_replays` on continuation  
- `crates/pevm/src/specfence/mod.rs` — M1j doc line  
- `crates/pevm/tests/specfence.rs` — M1j multi-SSTORE+LOG (`run_mode_conc` width 2)  
- `lab/notes/specfence-plant-v2-m1j-status.md` — this note  

## Success gate

- Lib: `specfence::boundary::m1c_tests` green (multi-SSTORE accept; valued jump reject; combine reject)  
- Integration: M1j multi-SSTORE+LOG (jump>0), M1i write/valued, M1g green (retry hang once)  
- Full `specfence` suite: **30/30** green per-test with `--test-threads=1` (retry flaky inspect up to 3×)  
- Honest: **ERC-20 full transfer L1 = no**
