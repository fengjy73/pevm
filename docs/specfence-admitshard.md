# AdmitShard

**日期：** 2026-09-24（北京时间）
**起点：** `cursor/idlestalewake-core-bc12` @ `ac9e82d`（Soft=0 代码 `21802b1`）。不是 HPC `16bce1f`。

## 结论

AdmitIndep 在播种时按索引切成与核数相等的连续段，放进每核私有的 Chase-Lev deque。所有者 `push_bottom` / `pop_bottom`（LIFO）。本地空了才 `pop_top`（FIFO）。不再有「全部 Indep 进 worker 0、其余核抢同一把 `Mutex<VecDeque>`」。

SpineHop / OrderedTip 仍只在 `SpineHandoffSlot`。学到的 WAW 后继在播种时 `mark_wait`，不会进可偷 deque。`spine_cores` 的上限逻辑没改。

同宿主 Soft=0（N=5，请求 8 核）：薄块 ratio **0.701**（SF 1.343 / OCC 0.942，tax 1.044），大块 **0.737**（7.359 / 5.420，tax 5.862）。相对同机 IdleStealWake，SF 墙和 steal 都下降。愿望线 **1.5 未到**。数字在文末，不外推。

## 播种

朴素 `tx % C` 会让每个核的 LIFO 都从自己的低索引开工。PR #49 在 2 核上把 15274915 跑成 `seq≠par`（前缀里的 Uniswap 交易并行提交了和顺序路径不同的收据）。单 deque 能过，是因为只有 worker 0 吃低索引，其余核从高端偷。

本刀不用取模：

1. 只收集真正的 AdmitIndep（未门控、没有未完成的 blocking producer、不是链头）。
2. 按 tx 索引升序切成 C 段，段长相差不超过 1。
3. **段 0** 从高到低 `push_bottom`。worker 0 的 LIFO 先弹出低索引（前缀留在所有者）。小偷 `pop_top` 拿走这段的高端。
4. **其余段** 从低到高 `push_bottom`。这些核的 LIFO 先弹出本段高端，一开工不进全局前缀。
5. 链头若仍是 AdmitIndep，最后推进 worker 0，所以它是 worker 0 的第一笔 LIFO。

运行中新产生的 AdmitIndep 仍进当前工人自己的 deque（线程本地所有者），不回到全局锁。

## 探针

导出到 `SpineReport`，并打印在 Soft=0 的 `spine` 行和 `TPS_SUMMARY` 上。

| 名 | 含义 |
| --- | --- |
| `seed_owner_local_pops` | 所有者 `pop_bottom` 成功取到 `ST_INDEP` 的次数（含随后 refuse）。不是 steal。 |
| `steal_top_ns` | 各工人花在小偷 `pop_top` 上的纳秒之和。不是墙钟。Mutex 已去掉，所以不叫 `steal_mutex_ns`。 |
| `end_block_ns` | 工人 join 之后的 end_block 学习阶段，与 `LearnReport::end_block_ns` 同一段。 |
| `exact_wake_missed_nopark` | `exact_wake_one` 扫完没有发现 parked 核。播种时的 AdmitIndep 不调用 ExactWake（工人还没起来）。 |

`steal` 仍是跨核成功声明的 AdmitIndep 次数。

## Soft=0

协议与 #49 相同：Instant-off（不设 `SPECFENCE_PROFILE`、去掉 `SPECFENCE_HANG_TRACE`）、release、LTO off、N=5、请求 8 核、宿主 4 核。主指标是 reuse-median TPS 的 SF/OCC（`sorted[len/2]`，四次复用取排序后的第 3 个）。`tax_ms = SF_wall − span`，span 取墙时等于该中位的那一轮。`seq≡par` 用 `SPECFENCE_COMPARE_CHECK=1`。

代码提交 `1fa3308`。同宿主对照是把 `ac9e82d` 在这台机器上按同一命令重跑，不是把下面引用的 #49 旧墙当成这次的 OCC。

### 引用（不是这次宿主）

| 来源 | 块 | ratio | SF_ms | OCC_ms | span | tax | steal |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 长链基线 | 3356896 | 0.690 | 1.459 | 1.006 | ~0.42 | ~+1.0 | — |
| 长链基线 | 15274915 | 0.707 | 7.542 | 5.332 | ~1.44 | ~+6.1 | — |
| IdleStealWake #49 原文 | 3356896 | 0.673 | 1.538 | 1.036 | 0.348 | 1.190 | 147 |
| IdleStealWake #49 原文 | 15274915 | 0.740 | 7.776 | 5.756 | 1.496 | 6.280 | 1081 |

### 这次宿主

| 来源 | 块 | ratio | SF_ms | OCC_ms | span | tax_ms | steal | local_pops | seq | spine_cores_max |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| IdleStealWake `ac9e82d` | 3356896 | 0.621 | 1.562 | 0.969 | 0.326 | 1.236 | 122 | — | par | 1 |
| **AdmitShard** | 3356896 | **0.701** | **1.343** | 0.942 | 0.299 | **1.044** | **61** | **98** | par | 1 |
| IdleStealWake `ac9e82d` | 15274915 | 0.648 | 8.330 | 5.397 | 1.774 | 6.556 | 1078 | — | par | 1 |
| **AdmitShard** | 15274915 | **0.737** | **7.359** | 5.420 | 1.497 | **5.862** | **296** | **877** | par | 1 |

TPS：薄块 SF **131021.7** / OCC **186905.6**；大块 SF **166606.8** / OCC **226192.2**。`ge_1_5=false`。愿望线 **1.5 未到**。

复用 span：薄块 **1.078、0.445、0.299、0.285**（中位墙是 1.343 ms 那一轮，span 0.299）；大块 **6.385、2.775、1.607、1.497**（中位墙 7.359 ms，span 1.497）。中位轮 handoff = L−1（16 / 76）。`est_block=0`，`soft=0`，`occ_picks=0`。

2 核冷检 `SPECFENCE_COMPARE_CHECK=1`、iters=1：两块各 **4/4** `seq=par`。

### 中位墙那一轮的探针

| 块 | defer | steal | local_pops | steal_top_ns | end_block_ns | wake_miss | exact_wakes | idle_parks | help |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 3356896 | 1 | 61 | 98 | 12464 | 96169 | 12 | 0 | 0 | 2 |
| 15274915 | 0 | 296 | 877 | 49620 | 79889 | 175 | 1 | 1 | 8 |

`steal_top_ns` 和 `end_block_ns` 是纳秒。前者是各核 `pop_top` 耗时之和，不是墙钟。薄块约 0.012 ms、大块约 0.050 ms，相对 tax 1.044 / 5.862 ms 很小。`end_block_ns` 约 0.096 / 0.080 ms，也不是税的主体。

四次复用按同一 `sorted[len/2]` 取的探针中位：薄块 steal 63、local_pops 111、steal_top_ns 13349、end_block_ns 136645、wake_miss 12、exact_wakes 1、idle_parks 1。大块 steal 296、local_pops 1000、steal_top_ns 49620、end_block_ns 79889、wake_miss 175、exact_wakes 3、idle_parks 3。

### 读法

- **协议是绿的。** 两块 `seq≡par`，`est=0`，`soft=0`，`occ_picks=0`，复用 `spine_cores_max=1`，handoff 16/76。2 核没有回到「每个核都从前缀开工」的 `seq≠par`。
- **steal 不再近似 Indep。** 同宿主 IdleStealWake 中位轮 steal 122 / 1078；AdmitShard 61 / 296。local_pops 98 / 877，大块本地弹出明显多于偷取。Ideal pure_indep 仍是 145 / 1106，steal 已经离开这个量级。
- **同宿主 SF 墙下降，税仍在。** 薄块 SF 1.562→1.343（OCC 只 0.969→0.942），tax 1.236→1.044。大块 OCC 几乎不动（5.397→5.420），SF 8.330→7.359，tax 6.556→5.862，ratio 0.648→0.737。偷取锁的时间不再解释这笔税。
- **1.5 未达到。** 0.701 / 0.737。
- 第一次复用仍可能很脏：薄块有一轮 defer=424、span 1.078；大块有一轮 defer=3579、span 6.385。中位没有落在这两轮上。同宿主的 IdleStealWake 也有脏轮（薄 defer=282；大块墙 19.761 ms）。
