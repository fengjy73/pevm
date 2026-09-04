# Mainnet sweep v2 (after OCC abort metrics + SpecFence fence)

Sweep: `cargo run -p pevm --release --config 'profile.release.lto=false' --example specfence_mainnet_sweep -- --out lab/results/mainnet-sweep-v2.json`
Same 7 blocks / cores 1,2,4,8 / 3 repeats / `reset_heat` each repeat. Figures: `lab/figures-v2/`.

## OCC abort instrumentation (before → after)

v1 reported **OCC `occ_aborts=0` on every row** (metric only recorded under `uses_regions()`).
v2 counts every successful `try_validation_abort`, including pure OCC.

| block | OCC abort_rate @8 (v1) | OCC abort_rate @8 (v2) | mean occ_aborts @8 |
|-------|------------------------|------------------------|--------------------|
| 13217637 | 0.000 | 0.007 | 7.3 |
| 14029313 | 0.000 | 0.018 | 13.0 |
| 14383540 | 0.000 | 0.048 | 34.7 |
| 14683600 | 0.000 | 0.110 | 72.3 |
| 15199017 | 0.000 | 0.012 | 10.7 |
| 19434587 | 0.000 | 0.357 | 139.3 |
| 19807137 | 0.000 | 0.891 | 634.7 |

Smoke `19434587` (`lab/results/occ-abort-smoke.json`, cores 1,8, repeats 1): OCC @8 abort_rate **0.367** (143 aborts); SpecFence @8 abort_rate 0.221 with `independent_txs_skipped_by_fence=9139`.

## SpecFence v1 fence vs whole-tx

- Landed: validation cascade rewind fenced to first higher reader of aborted writes; hint-only multi-writer→Wait gated for SpecFence; heat + observed invalid/WW promotion kept.
- Still whole-tx: aborted incarnation fully re-executes; ESTIMATE still covers the aborted write set.
- Fence metrics @8 (mean):

| block | SF occ_aborts | cascade_validations_scheduled | independent_txs_skipped_by_fence |
|-------|---------------|-------------------------------|----------------------------------|
| 13217637 | 7.3 | 93.0 | 3742.0 |
| 14029313 | 14.0 | 596.3 | 3053.7 |
| 14383540 | 32.7 | 6468.7 | 10397.7 |
| 14683600 | 65.3 | 9680.3 | 12117.7 |
| 15199017 | 8.0 | 1100.7 | 5303.0 |
| 19434587 | 124.3 | 14905.0 | 13896.3 |
| 19807137 | 334.3 | 100248.7 | 13883.7 |

## SpecFence vs OCC 8-core TPS (mean of 3)

| block | OCC v1 | SpecFence v1 | OCC v2 | SpecFence v2 | SF/OCC v2 | SF/OCC v1 |
|-------|--------|--------------|--------|--------------|-----------|-----------|
| 13217637 | 230504 | 86866 | 204865 | 103174 | 0.504 | 0.377 |
| 14029313 | 190679 | 196104 | 250112 | 206065 | 0.824 | 1.028 |
| 14383540 | 144051 | 122896 | 151632 | 122059 | 0.805 | 0.853 |
| 14683600 | 96670 | 80231 | 93396 | 86577 | 0.927 | 0.830 |
| 15199017 | 253029 | 154422 | 219006 | 171627 | 0.784 | 0.610 |
| 19434587 | 41882 | 34425 | 41540 | 38916 | 0.937 | 0.822 |
| 19807137 | 76615 | 71903 | 73451 | 73355 | 0.999 | 0.938 |

### Takeaways

- OCC abort rates are now **meaningful** and rise with cores (highest on 19807137 and 19434587).
- SpecFence/OCC @8 **improved vs v1 on most blocks** (closer to OCC), especially 13217637 (0.38→0.50), 14683600 (0.83→0.93), 19434587 (0.82→0.94), 19807137 (~0.94→1.00). Still generally ≤ OCC — fence is cascade confinement, not full region-local repair.
- Fence skips many independent suffix validations (`independent_txs_skipped_by_fence` large wherever aborts fire).

See also `lab/notes/specfence-redesign-v1.md`.
