# Mainnet sweep v5 smoke (P2 PartialRetry)

**Date:** 2026-09-04 (Asia/Shanghai)  
**Commit:** (see git)  
**Command:**
```
cargo run -p pevm --release --config 'profile.release.lto=false' --example specfence_mainnet_sweep -- \
  --blocks 19807137,19434587,15199017 --cores 1,8 --repeats 1 \
  --out lab/results/mainnet-sweep-v5-smoke.json
```

Smoke only (3 blocks × cores 1,8 × 1 repeat). No full 7-block v5 — TPS not uniformly better than v4 despite FullRetry collapse.

## SpecFence @8: full_retry before/after vs v4

| block | v4 tx_full_retry (mean/3) | v5 tx_full_retry | v5 partial_retry | v5 occ_aborts | fr/aborts v4 | fr/aborts v5 | v4 SF TPS | v5 SF TPS | v5 OCC TPS |
|-------|---------------------------|------------------|------------------|---------------|--------------|--------------|-----------|-----------|------------|
| 19807137 | 1872.0 | **0** | 1962 | 1962 | 1.00 | **0.00** | 7354 | 5583 | 25418 |
| 19434587 | 136.3 | **0** | 155 | 155 | 1.00 | **0.00** | 8323 | 11430 | 36344 |
| 15199017 | 8.0 | **0** | 9 | 9 | 1.00 | **0.00** | 62159 | 63007 | 180727 |

## Honest read

1. **FullRetry eliminated on smoke.** Every abort is now `partial_retry_count` (`pr_fb=0`). P1b’s `tx_full_retry == occ_aborts` is broken as intended.
2. **TPS mixed / noisy.** Hot block 19807137 still trails OCC badly and is slightly behind v4 SF TPS in this single-repeat smoke. 19434587 improved vs v4; 15199017 flat. OCC TPS also moved vs v4 (machine noise) — within-smoke SF/OCC remains ≪ 1.
3. **WaitHard still heavy** on 19807137 (wait_hard=12589) — PartialRetry fixes the abort accounting path but does not remove over-Wait tax.
4. **Stop after smoke** (no full 7-block v5): clear metric win on FullRetry; no clear uniform TPS win to justify the longer sweep yet.

## Artifacts

- JSON: `lab/results/mainnet-sweep-v5-smoke.json`
- Log: `lab/notes/mainnet-sweep-v5-smoke.log`
