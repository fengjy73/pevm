# AdmitShard

**日期：** 2026-09-24（北京时间）
**起点：** `cursor/idlestalewake-core-bc12` @ `ac9e82d`（Soft=0 代码 `21802b1`）。不是 HPC `16bce1f`。

## 结论

AdmitIndep 在播种时按索引切成与核数相等的连续段，放进每核私有的 Chase-Lev deque。所有者 `push_bottom` / `pop_bottom`（LIFO）。本地空了才 `pop_top`（FIFO）。不再有「全部 Indep 进 worker 0、其余核抢同一把 `Mutex<VecDeque>`」。

SpineHop / OrderedTip 仍只在 `SpineHandoffSlot`。学到的 WAW 后继在播种时 `mark_wait`，不会进可偷 deque。`spine_cores` 的上限逻辑没改。

Soft=0 数字在实测后写入下一节，不在这里预填。

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

协议与 #49 相同：Instant-off（不设 `SPECFENCE_PROFILE`）、release、LTO off、N=5、请求 8 核。主指标是 reuse-median TPS 的 SF/OCC。`tax_ms = SF_wall − span`，span 取墙时等于该中位的那一轮。

对照只引用已测数字：

| 来源 | 块 | ratio | SF_ms | OCC_ms | span | tax | steal |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 长链基线 | 3356896 | 0.690 | 1.459 | 1.006 | ~0.42 | ~+1.0 | — |
| 长链基线 | 15274915 | 0.707 | 7.542 | 5.332 | ~1.44 | ~+6.1 | — |
| IdleStealWake #49 | 3356896 | 0.673 | 1.538 | 1.036 | 0.348 | 1.190 | 147 |
| IdleStealWake #49 | 15274915 | 0.740 | 7.776 | 5.756 | 1.496 | 6.280 | 1081 |

本刀的表在同一宿主实测后补上。愿望线 ≥1.5。未达到就记未达到。
