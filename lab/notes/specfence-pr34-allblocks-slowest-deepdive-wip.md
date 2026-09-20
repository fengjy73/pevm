# PR #34 全量扫块 × 最慢块深挖 — WIP

**基线:** `cursor/specfence-prepaid-losers-b5de` @ `0144211ae115c87eb2e80828e17e0750d3e2cf6b`  
**性质:** 分析 / 文档 / 结果；**不改** CC / policy / learn  
**Soft=0 · OptimisticRead / OrderedAdmit · 不发明 ns**

## 计划

1. `SPECFENCE_ALL_BLOCKS=all` + `SPECFENCE_ALL_REUSE=1` N=3 @8，覆盖全部可加载 ethereum 快照（~99，不是 52 OCC-gap 默认集）。
2. 按绝对 SF wall 与 SF/OCC 比各排一榜。
3. 对最慢 K=5–8 用 `specfence_block_deepdive`（Instant-off PRIMARY；PROFILE Instant-tax 另跑并标清）。
4. 中文笔记 + summary JSON；下一刀只列假设。

本文件将在扫块完成后被 sweep / deepdive 报告替换或并入。
