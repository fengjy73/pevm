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

1. **进行中** — 常开 `admit_seed_ns` / `join_wait_ns` / `idle_ns`+`heal_ns` / `post_exec_validate_ns`（span 外）+ `post_exec_in_span_ns`。`join_mark_origin_ns` 用来和 focus 的 head/tail 相交，避免把整段 join 当成税。
2. **待做** — release、LTO off、N=5、请求 8 核。两块。2 核冷检 `seq≡par`。
3. **待做** — 笔记和 PR 填实测表。草稿留到用户验收。
