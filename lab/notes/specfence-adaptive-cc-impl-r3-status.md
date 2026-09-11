# Adaptive CC Redesign v1 — R3 status (HotSet tune + sweep v8)

**Date:** 2026-09-06 (Asia/Shanghai)  
**Parent:** R0–R2 `a855e14`  
**Sweep:** `lab/notes/mainnet-sweep-v8-status.md`

## Done

- Tuned HotSet: `H_w=8`, `H_a=3`, prior threshold/decay, no begin_block pre-seed.
- Excluded `LazyRecipient` from H_w writer counts.
- Fixed mid-block abort escalate window (was 1/1 ≥0.08 dumping write-sets into HotSet).
- Abort `note_abort` only on conflict locations.
- Full 7-block × {1,2,4,8} × repeats=3 OCC+SpecFence sweep v8.
- SpecFence suite: 22 passed, 13 ignored (M1* research-only).

## Headline

- Mean SF/OCC@8 = **0.325** (v7 ~0.09, v5 ~0.30).
- Wide hotset tiny, wait_hard≈0, lean high; **19434587 SF@8 completes** (v7 hung).
- Gate SF/OCC≥0.95 still miss — remaining Bind/meta vs OCC, not inspect.

## Files

- `crates/pevm/src/specfence/hotset.rs`, `engagement.rs`
- `crates/pevm/src/{vm,pevm}.rs`
- `crates/pevm/tests/specfence.rs`
- `lab/results/mainnet-sweep-v8.*`
