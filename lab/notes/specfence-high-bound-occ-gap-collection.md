# SpecFence — high-bound / OCC-gap collection (全部跑一遍)

**Date:** 2026-09-15 (Asia/Shanghai)  
**Tip:** `2cbd339`  
**SoftWait Soft:** **0**  
**Vocabulary:** [`specfence-cc-glossary.md`](specfence-cc-glossary.md)  
**Machine catalog:** [`specfence-high-bound-occ-gap-collection.json`](specfence-high-bound-occ-gap-collection.json)  
**Block ids:** [`specfence-high-bound-occ-gap-block-ids.txt`](specfence-high-bound-occ-gap-block-ids.txt)  

---

## Goal

From all nonempty `data/ethereum/blocks` (98 / 99; empty `19910734`), keep blocks where the **theoretical parallel upper bound @8 is high** but **pure OCC@8 Soft=0 does badly** versus that bound — and where SpecFence can act (conflict morphology: `RAW_fan_out` / `mixed_RAW_WAW`). Meta-only gaps and WAW spines are appendix-excluded. Scoring harness only; no plant protocol change.

---

## Method

Same equal-cost DAG bound as [`specfence-10block-parallel-upper-bound.md`](specfence-10block-parallel-upper-bound.md):

1. Build final-RW **RAW + WAW** edges (`analyze_dag`, beneficiary / `basic_lazy` excluded).
2. `L = longest_chain`, `W = max_wave_width`.
3. `bound@8 = min(8, n/L, W)`; `speedup_∞ = n/L`.
4. `t_work` = sequential wall, or **OCC@1** if `serial > 5× OCC@1`.
5. `ideal_ms = t_work / bound@8`; gap = `OCC@8 / ideal` (and `abs_waste_ms = OCC − ideal`).

Measurement: scoring example `crates/pevm/examples/specfence_all_blocks_upper_bound.rs` (bumps `header.gas_used` to ≥4M so low-gas blocks take the parallel OCC path). Six self-destruct `FallbackToSequential` blocks got DAG via a temporary capture-before-fallback probe (reverted from plant); OCC walls overlaid from `all-blocks-sf-occ-sweep` Soft=0 when lower. Results: `lab/results/all-blocks-parallel-upper-bound-finegrain.json` + `_ub-missing6.json`.

---

## Thresholds (tuned with evidence)

| Gate | Value | Evidence |
|------|------:|----------|
| `bound_at_8` | **≥ 5.0** | Corpus p25≈5.96; excludes pure WAW spines (~1–2.5×). |
| `occ_over_ideal` | **≥ 4.0** | Among bound≥5, median gap≈5.75; keeps named high-bound 10-block members (19469097≈4.8×, 19606599≈4.3×). |
| `n_tx` | **≥ 30** | Microblocks inflate OCC/ideal via fixed OCC overhead. |
| `abs_waste_ms` | **≥ 0.5** | Drop huge-ratio quiet crumbs with <0.5 ms absolute waste (e.g. 116525, 1796867). |
| morphology | **exclude `WAW_spine`, `trivial`, `near_independent_meta_gap`** | High-bound shortfall from conflict (RAW/mixed). Meta-gap + spines are appendix-only. |

Grid check (bound≥5 ∧ gap≥4 ∧ n≥30 ∧ waste≥0.5 ∧ ¬spine ∧ ¬meta_gap): **52 / 98**. Pre-exclusion of meta_gap was 61; the 9 near-independent blocks are appendix-only (no SpecFence lever). Looser gap≥3 adds ~10 mixed with milder shortfall; tighter gap≥5 drops named mixed anchors 19469097 / 19606599.

---

## Headline

- **Selected:** **52 / 98** nonempty (`RAW_fan_out` + `mixed_RAW_WAW` only)
- **Class histogram:** `RAW_fan_out`=3, `mixed_RAW_WAW`=49
- **Appendix `excluded_meta_gap` (`near_independent_meta_gap`):** 9 — no SpecFence analysis value; meta/scheduler overhead unavoidable
- **Appendix WAW_spine (excluded from main):** 8 — `[2641321, 4370000, 6137495, 6196166, 7280000, 12522062, 19469096, 19807137]`

---

## Classification rules

| Class | Rule (DAG) | Why OCC leaves bound on the table |
|-------|------------|-----------------------------------|
| `RAW_fan_out` | RAW-heavy, wide `W`, `L` not spine-long | Consumers take **optimistic_read** of unfinished / wrong version → validate → **full_abort_reexecute** fan-out storms. |
| `mixed_RAW_WAW` | Both RAW and WAW material | Mix of optimistic_read waste and WAW validate storms; still high `bound@8` so width exists. |
| `near_independent_meta_gap` *(appendix `excluded_meta_gap` only)* | Few RAW/WAW edges, high indep_frac, short `L` | Bound ≈8× but gap is **meta / scheduler / cold-start** — SpecFence cannot optimize; **excluded from main**. |
| `WAW_spine` *(appendix only)* | WAW-dominated long `L`, `bound@8`≲3.5 | Structural ceiling already low; excluded from main all-run set. |

---

## Per-class block lists

### `RAW_fan_out` (3)

```
4864590, 14689597, 15537394
```

| block | n | L | W | RAW | WAW | bound@8 | OCC@8 ms | ideal ms | OCC/ideal | occ_speedup |
|------:|--:|--:|--:|----:|----:|--------:|---------:|---------:|----------:|------------:|
| 14689597 | 564 | 29 | 434 | 449 | 145 | 8.00× | 4.84 | 0.46 | 10.51× | 0.76× |
| 4864590 | 195 | 6 | 170 | 21 | 14 | 8.00× | 1.38 | 0.26 | 5.24× | 1.53× |
| 15537394 | 80 | 13 | 40 | 45 | 35 | 6.15× | 2.24 | 0.47 | 4.79× | 1.28× |

### `mixed_RAW_WAW` (49)

```
3356896, 5283152, 8038679, 8889776, 9069000, 10760440, 11114732, 11743952, 12159808, 12243999, 12244000, 12459406, 13217637, 14029313, 14334629, 14383540, 14683600, 14689598, 15199017, 15274915, 15538827, 15752489, 16146267, 16257471, 17034869, 17034870, 17666333, 18426253, 18988207, 19426587, 19469097, 19469098, 19469099, 19469101, 19505152, 19606598, 19606599, 19638737, 19716145, 19737292, 19860366, 19917570, 19929064, 19932148, 19932703, 19932810, 19933122, 19933597, 19934116
```

| block | n | L | W | RAW | WAW | bound@8 | OCC@8 ms | ideal ms | OCC/ideal | occ_speedup |
|------:|--:|--:|--:|----:|----:|--------:|---------:|---------:|----------:|------------:|
| 3356896 | 176 | 17 | 151 | 0 | 27 | 8.00× | 0.83 | 0.04 | 18.95× | 0.42× |
| 11743952 | 206 | 11 | 182 | 7 | 49 | 8.00× | 10.95 | 1.02 | 10.76× | 0.74× |
| 12159808 | 180 | 5 | 159 | 7 | 27 | 8.00× | 5.28 | 0.52 | 10.17× | 0.79× |
| 11114732 | 100 | 6 | 87 | 11 | 19 | 8.00× | 4.79 | 0.49 | 9.69× | 0.83× |
| 15274915 | 1226 | 77 | 1121 | 35 | 120 | 8.00× | 4.66 | 0.55 | 8.55× | 0.94× |
| 8038679 | 237 | 5 | 218 | 1 | 32 | 8.00× | 1.82 | 0.22 | 8.36× | 0.96× |
| 14689598 | 111 | 8 | 76 | 7 | 32 | 8.00× | 2.19 | 0.30 | 7.38× | 1.08× |
| 19933122 | 45 | 4 | 38 | 1 | 7 | 8.00× | 0.87 | 0.12 | 7.34× | 1.09× |
| 19933597 | 154 | 13 | 131 | 7 | 39 | 8.00× | 3.58 | 0.50 | 7.20× | 1.11× |
| 19606598 | 91 | 6 | 80 | 3 | 13 | 8.00× | 1.60 | 0.23 | 6.86× | 1.17× |
| 15752489 | 132 | 9 | 107 | 5 | 30 | 8.00× | 2.51 | 0.37 | 6.85× | 1.17× |
| 19929064 | 103 | 11 | 82 | 12 | 36 | 8.00× | 2.98 | 0.47 | 6.37× | 1.26× |
| … | (37 more in JSON) | | | | | | | | | |

### Appendix — `excluded_meta_gap` (`near_independent_meta_gap`) (9)

User: no SpecFence analysis value; meta overhead unavoidable; SpecFence cannot optimize. Removed from main selected set / block-ids.

```
2179522, 4330482, 5891667, 11814555, 12047794, 12300570, 12520364, 13287210, 14396881
```

| block | n | L | W | RAW | WAW | bound@8 | OCC@8 ms | ideal ms | OCC/ideal | occ_speedup |
|------:|--:|--:|--:|----:|----:|--------:|---------:|---------:|----------:|------------:|
| 2179522 | 222 | 2 | 221 | 0 | 1 | 8.00× | 1.52 | 0.04 | 40.35× | 0.20× |
| 5891667 | 380 | 1 | 380 | 0 | 0 | 8.00× | 1.34 | 0.06 | 21.84× | 0.37× |
| 13287210 | 1414 | 3 | 1412 | 1 | 9 | 8.00× | 4.50 | 0.26 | 17.62× | 0.45× |
| 11814555 | 579 | 7 | 568 | 0 | 11 | 8.00× | 2.37 | 0.15 | 15.89× | 0.50× |
| 12300570 | 687 | 3 | 684 | 0 | 3 | 8.00× | 2.11 | 0.16 | 13.43× | 0.60× |
| 14396881 | 1346 | 5 | 1337 | 0 | 13 | 8.00× | 4.38 | 0.36 | 12.11× | 0.66× |
| 4330482 | 237 | 10 | 225 | 0 | 12 | 8.00× | 1.17 | 0.10 | 11.26× | 0.71× |
| 12520364 | 660 | 3 | 657 | 1 | 10 | 8.00× | 2.80 | 0.26 | 10.93× | 0.73× |
| 12047794 | 232 | 3 | 227 | 9 | 0 | 8.00× | 4.68 | 0.53 | 8.82× | 0.91× |

### Appendix — `WAW_spine` low-bound control (excluded)

Prefer exclude from main all-run set (`bound@8` already ≈1–2.5×). Kept labeled for contrast:

```
2641321, 4370000, 6137495, 6196166, 7280000, 12522062, 19469096, 19807137
```

| block | n | L | W | RAW | WAW | bound@8 | OCC/ideal |
|------:|--:|--:|--:|----:|----:|--------:|----------:|
| 2641321 | 83 | 75 | 6 | 0 | 77 | 1.11× | 6.69× |
| 19807137 | 712 | 571 | 106 | 9 | 628 | 1.25× | 0.95× |
| 6137495 | 60 | 33 | 28 | 0 | 32 | 1.82× | 1.93× |
| 19469096 | 250 | 132 | 92 | 6 | 172 | 1.89× | 1.37× |
| 6196166 | 108 | 49 | 25 | 0 | 249 | 2.20× | 5.56× |
| 7280000 | 118 | 39 | 66 | 5 | 58 | 3.03× | 2.10× |
| 4370000 | 97 | 32 | 56 | 2 | 123 | 3.03× | 5.05× |
| 12522062 | 177 | 56 | 95 | 5 | 101 | 3.16× | 2.33× |

---

## How to run

### Recompute DAG + OCC@8 (scoring harness)

```bash
cargo run -p pevm --release --config 'profile.release.lto=false' \
  --example specfence_all_blocks_upper_bound -- \
  --out lab/results/all-blocks-parallel-upper-bound-finegrain.json
# subset:
cargo run -p pevm --release --config 'profile.release.lto=false' \
  --example specfence_all_blocks_upper_bound -- \
  --blocks $(paste -sd, lab/notes/specfence-high-bound-occ-gap-block-ids.txt)
```

### SpecFence vs OCC all-run on this collection

`specfence_all_blocks_sweep` defaults to `lab/notes/specfence-high-bound-occ-gap-block-ids.txt` (52 ids; Soft=0). Override with `SPECFENCE_ALL_BLOCKS=all` for the full ~99 corpus, or a comma list for an ad-hoc subset. Prefer N≥3 on outliers. Compare walls to `ideal_ms` / `bound_at_8` in the JSON — the goal is closing **OCC/ideal**, not beating a low spine bound / meta gap.

### Named anchors (sanity)

| block | class | role |
|------:|-------|------|
| 14689597 | RAW_fan_out | classic storage RAW fan-out |
| 19606599 / 19469097 | mixed_RAW_WAW | high-bound mixed |
| 8889776 | mixed_RAW_WAW | Soft=0 worst/mixed (still bound≈5.9×) |
| 2179522 / 14396881 / 12047794 | excluded_meta_gap (appendix) | bound=8× meta/overhead — not in main run |
| 19807137 | WAW_spine (appendix) | low-bound control |

---

## Corpus snapshot (all 98 nonempty)

- bound@8: p25=5.96, median=8.00, p75=8.00
- OCC/ideal: p25=4.21, median=5.23, p90=11.51
- All morph (pre-filter): {'trivial': 2, 'near_independent_meta_gap': 18, 'mixed_RAW_WAW': 67, 'WAW_spine': 8, 'RAW_fan_out': 3}

---

## Honesty

- Soft=0 throughout (OCC path has no SoftWait Soft).
- Equal-cost hop model can under- or over-estimate when tx costs are skewed (see 19807137 OCC/ideal<1 in prior 10-block note).
- Six blocks used self-destruct fallback capture for DAG; OCC walls may use sweep overlay — see `fallback_self_destruct_capture` / `occ_src` in JSON.
- Scoring harness gas bump is measurement-only; plant protocol unchanged at tip `2cbd339`.

