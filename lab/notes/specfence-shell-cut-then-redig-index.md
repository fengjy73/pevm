# 压剩余 ungated 壳 → 全量扫 + 三面深挖 — 笔记索引

**PR:** https://github.com/fengjy73/pevm/pull/43 (draft, do not merge)
**Branch:** `cursor/specfence-shell-cut-redig-6a8f` @ `2add58d` (+ notes)
**Base:** PR #42 `cursor/specfence-pc-cc-learn-complete-2cd0` @ `42cf30d`
**SoT:** 先压剩余 ~65 块壳，再全量扫 + 三面深挖看新瓶颈
**Soft=0 · one spine · `select_arm` is the only mouth · no P0/P1/P2**

Corpus TPS **win-rate** is the north star. Instant-off is the primary wall. PC / CC / learn are analysis lenses, not split modules.

## Read order

| # | note | what |
|---|------|------|
| 1 | [`specfence-shell-cut-then-redig-land-v1.md`](specfence-shell-cut-then-redig-land-v1.md) | S1–S5 land + invariants + acceptance |
| 2 | [`specfence-shell-cut-then-redig-sweep.md`](specfence-shell-cut-then-redig-sweep.md) | 99-block Soft=0 TPS vs OCC (md table) |
| 3 | [`specfence-shell-cut-then-redig-summary.json`](specfence-shell-cut-then-redig-summary.json) | same sweep, machine JSON |
| 4 | [`specfence-shell-cut-then-redig-instant-off.md`](specfence-shell-cut-then-redig-instant-off.md) | Instant-off N=5 on worst 6 |
| 5 | [`specfence-shell-cut-then-redig-pc-cc-learn.md`](specfence-shell-cut-then-redig-pc-cc-learn.md) | **主读** 三面「做了什么 / 没做好」+ 新瓶颈 |

Raw sweep / Instant-off JSON lives under `lab/results/` (gitignored).

## Outcome

| bar | this | hit |
|-----|-----:|:---:|
| SF TPS ≥ OCC clearly > 33/98 | **26 / 98** | no |
| SF/OCC median > 0.908 | **0.923** | yes |
| NEAR 14396881 better than 0.840 | **0.893** | yes |
| NEAR 13217637 better than 0.835 | **0.928** | yes |
| no lazy 4–27× tail | none (wall max 3.41 = 13287210 OCC-noise) | yes |
| Soft=0 / iter11 / erc20 | hold | yes |

Win-rate is the north star and **regressed**. Median rose via left-tail compression. Instant-off: no `sf_le_occ`. Soft=0 every iter.

## New bottleneck (after Instant-off)

The remaining ~72 losers are **not** one ungated-shell class.

1. **Thin `train_hat` mill** — 6196166 Instant 1.43× Win_8; 3356896 climb Win_2→Win_8 (Instant 1.13×).
2. **Under-covered storage spine** — 19807137 Instant-off **Opt×5** 1.30× (sweep Full/2 is ĉ/PROFILE, not the Instant-off object).
3. **Mid leftover-long after no-start probe** — 4330482 Instant Win_1 1.26×; sweep 8889776 / 19469101.
4. **Large leftover Detect** — 14334629 Instant Opt 1.17×. Right object; residual is abort, not wait-set.
5. **OCC-quiet jitter** — 13287210 Instant 3.77×. Do not chase.

Do **not**: another `skip_ungated_*`; lean thin `end_block`; drop large live-probe flush; restart Win_2 on every mid reuse.

## Prior baseline

- PR #42 land: [`specfence-pc-cc-learn-complete-land.md`](specfence-pc-cc-learn-complete-land.md) — 33/98, median 0.908
- PR #42 sweep: [`specfence-pc-cc-learn-complete-sweep.md`](specfence-pc-cc-learn-complete-sweep.md)
- PR #42 TPS JSON: [`specfence-pc-cc-learn-complete-summary.json`](specfence-pc-cc-learn-complete-summary.json)
