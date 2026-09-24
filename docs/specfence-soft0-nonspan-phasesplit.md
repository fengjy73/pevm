# Soft=0 非 span 相探针

**日期：** 2026-09-24（北京时间）
**代码：** `8832029`（探针；本篇只补数字，不改二进制）
**起点：** AdmitShard `cursor/admitshard-core-d1f2` @ `aa448cb`（调度 `1fa3308`）
**PR：** https://github.com/fengjy73/pevm/pull/51

## 结论

两块 Soft=0 都量到了。愿望线 **未到**：薄块 ratio **0.697**，大块 **0.683**，`ge_1_5=false`。

宿主墙上，tax 可以拆成四段互不重叠的时间：span 之外的 join 之前、span 之外的 join、`end_block_ns`、以及没单独探针的残差（`execute()` 在 `exec_origin` 之前的准备，加上 scope 返回到 `end_block` 再回到 `execute` 返回的缝）。大块中位墙这四段是 **0.672 + 3.046 + 0.338 + 1.165 = 5.221 ms**，等于 tax。工人上的 `post_exec_*` / `idle_ns` / `heal_ns` / `steal_top_ns` 是各核之和，落在 join 里面，**不能再加进 tax**。

薄块中位墙落在第一次复用。过滤器的链头是 **31**，focus 的链头是 **4**，所以这一轮的 `post_exec_validate_ns` 不是 focus span 的外侧。大块中位墙链头链尾都是 **116 / 1219**，和 focus 一致。

## 探针窗口

常开，不进 `SPECFENCE_PROFILE`。打在 compare 的 `spine` 行和 `TPS_SUMMARY` 上。

| 名 | 窗口 | 时钟 |
| --- | --- | --- |
| `admit_seed_ns` | SpecFence 块初到 `thread::scope` 之前：prior sketch、arms、`admit_seed_begin_block`、crit install、`seed_begin` 把 AdmitIndep 切进每核 deque，以及随后的 `SpecFenceCtx` 装配。不含更早的 `MvMemory` / 调度器分配，不含 spawn 循环。 | 宿主墙。整段在工人开工前 |
| `join_wait_ns` | 最后一次 `scope.spawn` 返回之后，到 `thread::scope` 返回。 | 宿主墙。覆盖工人尾段，含链 span 的绝大部分 |
| `join_mark_origin_ns` | `exec_origin` 到 join 开始。与 focus 的 `head_ms` / `tail_ms` 同一原点。 | 用来和 span 相交 |
| `idle_ns` | `pick → None` 整段：heal、yield、spin、`park_idle`。不含那次返回 `None` 的 `pick`。 | 各工人之和，含 `heal_ns` |
| `heal_ns` | 空档里的 `heal_finished_preds`、sleeper wake、`heal`、`force_idle_recover`，以及旁边的 `drain_wave`。不含 yield / spin / park。 | `idle_ns` 的子集 |
| `post_exec_validate_ns` | 执行成功后的 `drain_wave` + `validate_to_plan` + `resolve_plan::apply`，以及 `Task::Validation` 臂。按窗口**起点**分类，起点在先验链 `[head.first_start, tail.first_start)` 之外才计入。起点落在开着的 span 里，整段算 span 内，即使链尾在窗口中途开工。 | 各工人之和 |
| `post_exec_in_span_ns` | 同一窗口，起点在开着的 span 内。两者相加是未过滤总和。 | 各工人之和 |

链端先取 `inter_prior` 的 crit（大块 sticky ≥32）。那条链空着时用 `SpinePrior::chains`（上一块最长写者表，和本块 `ordered_writers` 同一份）。没有链时 `span_head/tail` 是 `usize::MAX`，全部 post-exec 进 `post_exec_validate_ns`。冷启动就是这样。

仍保留：`steal_top_ns`、`end_block_ns`、`seed_owner_local_pops`、`steal`、`wake_miss`、`defer`、`idle_parks`、`exact_wakes`、`idle_spins`。

## 协议

Instant-off（不设 `SPECFENCE_PROFILE`、不设 `SPECFENCE_HANG_TRACE`），release，`profile.release.lto=false`，N=5，请求 8 核，宿主 4 核。主指标是四次复用墙的 `sorted[len/2]`。`tax_ms = SF_wall − span`，span 取中位墙那一轮 focus 的 `tail_ms − head_ms`。

中位墙两轮都是 `est_block=0`、`soft=0`、`occ_picks=0`、`spine_cores_max=1`。2 核冷检（`SPECFENCE_COMPARE_CHECK=1`、`CORES=2`、`ITERS=1`）两块各打印一次 `seq=par ok`。

## 同宿主墙

笔记里的 AdmitShard 数字是上一台测量，不是这次进程。OCC 对不上，所以把 `aa448cb` 在这台机器上按同一命令重跑了一遍。两次 N=5 的复用墙差一截，不能把探针二进制相对笔记的墙差说成探针开销。

| 来源 | 块 | ratio | SF ms | OCC ms | span | tax ms | 中位墙落在 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 笔记 AdmitShard | 3356896 | 0.701 | 1.343 | 0.942 | 0.299 | 1.044 | 笔记 iter 3 |
| 本机 `aa448cb` | 3356896 | 0.519 | 1.767 | 0.917 | 1.063 | 0.704 | 第一次复用，span 被拉到 1.063 |
| **本机探针** | **3356896** | **0.697** | **1.442** | **1.006** | **0.582** | **0.860** | **iter 1** |
| 笔记 AdmitShard | 15274915 | 0.737 | 7.359 | 5.420 | 1.497 | 5.862 | 笔记 iter 4 |
| 本机 `aa448cb` | 15274915 | 0.747 | 6.841 | 5.111 | 2.024 | 4.817 | iter 3 |
| **本机探针** | **15274915** | **0.683** | **7.615** | **5.202** | **2.394** | **5.221** | **iter 4** |

探针这次的复用墙：薄块 **1.442、1.471、1.352、1.378**（冷 2.532）；大块 **8.211、6.839、7.613、7.615**（冷 20.692）。TPS：薄块 SF **122014.5** / OCC **175011.5**；大块 SF **160995.6** / OCC **235696.9**。`ge_1_5=false`。

## 中位墙那一轮

薄块 iter 1 的 `span_head=31`，focus `head_tx=4`（尾都是 171）。这一行的 post-exec 外侧是相对先验链，不是相对 focus span。大块 iter 4 的 **116 / 1219** 与 focus 一致。

| 块 | wall | span | tax | defer | steal | local | handoff | wake_miss | exact | parks | spins | help |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 3356896 iter 1 | 1.442 | 0.582 | 0.860 | 68 | 79 | 152 | 15 | 80 | 4 | 7 | 31 | 3 |
| 15274915 iter 4 | 7.615 | 2.394 | 5.221 | 0 | 365 | 821 | 76 | 176 | 3 | 3 | 30 | 5 |

| 块 | admit_seed_ns | join_wait_ns | join_mark_origin_ns | idle_ns | heal_ns | post_exec_validate_ns | post_exec_in_span_ns | steal_top_ns | end_block_ns | span 端 | 与 focus |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- | --- |
| 3356896 | 49275 | 799458 | 215360 | 1559657 | 1052238 | 697802 | 43378 | 24076 | 101109 | 31→171 | 头不一致（focus 4→171） |
| 15274915 | 245290 | 5425843 | 685899 | 6496530 | 3248939 | 837860 | 2808357 | 65642 | 338116 | 116→1219 | 一致 |

换算成毫秒（纳秒 / 1e6）：

| 块 | admit_seed | join_wait | idle | heal | post 外 | post 内 | post 合计 | steal_top | end_block |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 3356896 | 0.049 | 0.799 | 1.560 | 1.052 | 0.698 | 0.043 | 0.741 | 0.024 | 0.101 |
| 15274915 | 0.245 | 5.426 | 6.497 | 3.249 | 0.838 | 2.808 | 3.646 | 0.066 | 0.338 |

`heal_ns` ≤ `idle_ns`。`idle − heal` 是 yield / spin / park 的和：薄块 0.507 ms，大块 3.248 ms，仍是各核之和。

## 和 tax 怎么加

只加宿主上、并且落在 focus span 外面的段。`join` 与 `[head_ms, tail_ms)` 的交集从 `join_wait_ns` 里扣掉。`head_ms` / `tail_ms` 和 `join_mark_origin_ns` 都从 `exec_origin` 起算。

大块 iter 4（链端与 focus 一致）：

| 段 | ms | 说明 |
| --- | ---: | --- |
| join 之前、span 之外 | 0.672 | 含 `admit_seed` 0.245。其余是 `exec_origin` 到 spawn 结束之间的 `MvMemory`、调度器、spawn |
| join 之中、span 之外 | 3.046 | `join_wait` 5.426，与 span 重叠 2.380 |
| `end_block_ns` | 0.338 | join 之后的学习 |
| 残差 | 1.165 | `execute()` 在 `exec_origin` 之前，以及 scope 返回到 `end_block`、再到 `execute` 返回的缝 |
| 合计 | 5.221 | 等于 tax |

薄块 iter 1（join 与 **focus** span 相交；post-exec 分类用的是另一条链，所以 post-exec 不进这张表）：

| 段 | ms |
| --- | ---: |
| join 之前、span 之外 | 0.215 |
| join 之中、span 之外 | 0.217 |
| `end_block_ns` | 0.101 |
| 残差 | 0.326 |
| 合计 | 0.860 |

四段的纳秒是 215360 + 217458 + 101109 + 326073 = 860000，等于 tax。上表毫秒是四舍五入，直接相加是 0.859。

工人累计都落在上面的宿主窗里，再加就会重复：

- `post_exec_validate_ns + post_exec_in_span_ns` 与 `idle_ns` 在同一工人上不重叠，跨工人重叠，也和 `join_wait_ns` 重叠。
- `steal_top_ns` 在 `pick` 里，不在 post-exec 里，也不在 idle 里，但是各核之和。
- `post_exec_in_span_ns` 的窗口起点在 span 内，不属于 tax。
- 大块对齐的一轮里，post-exec 合计 3.646 ms 里有 2.808 ms 被标进 span 内，外侧只有 0.838 ms，而且是和。它解释不了 5.221 ms 的 tax。宿主上 span 之外的 join（3.046 ms）比这列大。

薄块中位轮的 post-exec 外侧 **不能**当成 focus span 的外侧。链端对齐的复用轮（iter 2/3/4）上，`post_exec_validate_ns` 是 81467 / 92032 / 48475 ns，`post_exec_in_span_ns` 是 363465 / 116503 / 466584 ns。对齐之后，外侧只有几十微秒的和。

## 复用探针中位

四次复用各自排序后取 `sorted[2]`。这不是中位墙那一轮。

| 块 | defer | steal | local | handoff | wake_miss | exact | parks | spins | steal_top_ns | end_block_ns | admit_seed_ns | join_wait_ns | idle_ns | heal_ns | post 外 | post 内 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 3356896 | 3 | 80 | 96 | 16 | 13 | 0 | 0 | 24 | 21299 | 105546 | 49275 | 799458 | 1227782 | 633294 | 92032 | 363465 |
| 15274915 | 0 | 365 | 865 | 76 | 176 | 3 | 3 | 25 | 65642 | 336425 | 223049 | 5437697 | 5187903 | 2791799 | 1468106 | 2604668 |

## 读法

- **协议约束是满足的。** 两块中位墙 `est=0`、`soft=0`、`occ_picks=0`、`spine_cores_max=1`。2 核冷检 `seq=par`。薄块这一轮 handoff 是 15、脊链长 16，focus 链长 17；大块 handoff 76、链长 77。
- **tax 的宿主分解是齐的，残差是没探针的缝，不是一笔未知的工人相。** 大块残差 1.165 ms，薄块 0.326 ms。
- **post-exec 外侧不是大块 tax 的主体。** 对齐的一轮里，外侧和是 0.838 ms，span 内的和是 2.808 ms。
- **1.5 未到。** 0.697 / 0.683。同机 `aa448cb` 重跑的 ratio 是 0.519 / 0.747，笔记是 0.701 / 0.737。墙在这台 4 核上晃，探针这一轮不能单独证明比 AdmitShard 快或慢。
