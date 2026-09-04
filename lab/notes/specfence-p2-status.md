# SpecFence P2 status — PartialRetry prototype

**Date:** 2026-09-04 (Asia/Shanghai)  
**Branch:** `specfence`  
**Contract:** `specfence-rem-spec-v1.md` §4.3, §6 PartialRetry, §10 P2  
**Base:** `5c57e32` (P1b)
**Commit:** `da8d0e8`

## Mechanism (semantic PartialRetry)

revm cannot resume mid-bytecode. P2 implements **semantic** PartialRetry:

1. **Checkpoints / `PartialRetryState` (REM)**  
   Per-tx journal of effect ordinals `k`, first-touch map, EarlyVal certifications. Held in `PartialRetryTable` (block-scoped).

2. **EarlyVal under pressure**  
   On SpecRead with high `P_conflict` (or hot Wait), π prefers Bind/WaitHard; after origin record, `maybe_early_val` certifies or aborts early into PartialRetry (force-bind certified-so-far).

3. **`PartialRetry(t, k_fail)` on validation abort**  
   - Partition reads into certified (still valid) vs failed.  
   - `k_fail = min first_k(failed)`.  
   - Safe iff certified non-empty; prefix writes = touched before `k_fail` **and** certified; else suffix.  
   - `invalidate_partial_suffix`: ESTIMATE **only** suffix writes; **no** global `aborted_incarnation` stamp (prefix readers stay valid).  
   - Next incarnation: π **force Bind/WaitHard** on certified-prefix locations.  
   - Metrics: `partial_retry_count++` (not `tx_full_retry`).  
   - Unsafe / empty certified → FullRetry + `partial_retry_fallback_full`.

4. **Secondary**  
   Cold posterior (`< τ_revoke`) skips cold-start WaitHard and revokes sticky Wait.

OCC/PCC unchanged. Did not touch `pevm-specfence-server`.

## Tests

```
cargo test -p pevm --release --config 'profile.release.lto=false' --test specfence -- --test-threads=1
```

16 passed, including:

- `specfence_p2_partial_retry_on_localized_conflict` — `partial_retry_count ≥ 1`, OCC aborts still > 0, sequential ≡ SpecFence  
- `specfence_p2_full_retry_not_always_eq_aborts` — PartialRetry decouples `tx_full_retry` from `occ_aborts`

## Smoke (vs v4)

Blocks 19807137, 19434587, 15199017; cores 1,8; repeats 1.

| block | v4 full_retry @8 | v5 full_retry @8 | v5 partial_retry @8 |
|-------|------------------|------------------|---------------------|
| 19807137 | 1872 | **0** | 1962 |
| 19434587 | 136 | **0** | 155 |
| 15199017 | 8 | **0** | 9 |

TPS mixed → **no full 7-block v5**. Details: `lab/notes/mainnet-sweep-v5-smoke.md`.

## Files touched

- `crates/pevm/src/specfence/rem.rs` — `PartialRetryState` / `PartialRetryTable` / plan  
- `crates/pevm/src/specfence/metrics.rs` — `partial_retry_count`, `partial_retry_fallback_full`  
- `crates/pevm/src/specfence/mod.rs` — wire ctx  
- `crates/pevm/src/mv_memory.rs` — `invalidate_partial_suffix`, `origins_still_valid`  
- `crates/pevm/src/vm.rs` — journal, force-bind, EarlyVal  
- `crates/pevm/src/pevm.rs` — try_validate PartialRetry path  
- `crates/pevm/tests/specfence.rs` — P2 tests  
- `crates/pevm/examples/specfence_mainnet_sweep.rs` — export metrics  
- `SPECFENCE.md`, lab notes / smoke results

## Gaps → later

- Wave ready-queue over Ĝ components still approximate (sticky+revoke).  
- Over-Wait / WaitHard tax still dominates TPS on hot blocks.  
- True mid-tx resume still non-goal (revm).  
- Optional full 7-block v5 when TPS path improves.
