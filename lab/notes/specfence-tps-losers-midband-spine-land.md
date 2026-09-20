# Mid-band real spine × Opt path tax — full land

**PR:** (draft, this branch)  
**Branch:** `cursor/specfence-midband-spine-tps-041c`  
**Base:** PR #39 `cursor/specfence-endblock-spine-tps-c471` @ `efe87f2`  
**Design:** `lab/notes/specfence-tps-losers-midband-spine-v1.md`  
**Sweep:** `lab/notes/specfence-tps-losers-midband-spine-sweep.md`  
**TPS JSON:** `lab/notes/specfence-tps-losers-midband-spine-summary.json`  
**Soft=0 · one spine · `select_arm` is the only mouth**

Lazy-update chains stay **not** OrderedAdmit objects. Ungated OCC task selection, Done-on-success, wait-set predicate soft-cap, and Soft=0 are preserved.

## Landed

| ID | Content |
|----|---------|
| **M1** | Mid-band leftover-long: sticky OptimisticRead unless cover is proven cheaper. Wait-set at the soft-cap that still loses drops `cover_window` and withdraws order. Thin 3356896 keeps light-cover Win_2. |
| **M2** | `skip_ungated_path_tax` covers mid/large when the wait-set is empty or only short-chain. `ignore_leftover_reservations` stays large-lazy only (19469101). |
| **M3** | Empty Win_1 banned on leftover-long mid/large until a measured covering arm is cheaper than OCC abort. Under-covered spines stay Opt. |
| **M4** | Full 99-block Soft=0 TPS vs OCC (pending / see sweep). |
| **M5** | Keep PR36/38/39: lazy-update never OrderedAdmit; Done-on-success; Soft=0. |

## Implementation notes

- `yield_to_occ_abort` is under-covered **or** wait-set-capped lose **or** prepaid ≱ abort **or** leftover-long mid/large without `cover_proven_cheaper`. Thin excluded.
- `cover_proven_cheaper` requires `last_cover_ok`, a covering ordered arm, n≥2, and ĉ + hysteresis < abort.
- `generate_arms` drops `Win_1` when `ban_empty_win1`.
- `note_ordered_seed` / `note_wait_set` record leftover-long plants and the post-cap wait-set.
- `skip_ungated_path_tax` = large+lazy **or** (n>thin ∧ ¬leftover-long wait-set).
- After-publish D1 walk stays on for mid-band short-chain wait-sets (`ignore_leftover_reservations` is the fat-lazy skip). Skipping that walk under M2 livelocked 19716145 (wait-set live, leftover-long hops=0).
- Leftover-long mid/large yield does not T3-slide or flush idle hops.

## Metrics

See sweep after M4.

## Commands

```
cargo test -p pevm --release --lib -- specfence::policy -- --test-threads=1
cargo test -p pevm --release --lib -- specfence::admit -- --test-threads=1
cargo test -p pevm --release --test specfence -- --test-threads=1
cargo test -p pevm --release --test erc20 -- independent -- --test-threads=1

SPECFENCE_ALL_REUSE=1 SPECFENCE_ALL_ITERS=3 SPECFENCE_ALL_PROCESS_TOP=0 \
  SPECFENCE_ALL_BLOCKS=all \
  cargo run -p pevm --release --config 'profile.release.lto=false' \
  --example specfence_all_blocks_sweep
```
