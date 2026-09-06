# Mainnet sweep v6 smoke (cost-aware π)

**Date:** 2026-09-04 (Asia/Shanghai)  
**See:** `lab/notes/specfence-cost-policy-v6.md` for decision rule and gate.

**Command:**
```
cargo run -p pevm --release --config 'profile.release.lto=false' --example specfence_mainnet_sweep -- \
  --blocks 19807137,19434587,15199017 --cores 1,8 --repeats 1 \
  --out lab/results/mainnet-sweep-v6-smoke.json
```

## SpecFence @8 vs v5 smoke

| block | v5 wait_hard | v6 wait_hard | v5 SF TPS | v6 SF TPS | v5 SF/OCC | v6 SF/OCC |
|-------|--------------|--------------|-----------|-----------|-----------|-----------|
| 19807137 | 12589 | **8369** (−33%) | 5583 | **6481** | 0.220 | 0.141 |
| 19434587 | 3667 | **2848** (−22%) | 11430 | **12356** | 0.314 | 0.327 |
| 15199017 | 904 | **181** (−80%) | 63007 | 61025 | 0.349 | 0.277 |

Sum wait_hard −33.6%; mean SF/OCC 0.294→0.248 (OCC TPS noisy this run).  
`mean_p_at_wait≈0.90` / `mean_p_at_spec≈0.10` on hot block.

## Gate

Need SF/OCC>0.45 or WH drop ≥50% with TPS up → **not met → no full v6 / figures-v6**.

## Artifacts

- JSON: `lab/results/mainnet-sweep-v6-smoke.json`
- Note: `lab/notes/specfence-cost-policy-v6.md`
