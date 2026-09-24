# Soft0-NonSpanPhaseSplit

**日期：** 2026-09-24（北京时间）
**起点：** `cursor/admitshard-core-d1f2` @ `aa448cb`（调度代码 `1fa3308`）。不从 HPC `16bce1f` 起。新分支、新 PR，基分支就是这个 tip。

## 目标

在 Soft=0 Instant-off 上常开（不进 `SPECFENCE_PROFILE`）四组相计时，导出到 `SpineReport` 和 compare 的 spine / `TPS_SUMMARY` 行。然后对 3356896 与 15274915 做 N=5 Soft=0，把中位墙那一轮和复用探针中位的相 ns 写进笔记和 PR。不改税，不改调度语义。

## 约束

- `estimate_block_sf=0`，`soft_wait_arms=0`，`occ_picks=0`，`spine_cores_max≤1`，`seq≡par`。
- 探针是求和或宿主墙，重叠必须写明，不能把工人累计和宿主 join 加进同一条等式。
- 不叠 Estimate 门、park-all、#47 suspend-as-Avoid、T0–T6。ExactWake 接线不动。
- 数字只来自这次运行。愿望线 1.5 未到就如实写。

## 完成标准

1. 新 PR 含探针。
2. 两焦点块 Soft=0 中位墙轮有新相 ns。
3. 报告含 PR、tip、表、相加是否接近 tax、协议是否绿。

## 步骤

1. **已完成** — 常开四组探针。薄块第一次跑发现 `inter_prior.crit_chain()` 为空（链长 17 < 32），span 过滤没装上，`post_exec_validate_ns` 变成未过滤总和。已改为空 crit 时用 `SpinePrior::chains`。
2. **已完成** — `8832029` 上 N=5 两块。薄 0.697（税 0.860），大 0.683（税 5.221）。`ge_1_5=false`。2 核冷检 `seq=par`。OCC 相对笔记漂移，同机重跑了 `aa448cb`。
3. **已完成** — 表在 `docs/specfence-soft0-nonspan-phasesplit.md` 和 PR #51。大块宿主四段加总等于 tax，残差 1.165 ms 是没探针的缝。薄块中位轮链头 31≠focus 4。草稿留到用户验收。
