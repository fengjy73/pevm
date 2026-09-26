# SpecFence v2 stage 2b

## 目标

在 Stage 2（PR #71，`3164d51`）之上，查清并行路径解释器膨胀，并让活跃集按就绪宽度伸缩。新 PR 的基线是 `cursor/specfence-v2-stage2-afe4`。

## 约束

- 上游 `vm.rs`、`mv_memory.rs`、`scheduler.rs`、`pevm.rs` 字节级不变。
- SpecFence 仍在 `crates/pevm/src/specfence/`，仅 `feature = specfence`。
- 不替换 opcode，保留 `static_gas()`。
- 新鲜运行、无预热。K=10。线程池可复用，学到的状态每块重置。
- 不改上游 OCC。单独量线程 spawn/join。

## 完成标准

- 解释器按快路径、强制并行 1 工人、C=4 拆开，并按普通转账、热合约、其它分类。
- 单条热链的等待不在仍有就绪工作时缩小活跃集。
- 非热链空洞有位置和等待原因。
- 正确性测试通过。文档写入 `docs/specfence-v2-stage2b.md`。
- 本机 4 vCPU 上的速度门可以失败，但必须写明数字。ict21 重测才是速度门的权威结果。

## 步骤

1. **已完成** — 就绪宽度控制器、粘性交接倒出本队、前驱 boost、冷读缓存、懒发布、发送方与接收方合并预置。
2. **已完成** — abort 诊断。剩余 FullReplay 是合约内部碰到的基本账户，不在 `tx.to` / `tx.caller` 上。
3. **已回退** — 提交窗口、全部合约串行、执行末尾原地重试。三者分别拉长热链空隙、把活跃集收到 1、或在 C=8 上停住前缀。
4. **已完成** — 本机 K=10 扫描、桶归因、OCC spawn、正确性测试、`docs/specfence-v2-stage2b.md`。

## 结果

速度门在这台 4 vCPU 上未过：15274915 `(to, selector)` SF(4) 6.442 ms，SF(1) 4.498 ms，解释器 5.275 / 2.435 = 2.17 倍。SF(8) 相对 SF(4) 的 1.1 倍在该键上通过。`delta_mismatch` 为 0。细节在 stage 2b 文档。草稿保留到用户验收。
