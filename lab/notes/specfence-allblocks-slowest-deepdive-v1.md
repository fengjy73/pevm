# 全量扫块 × 最慢块多维开销深挖（分析任务）

**基线:** PR #34 `cursor/specfence-prepaid-losers-b5de`  
**用户:** 先全量跑所有区块；对最慢几个做多方面 + 开销细粒度深挖，定位问题  
**性质:** **分析为主**；可提交笔记/结果 JSON；**不改** CC/policy 行为（除非为跑 harness 的只读/日志开关）  
**Soft=0；OptimisticRead/OrderedAdmit；不发明 ns**

---

## 1. 全量

- 用现有 `specfence_all_blocks_sweep` / mainnet sweep（及 ALL_REUSE 若适用）跑 **全部可加载块**  
- 报告：每块 OCC/SF wall、reuse、臂、unfenced、double_pay、ratio；排序找 **最慢**（绝对 SF wall 与 SF/OCC 比 各一榜）  
- Soft=0 全行确认

## 2. 最慢 K 块深挖（建议 K=5–8）

对每块多方面：

| 面 | 内容 |
|----|------|
| **冲突结构** | 主 WAW/RAW 脊、ℓ、writer 数、Basic vs storage |
| **臂/学习** | select_arm 轨迹、w_need、cover、Opt 撤 |
| **CC 计数** | unfenced、inc、refuse、dp、commute |
| **开销桶** | Instant-off 墙；PROFILE Instant-tax（标清不可加总）；end_block、prepaid/stall、reexec_ns |
| **PC** | occ_pick_while_gated、核忙闲若可测 |
| **归类** | U/O/D/L4/S 或新类；必要 vs 不必要开销 |

## 3. 交付

1. `lab/notes/…-allblocks-sweep-….md` + summary json  
2. `lab/notes/…-slowest-deepdive-….md` 中文细挖（每慢块一节 + 总定位）  
3. Draft PR **仅文档/结果**（或笔记在 lab 路径）；**不 merge CC**  
4. 列出下一刀假设（不落地）

