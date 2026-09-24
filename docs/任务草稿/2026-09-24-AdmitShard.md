# AdmitShard

**日期：** 2026-09-24（北京时间）
**起点：** `cursor/idlestalewake-core-bc12` @ `ac9e82d`（Soft=0 代码 `21802b1`）。不从 HPC `16bce1f` 起。

## 目标

把 AdmitIndep 从「只种在 worker 0、其余核抢一把全局 Mutex」改成分核 LocalAdmitDeque（所有者 LIFO / 小偷 FIFO，Chase-Lev）。SpineHop 仍只在 SpineHandoffSlot。然后在 Soft=0 导出 `steal_top_ns`、`seed_owner_local_pops`、`end_block_ns`、`exact_wake_missed_nopark`。对照 IdleStealWake 中位：薄 0.673（1.538/1.036，span 0.348，tax 1.190，steal 147），大 0.740（7.776/5.756，span 1.496，tax 6.280，steal 1081）。愿望线 ≥1.5，未达如实报。

## 约束

- 只把 AdmitIndep 放进分核 deque。脊跳 / OrderedTip 不进可偷队列。`spine_cores≤1`。
- 播种不用朴素 `tx % C`。那一版让每个核都从低索引开工，2 核上 15274915 `seq≠par`。
- 按索引连续、数量均衡地切开真 AdmitIndep。worker 0 的低段 LIFO 先弹出低索引（前缀留在所有者，小偷从高端拿）。其余核的段 LIFO 先弹出该段高端，避免一开工就进前缀。
- Soft=0 Instant-off：`estimate_block_sf=0`，`soft_wait_arms=0`，`occ_picks=0`，`seq≡par`。
- 不叠 HPC T0–T6，不用 Estimate 门控 Avoid，不 park-all，不把 #47 suspend 当 Avoid。ExactWake 接线缺口本刀不修。

## 完成标准

1. 新分支 + 新 PR（基分支 `cursor/idlestalewake-core-bc12`）。
2. 两焦点块 Soft=0 表（协议同 #49）写进笔记和 PR，含 steal vs local_pops、探针、tax、是否 ≥1.5。
3. 2 核 `seq≡par` 在两焦点块上复测通过后再报 N=5。

## 步骤

1. **进行中** — Chase-Lev `LocalAdmitDeque` + 连续分片播种 + 四枚探针进 SpineReport / Soft=0 日志。
2. **待做** — 单元测试，再 2 核正确性，再 N=5 release、LTO off、请求 8 核。
3. **待做** — 用实测数字写 `docs/specfence-admitshard.md` 和 PR，不编墙时。
