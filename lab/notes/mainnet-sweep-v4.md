# Mainnet sweep v4 (after REM P1a plant)

Sweep: `cargo run -p pevm --release --config 'profile.release.lto=false' --example specfence_mainnet_sweep -- --out lab/results/mainnet-sweep-v4.json`

Same 7 blocks / cores 1,2,4,8 / 3 repeats / `reset_heat` each repeat. P1a metrics exported in JSON/CSV. Figures: `lab/figures-v4/`. CSV: `lab/results/mainnet-sweep-v4.csv` (273 rows, all `ok`).

## SpecFence vs OCC @8 TPS (mean of 3) vs v3

| block | OCC v3 | SpecFence v3 | OCC v4 | SpecFence v4 | SF/OCC v3 | SF/OCC v4 |
|-------|--------|--------------|--------|--------------|-----------|-----------|
| 13217637 | 291362 | 119167 | 246031 | 68431 | 0.409 | 0.278 |
| 14029313 | 260779 | 163022 | 223794 | 72699 | 0.625 | 0.325 |
| 14383540 | 164161 | 107076 | 140212 | 45845 | 0.652 | 0.327 |
| 14683600 | 98894 | 91238 | 87788 | 41890 | 0.923 | 0.477 |
| 15199017 | 251072 | 123603 | 223265 | 62159 | 0.492 | 0.278 |
| 19434587 | 42815 | 36103 | 30886 | 8323 | 0.843 | 0.269 |
| 19807137 | 93155 | 57586 | 58392 | 7354 | 0.618 | 0.126 |

- Arithmetic mean SF/OCC @8: **0.297** (v3 was ~0.65). Geometric mean: **0.279**.
- **No block** has SpecFence mean TPS ≥ OCC @8. Exit criterion **not met**.

Absolute OCC TPS also moved vs v3 (machine noise); within-sweep SF/OCC is the honest comparison.

## SpecFence @8 P1a metrics (v4 mean)

| block | abort_rate | tx_full_retry | bind_hits | wait_hard | spec_read | sel_inv | sel_fb_full | region_validate_fail | soft_edge_revokes | wait_adm |
|-------|------------|---------------|-----------|-----------|-----------|---------|-------------|----------------------|-------------------|----------|
| 13217637 | 0.006 | 6.3 | 1.0 | 986 | 2445 | 6 | 0 | 12 | 11 | 942 |
| 14029313 | 0.020 | 14.3 | 0.0 | 370 | 3107 | 14 | 0 | 15 | 1 | 277 |
| 14383540 | 0.046 | 33.0 | 7.3 | 773 | 4143 | 34 | 0 | 43 | 4 | 574 |
| 14683600 | 0.089 | 58.7 | 31.7 | 550 | 5036 | 61 | 0 | 73 | 21 | 241 |
| 15199017 | 0.009 | 8.0 | 0.3 | 720 | 3309 | 9 | 0 | 10 | 3 | 633 |
| 19434587 | 0.350 | 136.3 | 32.7 | 1144 | 8747 | 151 | 0 | 180 | 0 | 431 |
| 19807137 | 2.629 | 1872.0 | 3.3 | 3956 | 14264 | 1978 | 0 | 1877 | 0 | 1455 |

Notes:

- `tx_full_retry == occ_aborts` on every block (`fr/aborts = 1.00`) — every abort is still **FullRetry** (no PartialRetry / checkpoint re-entry yet).
- `selective_fallback_full = 0` everywhere — selective invalidate ran without falling back to full write-set ESTIMATE; `selective_invalidate_count ≈ tx_full_retry` on the hot blocks.
- Bind hits remain sparse (best on 14683600 / 19434587); they do not offset FullRetry cost.
- WaitHard vs SpecRead: SpecRead dominates counts, but WaitHard is large on 19807137 (3956) and correlates with the TPS collapse.

## Honest analysis — did P1a close the gap?

**No.** SpecFence v4 is **further behind OCC @8** than v3 (mean ratio 0.30 vs ~0.65). The plant is live (metrics non-zero: WaitHard, SpecRead, selective invalidate, region_validate_fail, soft revokes), but it does not improve throughput yet.

Attribution using metrics:

1. **FullRetry dominates.** Every abort is whole-tx re-exec (`tx_full_retry` tracks `occ_aborts` 1:1). Selective invalidate / readers / Bind do not yet replace FullRetry with PartialRetry — so REM machinery adds bookkeeping without repairing the expensive path.
2. **Over-Wait / WaitHard tax.** On cold-ish blocks abort_rate is already low (0.006–0.09) yet SF still trails OCC by ~2–3.5× — WaitHard + SpecRead path overhead serializes work OCC would keep speculative. Soft revokes are rare (0–21), so sticky Wait is not being cleared aggressively enough.
3. **Hot-block FullRetry storm (19807137).** abort_rate 2.63, `tx_full_retry` 1872, WaitHard 3956, SpecRead 14264 → SF/OCC **0.126**. Selective invalidate fires (~1978) but still ends in FullRetry; Bind almost never hits (3.3).
4. **Not selective_fallback_full.** Fallback-to-full ESTIMATE is **not** the culprit (`sel_fb_full = 0`). The gap is FullRetry + over-Wait, not fallback.

Exit criterion (SF mean ≥ OCC **or** explained by FullRetry): **explained by FullRetry** (with over-Wait amplifying the gap). Plant validation for P1b is success as instrumentation/behavior proof; performance exit is deferred to PartialRetry / ready-queue wave / Bind effectiveness (P2).

## Figures

- `lab/figures-v4/block_<id>_tps_abort.png` (×7)
- `lab/figures-v4/overview_tps_abort.png`
- `lab/figures-v4/specfence_wait_vs_speculate.png`
- `lab/figures-v4/specfence_wait_hard_vs_spec_read.png` (new)
- `lab/figures-v4/specfence_p1a_metrics_overview.png` (new: full_retry / bind / selective / fallback vs cores)

## Artifacts

- JSON: `lab/results/mainnet-sweep-v4.json`
- CSV: `lab/results/mainnet-sweep-v4.csv`
- Log: `lab/notes/mainnet-sweep-v4.log`
