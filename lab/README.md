# SpecFence lab

Single working tree for the clean PEVM fork (`fengjy73/pevm`, branch `specfence`).
Do not mix this with `pevm-specfence-server`.

```
specfence/
  crates/pevm/            engine + SpecFence CC
  bins/fetch/             snapshot a mainnet block via RPC
  data/ethereum/          block.json + pre_state.json snapshots
  SPECFENCE.md            mechanism
  lab/
    README.md             this map
    literature/           VLDB/OSDI figure & analysis notes (+ PDFs)
    experiments/
      configs/            which blocks, cores, repeats
      scripts/            plotters
    results/              JSON/CSV from a sweep (generated)
    figures/              TPS scalability + abort-rate plots (generated)
    notes/                run logs
```

## Run a mainnet sweep

```sh
# from repo root
# 1. missing snapshots (needs ETHEREUM_RPC_URL)
cargo run -p pevm-fetch --release --config 'profile.release.lto=false' -- \
  "$ETHEREUM_RPC_URL" 14683600
cargo run -p pevm-fetch --release --config 'profile.release.lto=false' -- \
  "$ETHEREUM_RPC_URL" 19434587

# 2. multi-core OCC / PCC / SpecFence
cargo run -p pevm --release --config 'profile.release.lto=false' --example specfence_mainnet_sweep -- \
  --out lab/results/mainnet-sweep.json

# 3. VLDB-style figures
python3 lab/experiments/scripts/plot_vldb.py \
  --input lab/results/mainnet-sweep.json \
  --outdir lab/figures
```

Core grid is `1,2,4,8` (this machine has 8 cores). Each point is the mean of `--repeats` (default 3) with min/max whiskers.
