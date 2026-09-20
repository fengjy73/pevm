# PR #34 全量扫块 + 最慢块深挖 — 文档索引

**文档 PR:** https://github.com/fengjy73/pevm/pull/35（draft，仅分析，未改 CC）  
**全部正文在 `lab/notes/`（本目录），并镜像到 [specfence-lab](https://github.com/fengjy73/specfence-lab)。**

| 文档 | 内容 |
|------|------|
| [specfence-pr34-allblocks-sweep.md](specfence-pr34-allblocks-sweep.md) | 全量 Soft=0 扫块**详版**：口径、分布、Top 榜、**全块表** |
| [specfence-pr34-slowest-deepdive.md](specfence-pr34-slowest-deepdive.md) | K8 最慢块**详版**：逐块结构/逐 iter/开销桶/PROFILE/定位/下一刀 |
| [specfence-pr34-allblocks-sweep-summary.json](specfence-pr34-allblocks-sweep-summary.json) | 扫块机器可读摘要 |
| [specfence-pr34-slowest-deepdive-summary.json](specfence-pr34-slowest-deepdive-summary.json) | K8 Instant-off + PROFILE 原始摘要 |
| [specfence-allblocks-slowest-deepdive-v1.md](specfence-allblocks-slowest-deepdive-v1.md) | 任务简报 |

**读法:** 先索引 → 扫块 §0–2 → 深挖 §0 总定位 → 关心的块 §2 逐块表。
