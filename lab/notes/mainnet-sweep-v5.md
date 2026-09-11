# Mainnet sweep v5 (after P2 PartialRetry)

**Date:** 2026-09-04 (Asia/Shanghai)  
**Base:** `da8d0e8` (P2 PartialRetry) / docs `0a58b5c`  
**Sweep:**
```
cargo run -p pevm --release --config 'profile.release.lto=false' --example specfence_mainnet_sweep -- \
  --out lab/results/mainnet-sweep-v5.json
```

Same 7 blocks / cores 1,2,4,8 / 3 repeats / `reset_heat` each repeat. P2 metrics (`partial_retry_count`, `partial_retry_fallback_full`, `tx_full_retry`, `bind_hits`, `wait_hard_count`, `spec_read_count`, …) exported in JSON/CSV. Figures: `lab/figures-v5/`. CSV: `lab/results/mainnet-sweep-v5.csv` (273 rows, all `ok`).

## SpecFence vs OCC @8 TPS (mean of 3) vs v4

| block | OCC v4 | SpecFence v4 | OCC v5 | SpecFence v5 | SF/OCC v4 | SF/OCC v5 |
|-------|--------|--------------|--------|--------------|-----------|-----------|
| 13217637 | 246031 | 68431 | 260392 | 55752 | 0.278 | 0.214 |
| 14029313 | 223794 | 72699 | 232151 | 68653 | 0.325 | 0.296 |
| 14383540 | 140212 | 45845 | 150134 | 45365 | 0.327 | 0.302 |
| 14683600 | 87788 | 41890 | 87787 | 41863 | 0.477 | 0.477 |
| 15199017 | 223265 | 62159 | 206912 | 63970 | 0.278 | 0.309 |
| 19434587 | 30886 | 8323 | 34464 | 12598 | 0.269 | 0.366 |
| 19807137 | 58392 | 7354 | 58264 | 6879 | 0.126 | 0.118 |

- Arithmetic mean SF/OCC @8: **0.297** (v4 was **~0.297**; v3 ~0.65). Geometric mean: **0.276** (v4 ~0.279).
- **No block** has SpecFence mean TPS ≥ OCC @8. Exit criterion **not met**.
- Absolute OCC TPS moved slightly vs v4 (machine noise); within-sweep SF/OCC is the honest comparison.
- Per-block deltas mixed: **19434587** improved (0.269→0.366), **15199017** slightly up (0.278→0.309); **13217637** down (0.278→0.214); hot **19807137** still worst (0.126→0.118). Mean ratio **unchanged** vs v4.

## SpecFence @8 P2 metrics (v5 mean)

| block | abort_rate | tx_full_retry | partial_retry | pr_fb | bind_hits | wait_hard | spec_read | sel_inv | wait_adm | occ_aborts |
|-------|------------|---------------|---------------|-------|-----------|-----------|-----------|---------|----------|------------|
| 13217637 | 0.006 | **0.0** | 6.7 | 0.0 | 2.0 | 1028 | 2393 | 18 | 945 | 6.7 |
| 14029313 | 0.021 | **0.0** | 15.3 | 0.0 | 1.3 | 531 | 2980 | 44 | 276 | 15.3 |
| 14383540 | 0.033 | **0.0** | 24.0 | 0.0 | 21.7 | 1252 | 3203 | 162 | 545 | 24.0 |
| 14683600 | 0.083 | **0.0** | 54.7 | 0.0 | 66.3 | 1385 | 4057 | 218 | 236 | 54.7 |
| 15199017 | 0.008 | **0.0** | 7.0 | 0.0 | 2.3 | 851 | 3131 | 35 | 622 | 7.0 |
| 19434587 | 0.382 | **0.0** | 149.0 | 0.0 | 125.3 | 3522 | 6119 | 598 | 426 | 149.0 |
| 19807137 | 2.618 | **0.0** | 1864.0 | 0.0 | 24.7 | 10799 | 6845 | 3863 | 1262 | 1864.0 |

Notes:

- **`tx_full_retry = 0` on every block**; `partial_retry_count == occ_aborts` (`pr/aborts = 1.00`, `pr_fb = 0`). P1b’s FullRetry==aborts identity is broken as intended — every abort is PartialRetry (certified-prefix Bind reexec).
- Bind hits are higher than v4 on several blocks (e.g. 19434587: 32.7→125.3; 14683600: 31.7→66.3) but still sparse relative to abort volume on the hot block (19807137: 24.7 binds vs 1864 partial retries).
- WaitHard remains large and grew on the hot path (19807137: 3956→10799). SpecRead counts dropped on that block (14264→6845) while WaitHard rose — path mix shifted, not removed.
- `selective_fallback_full = 0` everywhere (same as v4).

## Honest analysis — did P2 close the gap?

**No on throughput.** Mean SF/OCC @8 is still **~0.30** (identical to v4 within noise). PartialRetry successfully **replaces FullRetry accounting**, but does not move the exit criterion.

Attribution using metrics:

1. **FullRetry eliminated (metric win).** `tx_full_retry=0`, `partial_retry=occ_aborts`, `pr_fb=0` across all 7 blocks. The P2 prototype path is live and dominant.
2. **TPS did not follow.** Mean ratio flat vs v4; no block SF≥OCC. Cold/low-abort blocks still trail OCC ~2–3.5× — the remaining tax is **not** whole-tx re-exec bookkeeping.
3. **Over-Wait / WaitHard still dominates.** On 19807137 WaitHard ~11k with abort_rate 2.62; on colder blocks abort_rate is already tiny (0.006–0.08) yet SF/OCC stays ~0.2–0.5. PartialRetry fixes the abort *shape* without removing WaitHard serialization / SpecRead path overhead.
4. **Bind helps locally, not enough globally.** Bind hits rose where PartialRetry runs (19434587 best SF/OCC lift), but hot-block binds stay << partial retries, so certified-prefix reuse does not amortize the storm.
5. **Smoke prediction held.** v5-smoke already showed FullRetry→0 with mixed/noisy TPS; full 7-block confirms: clear instrumentation win, no uniform TPS win.

Exit criterion (SF mean ≥ OCC **or** explained by FullRetry): FullRetry is **gone**, so the remaining gap is **no longer explained by FullRetry** — it is explained by **over-Wait / WaitHard + SpecRead overhead** (and Bind miss rate under high abort). Next levers: ready-queue / wave scheduling, WaitHard admission tightening, Bind hit-rate on hot blocks — not more PartialRetry plumbing.

## Figures

- `lab/figures-v5/block_<id>_tps_abort.png` (×7)
- `lab/figures-v5/overview_tps_abort.png`
- `lab/figures-v5/specfence_wait_vs_speculate.png`
- `lab/figures-v5/specfence_wait_hard_vs_spec_read.png`
- `lab/figures-v5/specfence_p1a_metrics_overview.png`
- `lab/figures-v5/specfence_partial_vs_full_retry.png` (new P2)
- `lab/figures-v5/specfence_p2_metrics_overview.png` (new: partial / full / pr_fallback / bind vs cores)

## Artifacts

- JSON: `lab/results/mainnet-sweep-v5.json`
- CSV: `lab/results/mainnet-sweep-v5.csv`
- Log: `lab/notes/mainnet-sweep-v5.log`
