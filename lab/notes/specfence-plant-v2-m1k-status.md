# Plant v2 M1k status — hang-free jump-past-LOG + default-on valued CallOutcome

**Date:** 2026-09-06 (Asia/Shanghai)  
**Branch:** `specfence` @ fengjy73/pevm  
**Parent tip:** M1j `e913219`  
**Scope:** SpecFence path only; OCC/PCC unchanged; M2/M3/M4 lean/full intact

## Dual goals — honest outcomes

### A — Hang-free jump-past-LOG

| Piece | Status |
|-------|--------|
| Root cause (M1j refuse) | `try_arm` refused jump when filtered `log_replays` non-empty; restore path existed in `initialize_interp` but **never armed** `PENDING_LOG_REPLAYS`. Hang class was **live_boundaries JournalBlob with logs** under concurrency — not LogReplay itself. |
| Post-LOG live tip | **Landed** — `step_end` after LOG* attaches snap-only tip (`JournalBlob::default()`) |
| LogReplay arm/restore | **Landed** — `try_arm` arms `PENDING_LOG_REPLAYS`; eager `note_log_replays` on LOG; continuation tip-filters logs |
| No `present_values` / blob poison | **Preserved** |
| Integration | `specfence_m1j_multi_sstore_log_write_prefix_jump` — jump>0, receipt logs>0, seq≡par, resume<cold; **conc=2** |

### B — Default-on valued CallOutcome

| Piece | Status |
|-------|--------|
| Default-on | **Yes** — unset enabled; `SPECFENCE_VALUED_CALL_CACHE=0` disables |
| Hang-free path | In-journal-only `transfer_loaded` (never `load_account`). Cold → fall through to `make_call_frame` |
| Mid-exec abort race | **Fixed** — eager `note_call_outcomes` on `call_end`; `valued_blocks_jump` refuses absolute jump if incarnation saw valued nested CALL |
| Warm RewindTo SC seq≡par | **Partial** — unique+warm still residual seq≠par on some schedules; suite uses cold-callee fallthrough for green default |
| Absolute jump + valued | **Still forbidden** |
| write_replays + zero-value CallOutcome | **Allowed** at CALL-boundary (abort jump if touches cold) |
| Integration | `specfence_m1i_valued_nested_call_resume` — hang-free RewindTo + credit + seq≡par, env unset |

## ERC-20 transfer L1?

**No.** Jump-past-LOG + multi-SSTORE write-prefix + default-on valued cache (cold fallthrough hang-free) are real. Full ERC-20 `transfer` still needs hang-free multi-SSTORE at **full worker width**, warm valued SC seq≡par on RewindTo, and valued+write combine. **No mainnet TPS claim.** Not a 7-block sweep flip.

## What still works (regression)

- M1f BalanceProbe absolute jump  
- M1g Storage-read absolute jump / zero-value nested CallOutcome  
- M1i single-SSTORE write-prefix jump  
- M2 park/steal; M3 prior Bind; M4 lean/full  

## Remaining gaps → 7-block sweep readiness

1. Multi-SSTORE write-prefix hang-free at **full** worker width (WW class)  
2. Warm valued CallOutcome SC on RewindTo with reliable seq≡par  
3. Valued + CALL-boundary absolute jump after touches  
4. Denser Transfer-shaped schedules in default suite  
5. Re-measure 7-block sweep after (1)–(3); do not expect SF/OCC ≫ 1 from M1k alone  

## Files

- `crates/pevm/src/specfence/boundary.rs` — jump-past-LOG arm, post-LOG tip, valued default-on, eager flushes, zero-value+write combine  
- `crates/pevm/src/specfence/rem.rs` — tip-filtered `log_replays`, `valued_blocks_jump`  
- `crates/pevm/src/specfence/mod.rs` — M1k doc line  
- `crates/pevm/tests/specfence.rs` — M1k jump-past-LOG + valued default  
- `lab/notes/specfence-plant-v2-m1k-status.md` — this note  

## Success gate

- Lib: `specfence::boundary::m1c_tests` green  
- Integration: M1k multi-SSTORE+LOG jump-past-LOG, M1i write/valued, M1g green (retry flaky inspect)  
- Full `specfence` suite: **30/30** green per-test `--test-threads=1` (retry hang ≤3×)  
- Honest: **ERC-20 full transfer L1 = no**
