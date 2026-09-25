# SpecFence 块退出与读值校验

**目标：** 在 PR #64 之上修 SpecFence 的两处正确性洞，使块结束时每一笔交易的读集都对着最终版本校验过，并且原点 `(tx_idx, incarnation)` 相同不能再放过一个不同的值。

**约束：** 不改学习策略和调度设计，除非正确性必须动到退出条件或验证。探针用完即删。

**完成标准：**

- 写清两个根因、不变量、修复和证据，落在 `docs/specfence-exit-validation-soundness.md`。
- 加宽 Commit 窗口或确定性交错能复现分叉；修复后同一路径不再分叉；然后撤掉探针。
- C=4 与 C=8 上 15274915、3356896 各几十次产品路径 `seq=par`，加上仓库 mainnet 测试。

## 步骤

1. **探针复现** — 已完成。加宽 Commit 窗口（2ms×8、5ms×20）在 PR #64 的脏标志之后全部 `seq=par`，`invalid_n=0`，没有同 incarnation 原地改写。确定性钩子 `SPECFENCE_SKIP_FANOUT=1` 复现了退出洞：15274915 C=4 退出时 73 笔读集已无效，`seq!=par`（第一处 tx 105）。修复后同一钩子 `invalid_n=0` 且 `seq=par`。钩子已删。
2. **根因** — 已完成。见正式文档。退出只看调度计数；原点不比值；预验证的 lazy/tx0 走 Closed 不通知更高读者；同 incarnation 把 Estimate 换成另一个 Data，或快进快照在身份相同时代回旧字节。
3. **修复** — 已完成。读原点带上 `MemoryValue`；最后一核在退出前按写序号锁再验；预验证写者仍 `enqueue_higher_revalidate` 并 `mark_done`；快进在身份相同但字节不同时拒绝快照。
4. **撤探针并复跑** — 已完成。探针已从树上删除。产品路径 15274915 与 3356896、C=4 与 C=8 各 12 次，48/48 `seq=par`。单测通过。`rise_blocks_from_disk` 通过。`mainnet_blocks_from_disk` 在父提交上就因块头收据根 / gas 对不上而失败；15274915 与 3356896 的失败哈希与父提交相同，且都发生在顺序结果已经等于并行结果之后。
