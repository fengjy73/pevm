# PR #40 TPS losers — optimal vs overhead (index)

**Baseline:** PR #40 `cursor/specfence-midband-spine-tps-041c` @ `a7562676301eb173ef7a3692f89cd3b3e2cdbb75`  
**This branch:** `cursor/specfence-tps-losers-optimal-overhead-ff75`  
**Nature:** analysis only. No SpecFence CC / policy / learn change.  
**Soft=0 · Instant-off · professional terms**

## Read order

1. **[MAIN](specfence-pr40-tps-losers-optimal-vs-overhead.md)** — 先读。NEAR/FAR、为何没到最优排列、NEAR 后为何还有壳、PC/CC/学习、23/98。  
2. **[Appendix](specfence-pr40-tps-losers-optimal-vs-overhead-appendix.md)** — 逐块 Instant-off + DAG 表。  
3. **[Summary JSON](specfence-pr40-tps-losers-optimal-vs-overhead-summary.json)** — 机器可读 K=11。  
4. Raw（gitignore）: `lab/results/pr40-k11-optimal-overhead/{*-compare.json,dag-upper-bound.json}`。

## Verdict

| 簇 | 块 | 相对等权最优 |
|----|----|--------------|
| A 近独立 / 薄 | 14396881, 13217637, 19638737, 3356896 | **NEAR** — 残差是壳 |
| B 真脊欠盖 | 19807137, 16146267, 8889776, 19716145, 19860366, 19469101 | **FAR** — 软顶 8 + sticky Opt/空 Full |
| C 错对象 | 15274915 | **FAR** — Full/996 on lazy |

Corpus Soft=0 SF TPS≥OCC remains **23/98**. PR40 mid-band Δ withdrew over-admission; it did not reach L-wave schedules. Next cut must fork A/B/C.

## Context

- Land: [`specfence-tps-losers-midband-spine-land.md`](specfence-tps-losers-midband-spine-land.md)  
- Sweep JSON: [`specfence-tps-losers-midband-spine-summary.json`](specfence-tps-losers-midband-spine-summary.json)  
- Style: PR19 3356896 NEAR vs unit-cost bound（Instant-tax 不加总进墙）
