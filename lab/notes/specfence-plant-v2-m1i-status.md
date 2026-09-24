# Plant v2 M1i status — post-SSTORE write jump + default valued CallOutcome

**Date:** 2026-09-05 (Asia/Shanghai)  
**Branch:** `specfence` @ fengjy73/pevm  
**Parent tip:** M4 `f6a0c8b`  
**Scope:** SpecFence path only; OCC/PCC unchanged; M2/M3/M4 lean/full intact

## Dual goals — honest outcomes

### A — Post-SSTORE gas-equal absolute jump

| Piece | Status |
|-------|--------|
| `BoundarySnapshot.post_sstore` | **Landed** — set in Inspector `step_end` when last opcode was SSTORE (gas includes dynamic ~20k) |
| `PartialRetryState.post_sstore_gases` → `StorageWriteReplay.gas_remaining_after` | **Landed** — FIFO/sticky fill at finalize `note_write_replay` |
| `write_prefix_jump_is_safe` | **Landed** — requires non-empty `write_replays` + post-SSTORE evidence; refuses storage-bearing journal blob |
| Controlled journal slot replay | **Landed** — `apply_write_replays` mutates in-journal slots only (**no** `load_account` / Db re-entry; no present_values dump) |
| `jump_is_safe` Write gate | Write effects without replays still forbid; storage `write_replays` go through M1i gate; account-only `prefix_writes` do **not** block M1g Storage-read jumps |
| Default-on when safe | **Yes** — anti-livelock `jump_disabled` unchanged |
| Integration | `specfence_m1i_write_prefix_absolute_jump_seq_eq_par` — `absolute_jump_applied > 0`, seq≡par, resume steps < cold |

### B — Default valued CallOutcome

| Piece | Status |
|-------|--------|
| Mid-exec valued short-circuit | **Opt-in** `SPECFENCE_VALUED_CALL_CACHE=1` (default off). `transfer_loaded` path landed; default-on still seq≠par on some RewindTo schedules |
| Shared-account WW livelock | Identified: shared outer/inner valued transfers WW-livelock. Unique pairs avoid hang but not all seq≠par |
| Absolute jump + valued `call_outcomes` | **Still forbidden**; zero-value CALL-boundary jump unchanged (M1g) |
| Integration | `specfence_m1i_valued_nested_call_resume` — hang-free RewindTo + prefix credit + seq≡par under default (override off) |

## ERC-20 transfer L1?

**Partial, not full.** Write-prefix absolute jump now covers SSTORE-then-hot-BALANCE probes (gas-equal post-SSTORE snap + write_replays). Valued nested CALL short-circuit works default-on when caller/callee Basics are not heavily co-written. A full ERC-20 `transfer` still combines:

1. Multiple SSTOREs (balances + allowances),  
2. LOG Transfer,  
3. Often nested/valued external calls,  

under shared storage slots (the exact WW shape that livelocks naive valued override). **No mainnet TPS claim.** M1i is an L1 plant expansion, not a 7-block sweep flip.

## What still works (regression)

- M1f BalanceProbe absolute jump  
- M1g Storage-read absolute jump (gate carefully preserved)  
- M1g zero-value nested CallOutcome / CALL-boundary  
- M2 park/steal; M3 prior Bind; M4 lean/full  

## Remaining gaps → 7-block sweep readiness

1. LOG / multi-SSTORE write-prefix without blob poison.  
2. Valued CALL-boundary jump that is hang-free on shared ERC-20 slots (or region-level serialization for those Basics).  
3. Combine write-prefix jump + nested CallOutcome on one resume.  
4. Re-measure 7-block sweep after (1)–(3); do not expect SF/OCC ≫ 1 from M1i alone.  
5. Intermittent `inspect_run` hang under high parallel test load — retry isolated (`--test-threads=1`).

## Files

- `crates/pevm/src/specfence/boundary.rs` — `post_sstore`, `jump_is_safe` M1i, `apply_write_replays`, valued default-on  
- `crates/pevm/src/specfence/rem.rs` — `post_sstore_gases`, gas fill on `note_write_replay`  
- `crates/pevm/src/vm.rs` — finalize write_replay comment  
- `crates/pevm/tests/specfence.rs` — M1i write + valued tests  
- `lab/notes/specfence-plant-v2-m1i-status.md` — this note  

## Success gate

- Lib: `specfence::boundary::m1c_tests` green (incl. write-prefix accept/reject)  
- Integration: M1i write (jump>0), M1i valued (RewindTo hang-free), M1g Storage/nested green  
- Full `specfence` suite: green per-test with `--test-threads=1` (retry flaky inspect once)
