# Learn × CC fine-grain × PC utilization — full-package land

**PR:** https://github.com/fengjy73/pevm/pull/42 (draft)  
**Branch:** `cursor/specfence-pc-cc-learn-complete-2cd0` @ `a995f81`  
**Base:** PR #40 `cursor/specfence-midband-spine-tps-041c` @ `a756267`  
**Design:** `lab/notes/specfence-pc-cc-learn-complete-land-v1.md`  
**Evidence:** `lab/notes/specfence-pr40-tps-losers-optimal-vs-overhead.md`  
**Sweep:** `lab/notes/specfence-pc-cc-learn-complete-sweep.md`  
**TPS JSON:** `lab/notes/specfence-pc-cc-learn-complete-summary.json`  
**Soft=0 · one spine · `select_arm` is the only mouth**

Lazy-update chains stay **not** OrderedAdmit objects. Ungated OCC task selection, Done-on-success, and Soft=0 are preserved.

## Landed

| ID | Content |
|----|---------|
| **L1** | Break `cover_proven_cheaper` deadlock: cold/crisis may probe a short cover_window. Sticky only on wall success vs OCC abort counterfactual; else OptimisticRead. Never-tried is not a refuse. |
| **L2** | Per-ℓ reward is loc wall (reexec + ordered + refuse share) vs abort CF. Mid-band `cover_window` grows only when wall still beats abort — not unfenced-only. Instant idle stays out of ĉ. |
| **L3** | lazy-update / near-independent leftover-long below L=20: candidates are OptimisticRead + DeferPlant only. Morph still does not inherit Win onto lazy. |
| **L4** | Hot proven cover stays sticky. Probe budget is 2. Instant idle ↛ ĉ. |
| **C1** | L∈[20,64] real Basic/storage (and real Basic up to the under-covered floor, 15274915 Basic-77): segmented/sliding cover_window. Not whole-spine Full, not forever Opt. |
| **C2** | L≫64 (storage ≥64, Basic ≥128): sticky OptimisticRead. Ban empty Win_1 / Seg uphill. Ultra-long-only large blocks drop leftover wait-set holes. |
| **C3** | Thousand-writer (n_pairs≥128) on a large non-Storage loc is never OrderedAdmit. 15274915-class gates only the real Basic spine. |
| **C4** | Wait-set soft-cap is decoupled from cover depth. At-cap leftover deepens `cover_window` by one segment instead of total withdraw. |
| **P1** | `skip_ungated_tx_path_tax`: ungated execute+validate is OCC-equivalent on mid/large (and thin majority). Gated txs still take SpecFence. |
| **P2** | Gates remain edge constraints. `next_sf_task` still OCC-picks when the wait-set is empty. Ungated_occ ≈ n − wait_set after P3/C3. |
| **P3** | Near-independent large blocks (no mid-band coverable spine) drop non-critical wait-set slots (soft-cap 0). |

## Implementation notes

- `yield_to_occ_abort` is under-covered **or** failed wall probe **or** leftover-long without remaining probe budget. Mid-band `can_probe_cover` does **not** yield.
- `select_arm` forces a covering probe arm so the unused Opt prior cannot skip never-tried cover.
- Unproven `probe_cover_w` is always Win_2, including after C4 deepen. Reusing a grown window as the next probe plant livelocks ERC-20 mid-band reuse (`p4`). Proven cover reads `loc_cover_window`.
- `train_hat` on mid-band real spines allows a second segment (C4).
- `leftover_slide_ok` never T3-slides a mid-band coverable spine or a large planted OrderedAdmit prefix, including after crisis / sys-reexec (19469101 N=3 reuse). Thin leaking short-chain may still slide. C4 deepens at `end_block`.
- `skip_ungated_path_tax` stays block-level (empty / short-chain wait-set) for lean `end_block` and after-publish D1 skip.
- After-publish D1 walk stays on for a **live** wait-set.
- Soft=0. No SoftWait Soft arm.

## Metrics (Soft=0, N=3 reuse @8, n>0)

| metric | this | PR #40 |
|--------|-----:|-------:|
| SF TPS ≥ OCC | **33 / 98** | 23 / 98 |
| SF/OCC median | **0.908** | 0.832 |
| wall max (excl. noisy 2179522) | **1.71** | 3.220 |
| 4–27× lazy thousand-writer tail | **none** | none |
| Soft=0 | yes | yes |

FAR: 19716145 0.660→1.035; 19860366 0.677→0.857; 8889776 0.445→0.872; 16146267 0.362→0.856.  
NEAR: 14396881 0.311→0.840; 13217637 0.360→0.835.  
15274915: Opt/60 + short Full/1, wait-set 0 — no lazy Full/996.  
19469101 leftover-slide hang class completes (N=3 reuse).  
`2179522` wall 10.5 this run is quiet-block noise (SF reuse ~144 ms; OCC ~1.4 ms), not a lazy Full tail.

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
