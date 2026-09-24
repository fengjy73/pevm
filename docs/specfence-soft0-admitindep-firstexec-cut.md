# Soft=0：非链 AdmitIndep 首次 Opt 少记冷读元数据

**日期：** 2026-09-24（北京时间）
**代码：** `a253d6f`（QuietExit 尖 `a6f85fd` 之上）
**对照：** dig `114cdda`，见 `docs/specfence-soft0-joinafterspan-dig.md`。大块中位 SF **7.065** / OCC **5.306** / ratio **0.751** / span **2.089** / tax **4.976** / join-out **3.046**，链 **116/1219**。
**协议：** release、LTO off、Instant-off、N=5、请求 8 核、宿主 4 核、`taskset -c 0-3`。`ge_1_5=false`。

## 结论

这刀是 **(A)**，不是 **(B)**。大块复用中位 SF 墙是 **7.210 ms**，高于 dig 的 **7.065 ms**。span 从 2.089 拉到 3.312，join-out 从 3.046 收到 2.166。墙留在 dig 已经标出的 6.3–7.1 ms 带附近（这一轮中位略高，四次复用是 6.779 / 6.859 / 7.210 / 8.198）。只把反链赶在链尾开工之前做完，不算这刀成功。

QuietExit 没有改。大块中位行最后一次验证到最后一名工人离开是 **0.073 ms**。

## 这一刀改了什么

incarnation 0、未门控、不是有序链成员、也不是 leftover-min 的第一次 Opt：

- 整笔跳过 access ordinal、value snap、finegrain。
- 冷位置跳过 `spine_before_read` 的 peek 和 WaitOnce consult。
- 有序链位置、crit、protected、非 crit WaitOnce 仍走 Detect。
- 读到 Estimate 仍走 `live_writer_act`，不增加 `estimate_block_sf`。
- 中止整段重放，不用残缺前缀 rewind。

种子顺序、AdmitShard LIFO、HandoffSlot、WaitOnce 真 tip、QuietExit 都没动。没有 Estimate 门，没有 park-all，SpineHop 不进可偷 deque。

## 数字

复用中位是四次复用 SF 墙排序后的 `sorted[2]`，和 harness 的 `primary=reuse` 同一行。OCC 中位含冷启动，和 `TPS_SUMMARY` 一致。join-out 是 `join_wait_ns` 扣掉与 focus `[head_ms, tail_ms)` 的重叠。tax = SF 墙 − span。

| 块 | 行 | ratio | SF | OCC | span | tax | join-out | 链 | first_cut |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| 15274915 | 本次中位（iter 1） | 0.818 | 7.210 | 5.900 | 3.312 | 3.898 | 2.166 | 116/1219 | 1148 |
| 15274915 | dig 中位 | 0.751 | 7.065 | 5.306 | 2.089 | 4.976 | 3.046 | 116/1219 | — |
| 3356896 | 本次中位（iter 4） | 0.591 | 1.520 | 0.898 | 0.338 | 1.182 | 0.610 | 4/171 | 157 |
| 3356896 | NonSpan `8832029` | 0.697 | 1.442 | 1.006 | 0.582 | 0.860 | 0.217 | focus 头 4 | — |

大块四次复用都是链 116/1219，`est=0`，`soft=0`，`occ_picks=0`，`spine_cores_max=1`。薄块中位行同样满足这四项；冷启动轮 `spine_cores_max=0`，因为还没有学到的链。2 核冷检（`SPECFENCE_COMPARE_CHECK=1`、`CORES=2`、`ITERS=1`、`taskset -c 0-1`）两块各打印一次 `seq=par ok`。

大块 ratio 从 0.751 升到 0.818，是 OCC 中位从 5.306 漂到 5.900，SF 墙没有下降。薄块 SF 1.520 高于 NonSpan 的 1.442，和 QuietExit 两轮的 1.457 / 1.502 同一档；ratio 0.591 更低，因为这次 OCC 是 0.898。

## 为什么记成 (A)

dig 对齐三轮已经是这种交换：链尾开工越晚，span 越长，join-out 越短，墙留在 6.355 / 6.754 / 7.065。这次中位 span 3.312、join-out 2.166、墙 7.210，落在同一形状上，只是比 dig 中位更靠近「链尾偏晚」那一侧。

`first_cut` 在大块中位是 1148 / 1226，路径确实走过。少记的是冷读上的 HashMap 和 peek，不是解释器里那一截 OCC 也要做的执行。中位行 `full=13`：前缀 snap 被整笔跳过之后，中止不能从失败 k 续上，只能整段重放。
