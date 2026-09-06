# Mainnet sweep v3 (SpecFence Bayesian location-level feedback)

Sweep: `cargo run -p pevm --release --config 'profile.release.lto=false' --example specfence_mainnet_sweep -- --out lab/results/mainnet-sweep-v3.json`
Same 7 blocks / cores 1,2,4,8 / 3 repeats / `reset_heat` each repeat. Figures: `lab/figures-v3/`.

## SpecFence vs OCC @8 TPS (mean of 3)

| block | OCC v2 | SpecFence v2 | OCC v3 | SpecFence v3 | SF/OCC v2 | SF/OCC v3 |
|-------|--------|--------------|--------|--------------|-----------|-----------|
| 13217637 | 204865 | 103174 | 291362 | 119167 | 0.504 | 0.409 |
| 14029313 | 250112 | 206065 | 260779 | 163022 | 0.824 | 0.625 |
| 14383540 | 151632 | 122059 | 164161 | 107076 | 0.805 | 0.652 |
| 14683600 | 93396 | 86577 | 98894 | 91238 | 0.927 | 0.923 |
| 15199017 | 219006 | 171627 | 251072 | 123603 | 0.784 | 0.492 |
| 19434587 | 41540 | 38916 | 42815 | 36103 | 0.937 | 0.843 |
| 19807137 | 73451 | 73355 | 93155 | 57586 | 0.999 | 0.618 |

## SpecFence @8 abort / wait / bayes (v3 mean)

| block | abort_rate | wait_admissions | bayes_wait | bayes_speculate | bayes_conflict | bayes_success | wave_promotions | mean_wait_posterior |
|-------|------------|-----------------|------------|-----------------|----------------|---------------|-----------------|---------------------|
| 13217637 | 0.006 | 940 | 3837 | 5873 | 46 | 12278 | 41 | 0.338 |
| 14029313 | 0.017 | 275 | 1211 | 5766 | 69 | 10545 | 56 | 0.308 |
| 14383540 | 0.032 | 535 | 2368 | 5984 | 87 | 14050 | 58 | 0.324 |
| 14683600 | 0.091 | 267 | 942 | 7717 | 209 | 16423 | 125 | 0.327 |
| 15199017 | 0.009 | 601 | 2702 | 6011 | 50 | 13416 | 41 | 0.330 |
| 19434587 | 0.320 | 506 | 696 | 11716 | 234 | 25863 | 82 | 0.566 |
| 19807137 | 0.690 | 3182 | 60 | 27608 | 528 | 13162 | 36 | 0.323 |

## Honest analysis

SpecFence v3 is **still generally behind OCC @8**. Bayesian location-level Wait/Speculate + validation feedback is the right control unit, but:

1. **Whole-tx re-exec** still dominates cost when a speculated region aborts — Wait reduces some aborts but cannot repair only the bad slots inside revm.
2. **Full write-set ESTIMATE** retained: selective ESTIMATE broke serial equivalence when concurrent readers had not recorded a read set yet.
3. **Decision threshold / WW signal**: once-per-block WW conflict observations + validation `observe_conflict_location_always` drive posteriors; on hot blocks `wait_admissions` rise and can over-serialize relative to pure OCC when independence is high.
4. Cascade fence metrics still matter for independent suffix re-validation; they are not a substitute for local repair.

Remaining gaps toward full wave/local repair: partial reexec in revm, ready-queue wave scheduler, safe selective ESTIMATE / version-switch without leftover aborted Data.

See `lab/notes/specfence-redesign-v2.md`.
