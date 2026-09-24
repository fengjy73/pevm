# Lean `end_block` × under-covered sticky Opt × large-lazy path tax — full land

**PR:** https://github.com/fengjy73/pevm/pull/39 (draft)

**Branch:** `cursor/specfence-endblock-spine-tps-c471`  
**Base:** PR #38 `cursor/specfence-midband-spine-rename-7361` @ `37eb27b`  
**Design:** `lab/notes/specfence-endblock-spine-tps-land-v1.md`  
**Sweep:** `lab/notes/specfence-endblock-spine-tps-sweep.md`  
**TPS JSON:** `lab/notes/specfence-endblock-spine-tps-summary.json`  
**Soft=0 · one spine · `select_arm` is the only mouth**

Lazy-update chains stay **not** OrderedAdmit objects. Ungated OCC task selection, Done-on-success, wait-set predicate soft-cap, and Soft=0 are preserved.

## Landed

| ID | Content |
|----|---------|
| **E1** | Mid-band lean `end_block`: reuse with stored D1 **or** already-seen conflict structure skips HotSet / inter-prior / sketch / MV merge / persist clone / pair-merge / morph flush. First mid-band still persists. `19860366`-class `end_block` 3.92 ms → **0.22 ms**. |
| **E2** | Under-covered conflict spine **or** ordered prepaid ≥ OCC abort: sticky OptimisticRead. No empty Win_1 churn; no costly Seg/Full invite. `last_prepaid_ns` / `last_abort_cf_ns` survive `begin_block`. Thin short-chain (3356896) keeps light-cover Win_2. Mid-block loc_strategy / hops honor a cached ordered arm under a live wait-set. Cohort seed skips via `should_skip_ordered_admit_seed`. |
| **E3** | Large lazy-update / near-independent: `skip_ungated_path_tax` on execute+validate. Same-block reuse keeps `lazy_structure_seen`. Ungated OCC task selection is unchanged. Leftover slide must not freeze under a live wait-set (19469101 livelock). |
| **E4** | Keep PR36/38: lazy-update never OrderedAdmit; wait-set predicate soft-cap; Done-on-success; Soft=0. |

## Implementation notes

- `should_lean_end_block` is true on mid-band when stored D1 **or** `conflict_structure_seen` / `d1_structure_seen`; large+lazy-seen still leans.
- Lean `end_block` additionally skips MV-memory merge, persist clone, pair-merge, and thin-style unfenced walk when structure is already stored.
- `yield_to_occ_abort` is under-covered **or** (mid/large and prepaid ≥ abort). Thin 3356896 is excluded.
- `should_reuse_stored_d1` / `d1_structure_seen` keep lean reuse without requiring a fresh HotSet.
- `skip_ungated_path_tax` is large + (`lazy_seen` this block **or** `lazy_structure_seen` from prior incarnation).
- After the 19469101 hang: leftover slide stays enabled on yield; loc_strategy/hops yield **after** cache so a live wait-set keeps its ordered arm; hops=0 only if yield **and** no cached ordered arm.

## Metrics (Soft=0, N=3 reuse @8, `profile.release.lto=false`)

Corpus: `SPECFENCE_ALL_BLOCKS=all`, 99/99 loaded. TPS = `n_tx / wall_seconds` (median of 3). SF/OCC TPS >1 means SpecFence faster. Aggregates use n>0 (98); empty `19910734` listed, excluded from TPS rollup.

### Aggregates (n>0, 98 blocks)

| metric | value |
|--------|------:|
| loaded / candidates | 99 / 99 |
| Soft=0 every row | yes |
| SF TPS ≥ OCC | **28 / 98** |
| SF/OCC TPS median / p90 / min / mean | 0.857 / 1.133 / 0.246 / 0.876 |
| wall ratio median / p90 / max | **1.168** / 2.272 / 4.062 |
| SF TPS median | 38419 |
| OCC TPS median | 45975 |

### Residuals vs PR38 (same Soft=0 reuse @8)

| block | n | PR38 wall × | this wall × | this TPS SF/OCC | wait-set | end_block | arm | note |
|------:|--:|------------:|------------:|----------------:|---------:|----------:|-----|------|
| **19860366** | 430 | 2.27 (end 3.92 ms) | **2.00** | 0.50 | 8 | **0.22 ms** | Opt→Win_1 | E1: end_block 3.92→0.22 ms |
| **19807137** | 712 | 3.66 (Opt→Win_1) | **2.65** | 0.38 | 8 | 0.27 ms | Full census / cover_window=0 | E2: focused Opt→Opt ×2.04; corpus ×2.65 vs 3.66 |
| **19716145** | 341 | 1.62 (end 2.29 ms) | 2.27 | 0.44 | 8 | **0.23 ms** | Win_1→Opt | end_block cut; wall noisier vs PR38 |
| **3356896** | 176 | 1.20 (N=7) | 1.40 (N=3) | 0.71 | 3 | 0.05 ms | Win_1→Opt | no severe regress |
| **14396881** | 1346 | 3.33 | 4.06 | 0.25 | 5 | 0.31 ms | Opt→Opt | one large near-independent on the 4× line; not a 4–27× tail |
| **19469101** | 469 | (hang-risk) | **1.52** | 0.66 | 8 | 2.35 ms | Opt→Opt | livelock fixed; completes |

Focused N=3 (pre-corpus): 19860366 end_block 0.23 ms, wall 15.1/8.7; 19807137 Opt→Opt, 37.3/18.3 (OCC reuse spike ignored).

`14396881` is the only ≥4× row (max 4.062). That is one large near-independent on the 4× line, not a 4–27× lazy-update fat-tail return.

### Safety

- Soft=0 on every compare / sweep row
- lib specfence policy **70** passed; admit **35** passed; ready_edge **17** passed
- specfence integration **44** passed / 20 ignored including **iter11**
- erc20_independent **ok**

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
