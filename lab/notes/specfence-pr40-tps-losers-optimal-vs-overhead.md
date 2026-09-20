# PR #40 TPS 输家：理论最优排列 vs 不必要开销

**基线:** PR #40 `cursor/specfence-midband-spine-tps-041c` @ `a7562676301eb173ef7a3692f89cd3b3e2cdbb75`  
**性质:** 分析；不改 SpecFence CC / policy / learn  
**Soft=0 · Instant-off 主墙 · Instant-tax 不计入墙**  
**索引:** [`specfence-pr40-tps-losers-analysis-index.md`](specfence-pr40-tps-losers-analysis-index.md)  
**数据:** [`specfence-pr40-tps-losers-optimal-vs-overhead-summary.json`](specfence-pr40-tps-losers-optimal-vs-overhead-summary.json)

用户问：这些块还有什么问题；若还没接近理论最优排列 — 为什么；若已接近 — 为什么还有大量不必要开销。

测量进行中。结论与逐块 NEAR/FAR 在 Instant-off + DAG 跑完后写入。不得发明 ns。

## 0. 方法（先固定）

- **Instant-off Soft=0:** `specfence_3356896_compare` interleaved OCC/SF, N=5, reuse SF, `SPECFENCE_COMPARE_CORES=8`。主墙 = SF reuse median vs OCC median。
- **DAG 下界:** `analyze_dag` 有效边（beneficiary + `basic_lazy` 排除）。`L = longest_chain`，`W = max_wave_width`，`bound@8 = min(8, n/L, W)`。等权 makespan 波数 `max(L, ⌈n/8⌉)`。
- **串行帽:** upper-bound harness 的 sequential wall；若 serial 病态（≫ OCC@1）则标出，不用病态 serial 当 `t_work`。
- **NEAR / FAR:** 结构距 list-schedule 骨架（关键路径序、wait-set 是否盖住真脊、独立集是否过闸、臂是否该 Opt 却闸 / 该盖却没盖）。墙远高于单位成本界 **单独** 不构成 FAR。
- **未测桶:** 显式标 **未测**。不把 Instant-tax / PROFILE 加进墙。

## 1. K=11

最差 SF/OCC TPS + 中档真脊代表 + 大块近独立对照 + 薄块 3356896。见索引表。

---

*正文待 Instant-off / DAG 结果填入。*
