# SpecFence v2 stage 1

## 目标

在 upstream pevm `e94b0e3` 的干净副本上实现 SpecFence 第一阶段：自有引擎、带值的读源校验、块内有序写者链、下标播种的 Chase-Lev，以及 SEQ / OCC / SF 计时。上游 OCC 路径在不启用 `specfence` 时不参与编译。

## 约束

- 基线分支 `specfence-v2` 等于 `e94b0e3`。功能分支 `cursor/specfence-v2-stage1-0989`，PR 对 `specfence-v2`。
- `vm.rs`、`mv_memory.rs`、`scheduler.rs`、`pevm.rs` 保持上游字节级内容。
- 退出条件是 `committed_upto == n` 且每条读集都对照最后一次写做过身份和值的检查。没有 PartialAbortRebind。
- 类键至少 `(to, selector)` 与 `(code_hash, selector)` 可切换。链预算在安全界内在线调整。

## 步骤

1. **已完成** — 干净基线 `mainnet` 等价：块 15274915（gas 29928443）与 3356896（gas 4033966）顺序执行与 OCC 都对得上链上收据根、bloom 和 gas。
2. **已完成** — `src/specfence/` 引擎、值校验、Chase-Lev、两种类键。
3. **已完成** — 前缀卡住的原因：读等待自旋约 1 秒，以及 nonce 阻塞在已提交的 `tx-1` 上后把同一笔交易反复压回队首。改为短等待、阻塞同一发送者的前序，前序已结束则让出前缀。
4. **已完成** — 盲写和 lazy 写进入读者可见的链，但不进入 admission。受益人账户不进链。播种改为按 worker 跨步，第一波是 `0..C`。
5. **已完成** — 链上等价（C=1/4/8，两种类键）和 C=4/8 各 12 次 `seq=par` 通过。计数、墙钟和偏差写在 `docs/specfence-v2-stage1.md`。FullReplay 未稳定落在设计上限内，原因是写集合要等解释器返回才发布。
6. **已完成** — 换用 PR #62 的 scan / report / step-ideal。OCC 是本树的 `Pevm::execute_revm_parallel`。墙钟表在 `docs/specfence-v2-stage1.md`。`seqcheck` 与 `occcheck` 的 `diverge=0`。FullReplay 上限仍未稳定达到。

## 未纳入本阶段

持久线程池、`C_eff`、Ideal-ready、late split、IntraPatch、QuietExit、解释器返回前的提前发布、opcode 级 SSTORE 钩子。
