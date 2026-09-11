# Plant v2 M4 status — adaptive low-conflict OCC bypass

**Date:** 2026-09-05 (Asia/Shanghai)  
**Branch:** `specfence` @ fengjy73/pevm  
**Parent tip:** M3 `fbc9c2a`  
**Scope:** SpecFence path only; OCC/PCC unchanged; M2 park/steal + M3 prior Bind intact when **full**

## Goal (iron-law C)

When conflict prior is low, SpecFence runs a **lean** path whose wall-clock ≈ OCC (meta off / minimal): no inspect_run, no Bayes WaitHard/Bind π, no rem checkpoints. When contention signal rises, engage the full plant (Bind/Wait/repair/inspect as M1–M3).

## Exact engagement trigger

### Block start → lean iff **all** hold
1. `last_abort_rate < τ_abort` (`0.05`), where `last_abort_rate = occ_aborts / n_tx` from the previous SpecFence block (cold / `reset_heat` → `0`).
2. `bayes.conflict_mass() < τ_mass` (`0.12`) — mean excess `max(0, P_ℓ − prior)` over tracked locations.
3. `bayes.hot_conflict_count(DEFAULT_TAU) == 0` — nothing would seed Wait.
4. No hinted account (excl. beneficiary) has `writer_count ≥ 2` — multi-writer schedules stay **full** so RewindTo / selective invalidate still run.

Else start **full**. Lean also skips `seed_wait_regions`.

### Mid-block escalate lean → full
When lean and `occ_aborts_so_far / max(1, txs_started) ≥ τ_abort_mid` (`0.08`), flip to full and bump `engagement_switches`. Subsequent `Vm::execute` incarnations use the full plant.

### Lean path (still `ConcurrencyMode::SpecFence`)
| Piece | Lean | Full |
|-------|------|------|
| `inspect_run` / SpecFenceInspector | **off** (`Handler::run`) | on |
| Bayes Wait / choose_resolve / Bind | SpecRead-only | M3 π |
| Hinted Wait admission | off | on |
| Rem checkpoints / EarlyVal | off | on |
| Abort invalidate | OCC ESTIMATE | RewindTo / selective |
| Cascade | SpecFence **fence** (kept) | fence |
| Bayes / rw_prior learn on abort/success | yes (for next block) | yes |

Sequential ≡ parallel is preserved via validate under SpecFence mode.

## Metrics
- `lean_mode_txs` / `full_mode_txs` / `engagement_switches` on `SpecFenceMetrics`.

## Tests
- `specfence_m4_low_conflict_engages_lean` — independent transfers: `lean_mode_txs > 0`, no WaitHard/inspect, seq≡par.
- `specfence_m4_high_conflict_uses_full_plant` — same-sender WW: multi-writer → `full_mode_txs > 0`, seq≡par.
- Unit: cold lean, high abort refuse, mid-block escalate.
- Specfence suite green when run per-test with retry on flaky inspect (`m1c` / occasional p1a/p2); `m1h_*` remain `#[ignore]`.

## Before / after story

| Scenario | Before M4 | After M4 |
|----------|-----------|----------|
| Quiet independent block | Full SpecFence meta (inspect + Bayes decide on every read) even with 0 aborts | Lean ≈ OCC Handler::run; `lean_mode_txs ≈ n_incarnations` |
| Multi-writer / hot Bayes | Full plant | Still full from block start (hint / mass / abort-rate gates) |
| Quiet → unexpected aborts | N/A | Mid-block escalate; fence metrics still move on lean aborts |

**Honest TPS:** may help SF/OCC on quiet / low-contention blocks by removing meta tax. Contended mainnet still needs **M1i** (Storage/nested jump covering abort gas) — M4 does not claim SF/OCC ≫ 1 on the 7-block sweep.

## Remaining gaps
1. **M1i** — write-prefix + valued CALL jump covering ERC-20 abort gas.  
2. **7-block sweep** with `lean_mode_txs`, `full_mode_txs`, `engagement_switches`, `evm_entries`, SF/OCC.  
3. Optional: location-grain engagement (per-ℓ lean) beyond block/mid-block.  
4. Flaky inspect hangs under single-process suite (retry; not introduced as a new correctness bug).

## Files
- `crates/pevm/src/specfence/engagement.rs` — trigger + AdaptiveEngagement  
- `crates/pevm/src/specfence/{mod,metrics,bayes}.rs` — wire, metrics, `conflict_mass`  
- `crates/pevm/src/{vm,pevm}.rs` — lean execute / abort / carry `last_abort_rate`  
- `crates/pevm/tests/specfence.rs` — M4 integration tests  
- `lab/notes/specfence-plant-v2-m4-status.md` — this note  
