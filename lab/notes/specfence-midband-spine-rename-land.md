# Mid-band over-admission × under-covered spine × CC/PC rename — full land

**Branch:** `cursor/specfence-midband-spine-rename-7361`  
**Base:** PR #36 `cursor/specfence-s-lazy-object-14f0` @ `c063844`  
**Design:** `lab/notes/specfence-midband-spine-rename-land-v1.md`  
**Why:** `lab/notes/specfence-pr36-k8-pc-cc-learn-analysis.md`  
**Soft=0 · one spine · `select_arm` is the only mouth**

Lazy-update chains stay **not** OrderedAdmit objects. Ungated OCC task selection, Done-on-success, and Soft=0 are preserved.

## Landed

| ID | Content |
|----|---------|
| **A1** | Wait-set soft-cap is an **over-admission OrderedAdmit** predicate, not `n≥512`. Thin short-chain (3356896) stays uncapped unless `cover_window` already inflated. Mid-band real spines (`19716145` begin~108, `19860366` begin~76) take the same light prefix (`cores.max(8).min(16)`), earliest wait-for deps kept. |
| **A2** | Lean `end_block` (skip HotSet / inter-prior / sketch) extends from large+lazy-seen to **mid-band stored D1**. First mid-band block still persists; reuse with stable conflict structure is lean. |
| **A3** | **Under-covered conflict spine** (Storage ≥64 pairs, or any real spine ≥128): no Full/Seg hard-order; ĉ prefers OptimisticRead / OCC abort over ever-costlier cover; `cover_window` does not learn-uphill when leftover+unfenced stay high at/above the light hat. 19807137-class yields to OCC. |
| **A4** | Keep PR36: lazy-update chain never OrderedAdmit; ungated OCC task selection; Done-on-success; Soft=0. |

## Terminology (code / metrics / compare / this note)

| Old | New |
|-----|-----|
| holes / begin 洞 | dependency-gated admission / OrderedAdmit wait-set |
| plant / 种 | admit / seed wait-for dependency |
| pick_occ | ungated OCC task selection (`ungated_occ_n`, `ungated_occ_while_gated`) |
| S-lazy | lazy-update chain |
| Spine-U | under-covered conflict spine |
| S-mixed | mixed spine |
| double_pay | Detect+Resolve double charge (`detect_resolve_double_charge_n`) |
| fat / 肥块 | large block (`LARGE_BLOCK_N`) |
| w_need | cover_window / ordered_window_width (`chosen_cover_window`) |
| 过预付 | over-admission OrderedAdmit |

`DeferPlant` remains the internal enum variant (label `Defer` = defer wait-for admit).

## Preserve

Done-on-success / iter11; no mid-execute ReadyEdge; Soft=0; Instant idle ↛ ĉ; lazy-update chain never OrderedAdmit; 3356896 Basic `0x32be` still light-cover Win_2; mixed-spine light cover stays (do not re-admit a 95-wide wait-set).

## Implementation notes

- `wait_set_soft_cap()` returns `None` on thin short-chain; mid-band and large return `cores.max(8).min(16)`.
- `should_lean_end_block(d1_stored)` is `n > THIN_N_MAX && d1_stored` or large+lazy-seen.
- `is_under_covered_spine` is Storage ≥64 or Basic/Unknown ≥128. 40-pair storage still light-covers; 80-pair storage yields to OptimisticRead.
- Futile cover (ordered leftover ≥2, unfenced ≥8, `cover_window` already at the light hat) freezes `cover_window` and clears crisis so Seg/Full are not invited back.

## Metrics (Soft=0, N=3 reuse @8 unless noted)

PR36 Instant-off / sweep walls from `specfence-pr36-k8-pc-cc-learn-analysis.md`.

| block | n | PR36 × | this wall × | wait-set | end_block | arm | note |
|------:|--:|-------:|------------:|---------:|----------:|-----|------|
| **19716145** | 341 | 2.22 sweep / 2.00 Instant-off (wait-set 102–108) | **1.62** (16.0 / 9.9) | **8** | 2.29 ms | Win_1→Opt | A1: wait-set 108→8 |
| **19860366** | 430 | 2.26 / 2.30 (wait-set 76–78, end 3.85–4.29, Win_7) | 2.27 (21.6 / 9.5) | **8** | 3.92 ms | Win_1→Opt | A1: wait-set 76→8; not Win_7 |
| **19807137** | 712 | 3.10 / 4.11 Instant-off (Full…Seg_3) | 3.66 (52.3 / 14.3) | 8 | 0.64 ms | Opt→Win_1 | no Seg learn-uphill; cover_window=0 |
| **3356896** | 176 | 1.34 N=7 | **1.20** compare N=7: 1.170 / 0.951 | 3 | 0.08 ms | Win / Opt | no severe regress |
| **14396881** | 1346 | 3.61 | 3.33 (14.0 / 4.2) | 0 | 0.31 ms | Opt→Full | not a 4×+ lazy-update tail |

OCC reuse on `19807137` spiked (~2.3 s); wall uses OCC median 14.3 ms, not the spike.

### Safety

- Soft=0 on every compare / sweep row
- lib specfence policy **67** passed; admit+ready_edge **52** passed
- specfence integration **44** passed / 20 ignored including **iter11**
- erc20_independent **ok**

## Commands

```
cargo test -p pevm --release --lib -- specfence::policy -- --test-threads=1
cargo test -p pevm --release --lib -- specfence::admit -- --test-threads=1
cargo test -p pevm --release --test specfence -- --test-threads=1
cargo test -p pevm --release --test erc20 -- independent -- --test-threads=1

SPECFENCE_COMPARE_ITERS=7 cargo run -p pevm --release \
  --config 'profile.release.lto=false' --example specfence_3356896_compare

SPECFENCE_ALL_REUSE=1 SPECFENCE_ALL_ITERS=3 SPECFENCE_ALL_PROCESS_TOP=0 \
  SPECFENCE_ALL_BLOCKS=19716145,19860366,19807137,3356896,14396881 \
  cargo run -p pevm --release --config 'profile.release.lto=false' \
  --example specfence_all_blocks_sweep
```
