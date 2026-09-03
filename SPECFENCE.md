# SpecFence

Mixed **Wait (PCC)** and **Speculate (OCC)** in the **same block**, at **region** granularity (account basic / storage slot). Not an OCC variant that only shrinks abort blast radius.

Committed state is always equivalent to sequential execution in preset block order. No commit reordering.

## Mechanism

- **Speculate (cold):** current Block-STM path — optimistic execute, validate, abort/retry.
- **Wait (hot):** later transactions that touch the region **wait for the last lower-idx writer** (hinted `from`/`to`, then MV last writer) before reading or writing it. This is a proactive admission/read-side action, not only ESTIMATE-after-the-fact.
- One transaction may Wait on region A and Speculate on region B in the same execution.
- **Intra-block wave:** first observed conflict on a region (validation abort, ESTIMATE, RW/WW overlap) promotes that region Speculate → Wait for the rest of the block. No global barrier.
- **Inter-block heat:** EWMA on cheap `from`/`to` account hints. Multi-writer / promoted accounts get hotter. Next block seeds those accounts in Wait. Unknown/cold stays Speculate. Map size is capped.
- **Beneficiary / lazy sender-recipient:** gas-payment writes to the beneficiary never promote or Wait. Lazy ETH updates are preserved.

## Modes

Selectable on `Pevm`. Default is current PEVM OCC.

| Mode | Behaviour |
|------|-----------|
| `ConcurrencyMode::Occ` | Block-STM only. Existing tests. |
| `ConcurrencyMode::Pcc` | Hinted accounts start in Wait (conservative prior-writer wait). |
| `ConcurrencyMode::SpecFence` | Mixed: heat-seeded Wait + OCC elsewhere + intra-block promotion. |

```rust
let mut pevm = Pevm::with_concurrency_mode(ConcurrencyMode::SpecFence);
pevm.execute_revm_parallel(/* ... */)?;
let metrics = pevm.last_specfence_metrics();
```

## Hooks

- `crates/pevm/src/specfence/` — region table, EWMA heat, wave promotion, metrics.
- `scheduler.rs` — `is_done` so Wait can block on a prior writer; `add_dependency` used for admission.
- `mv_memory.rs` — per-location / per-account `{Speculate, Wait}`; WW overlap detection.
- `vm.rs` — Wait region: depend on last lower-idx writer. Speculate region: OCC read.
- `pevm.rs` — mode, heat map (thread-safe, `reset_heat`), metrics snapshot.

## Tests

Mocked OCC (no `ethereum/tests` submodule):

```sh
cargo test -p pevm --release --test raw_transfers --test beneficiary --test small_blocks --test mixed -- --test-threads=1
```

SpecFence:

```sh
cargo test -p pevm --release --test specfence -- --test-threads=1
```

Full workspace tests need `git submodule update --init` (ethereum/tests). This crate does not download mainnet.

## Metrics

After a parallel block: `wait_admissions`, `speculate_executions`, `region_promotions`, `occ_aborts`, plus the addresses that waited vs speculated. `last_initial_wait_accounts()` is the test hook for inter-block heat seeding.
