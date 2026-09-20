# PR #36 全量扫块 × 最慢深挖 — 文档索引

**全部正文在 `lab/notes/`。** 文档 PR：本分支 draft（只分析，未改 CC / policy / learn）。  
**基线:** PR #36 `cursor/specfence-s-lazy-object-14f0` @ `c0638440813484b1f55abe4351383a4e8c9f8110`  
**Soft=0 · OptimisticRead / OrderedAdmit · Instant-off 主证墙 · Instant-tax 不可加总**

| 文档 | 内容 |
|------|------|
| **[specfence-pr36-k8-pc-cc-learn-analysis.md](specfence-pr36-k8-pc-cc-learn-analysis.md)** | **三面分析（主读）:** PR36 做了什么 / 没做好；每块 PC/CC/学习；S-lazy 尾是否收干净；对照矩阵与下一刀 |
| [specfence-pr36-allblocks-sweep.md](specfence-pr36-allblocks-sweep.md) | 全量 Soft=0 扫块详版（分布、Top、98 块全表、对照 PR34） |
| [specfence-pr36-slowest-deepdive.md](specfence-pr36-slowest-deepdive.md) | K8 数据详版（结构 JSON、逐 iter OCC/SF、PROFILE Instant-tax） |
| [specfence-pr36-allblocks-sweep-summary.json](specfence-pr36-allblocks-sweep-summary.json) | 扫块机器摘要 |
| [specfence-pr36-slowest-deepdive-summary.json](specfence-pr36-slowest-deepdive-summary.json) | K8 机器摘要 |

**推荐阅读顺序:** 三面分析 §0（先读那句判断）→ 关心的块 §i → 需要数字时再翻逐 iter / 全表。

## 三面分析置顶结论（不代替正文）

S-lazy 4–25× 肥尾已经收干净（全集 max 27.45→3.61，`n≥512`≥4× 为 0）。剩余最慢不是同一条尾巴：

- **Spine-U** `19807137` 仍是墙冠军（Instant-off ≈4.1×）——PC 宽度已在，CC 盖不住 storage 571。
- **中档真脊 Detect / n<512 过预付** 是新的主剩余（`19716145` 洞 108、`19860366` end_block 4 ms）。
- **S-lazy 残留** 是 2.7–3.5× 的壳（`14396881` / `13217637`），不是 `pick_occ≈0` 的模式开关。

下一刀不要再拿「禁 lazy 有序」当唯一北极星。
