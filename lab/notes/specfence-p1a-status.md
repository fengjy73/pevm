# SpecFence P1a status

**Date:** 2026-09-04  
**Branch:** `specfence`  
**Contract:** `specfence-rem-spec-v1.md` §10 P1a

## Landed

| Item | Status |
|------|--------|
| `rem.rs` region events / effect ordinal / checkpoint counters | yes (Phase-2 prep) |
| `dag.rs` revokeable wait-flags + soft waiters + ready hints | yes (minimal Ĝ) |
| `resolve.rs` WaitHard/Bind/SpecRead/π + thresholds | yes |
| Bayes `P_conflict` + revoke + `P_bind_useful` | yes |
| Metrics Spec v1 set | yes |
| `readers[ℓ]` index on record/clear | yes |
| `InvalidateSelective` + aborted-incarnation detection | yes |
| Per-location `validate_location` API | yes |
| vm π on world-state reads (beneficiary excluded) | yes |
| pevm validation: conflict Bayes, FullRetry, selective, fence, revoke | yes |
| Prior incarnation residual WS → Bind/WaitHard placeholders | yes (Bohm-lite) |
| Tests 1–6 (§9) | green |

## Gaps → P1b / P2

- Mainnet sweep v4 figures; prove SF@8 ≥ OCC@8 or explain via `tx_full_retry`.
- Explicit wave ready-queue over Ĝ components (Phase-1 approximates via sticky+revoke).
- EarlyVal default scheduling under core pressure.
- PartialRetry from checkpoints (revm re-entry).
- Richer structural Bayes (co-access); cost-based π.
- Cold-path `--region-sample` opt-in.

## Non-goals honored

- Did not touch `fengjy73/pevm-specfence-server`.
- OCC/PCC behavioral compatibility preserved (OCC still records `occ_aborts`).
