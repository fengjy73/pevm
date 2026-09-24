# Soft=0 非 span 相探针

**日期：** 2026-09-24（北京时间）
**起点：** AdmitShard `cursor/admitshard-core-d1f2` @ `aa448cb`（调度 `1fa3308`）。本刀只加计时，不改调度。
**协议：** Soft=0 Instant-off（不设 `SPECFENCE_PROFILE`、不设 `SPECFENCE_HANG_TRACE`），release，`profile.release.lto=false`，N=5，请求 8 核。主指标仍是 reuse-median TPS 的 SF/OCC。`tax_ms = SF_wall − span`，span 取中位墙那一轮的 focus `tail_ms − head_ms`。

## 探针窗口

常开，不进 PROFILE。导出在 `SpineReport`，并打在 compare 的 `spine` 行和 `TPS_SUMMARY` 上。

| 名 | 窗口 | 时钟 |
| --- | --- | --- |
| `admit_seed_ns` | SpecFence 块初到 `thread::scope` 之前：prior sketch、arms、`admit_seed_begin_block`、crit install、`seed_begin` 把 AdmitIndep 切进每核 deque，以及随后的 `SpecFenceCtx` 装配。不含更早的 `MvMemory` / 调度器分配，不含 spawn 循环。 | 宿主墙，整段在任何 `first_start` 之前，在链 span 外 |
| `join_wait_ns` | 最后一次 `scope.spawn` 返回之后，到 `thread::scope` 返回（主线程等最慢工人）。 | 宿主墙。覆盖工人尾段，和 execute / `post_exec_*` / `idle_ns` 重叠 |
| `join_mark_origin_ns` | 并行相位原点 `exec_origin` 到 join 开始。与 focus 的 `head_ms` / `tail_ms` 同一原点。 | 用来算 join 与 span 的交集，本身不是税 |
| `idle_ns` | `pick → None` 整段：heal、yield、32 次 spin、`park_idle`。不含返回 `None` 的那次 `pick`。 | 各工人纳秒之和，含 `heal_ns` |
| `heal_ns` | 空档里的 `heal_finished_preds`、sleeper wake、`heal`、`force_idle_recover`，以及这些调用旁边的 `drain_wave`。不含 yield / spin / park。 | `idle_ns` 的子集，仍是求和 |
| `post_exec_validate_ns` | 执行成功后的 `drain_wave` + `validate_to_plan` + `resolve_plan::apply`，以及 `Task::Validation` 臂上的 validate+resolve。按窗口**起点**分类：起点落在先验 crit 链 span `[head.first_start, tail.first_start)` 之外才计入。 | 各工人纳秒之和 |
| `post_exec_in_span_ns` | 同一窗口，起点落在开着的 span 内。整段算进 span 内，即使尾部在窗口中途开工。 | 求和。`post_exec_validate_ns + post_exec_in_span_ns` 才是未过滤总量 |

`span_head` / `span_tail` 是先验 crit 链的最小 / 最大 tx。没有链时是 `usize::MAX`，此时过滤器把全部 post-exec 算进 `post_exec_validate_ns`（冷启动没有先验链）。中位墙是复用轮，有先验链。薄块链长 < 32，安装门会跳过，但过滤器仍用这条先验链。focus 在链长 < 32 时改用本块最长写者表；若 `span_head/tail` 和 focus `head_tx/tail_tx` 不一致，span 外那一列只是近似，表里会标明。

仍保留：`steal_top_ns`、`end_block_ns`、`seed_owner_local_pops`、`steal`、`wake_miss`（`exact_wake_missed_nopark`）、`defer`、`idle_parks`、`exact_wakes`、`idle_spins`。

## 什么能相加

宿主上三段互不重叠：

1. `admit_seed_ns` 在 spawn 之前。
2. spawn 循环本身没有单独探针（`admit_seed_ns` 结束到 `join_mark_origin_ns` 之间）。
3. `join_wait_ns` 从 spawn 结束到 scope 返回。
4. scope 返回到 `end_block` 计时起点之间还有一小段（脊报告拷贝、停车计数）。
5. `end_block_ns` 是 join 之后的学习阶段。

`join_wait_ns` 里包含链 span 的大部分，因为工人在 join 等待期间跑完首尾开工。join 对 tax 的贡献是 join 区间扣掉与 `[head, tail)` 的交集，不是整段 `join_wait_ns`。

同一工人上，execute、post-exec、idle 互斥。跨工人这三者和 `join_wait_ns` 重叠。`steal_top_ns` 在 `pick` 里，不在 post-exec 里，也不在 idle 里，但是各核之和，不是墙。

所以不能把 `post_exec_validate_ns + idle_ns + join_wait_ns + admit_seed_ns + end_block_ns` 当成 tax。能直接从 tax 里扣的宿主片只有整段落在 span 外的 `admit_seed_ns` 和 `end_block_ns`。其余残差要单独标。

## Soft=0 实测

数字在测量完成后写入。不预填。
