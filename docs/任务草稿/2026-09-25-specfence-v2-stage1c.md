# SpecFence v2 stage 1c

## 目标

在 Stage 1b 的基础上，解释 C>1 的反伸缩，并按实测根因改调度。门是 SF C=4 墙钟不超过 OCC C=4，且快于 SF C=1。PR 基线是 `cursor/specfence-v2-stage1b-ac6e`，工作分支 `cursor/specfence-v2-stage1c-d3e2`。

## 约束

- 上游 `vm.rs`、`mv_memory.rs`、`scheduler.rs`、`pevm.rs` 字节级不变。
- 不替换 opcode，保留 `static_gas()`。
- SEQ = OCC = SF = 链上头，块 15274915 与 3356896。
- 计时跑关掉计时器。归因先于改规则。新鲜运行、无预热。
- FullReplay：15274915 ≤ 12，3356896 ≤ 5。两种类键都报。
- 正确性覆盖 RAW、WAR、WAW 和写者链。参数保持自适应。

## 完成标准

- C=4 与 C=8、两块、两种类键的时间线，以及同一次运行上写者链和 Ideal_C 的分段。
- 用数字确认或否定 (a) 提交前缀等待、(b) 类头屏障、(c) 只在提交头验证、(d) 多工人簿记。
- 终局性沿读依赖传播。提前异步验证。类头和同类预测仅在本块冲突证据之后打开。
- 门未达到时，文档写明前后数字和仍开着的临界路径，不把未过的门写成已过。

## 步骤

1. **已完成** — 时间线在改规则之前。主机 4 vCPU、Xeon family 6 model 207、L3 320 MiB。15274915 `(to, selector)` C=4：77 个非 lazy 写者，执行合计 0.596 ms，链跨度 4.182 ms，内部墙钟 7.091 ms。76/76 跳在前一写者提交之后才开始；提交滞后 3.495 ms，占跳间空隙 3.586 ms 的 97%。C=8 同一条链跨度 7.305 ms。`(code_hash, selector)` 的类 6 admission 是 4220 ms / 1064 次停车。验证 CPU 0.34 ms。结论：(a)(c) 成立；(b) 类头本身不是墙钟，同类 admission 在 `code_hash` 上是额外串行；(d) 是执行内的线程时间，小于链跨度。同一次运行的列表调度理想值含进了交易时长里的等待，不能当 Ideal_C。
2. **已完成** — 依赖闭合终局性、提交头之外的验证、有证据才开类头和同类预测、存储读不再被更低的预置成员挡住、最终洞一次清完。预置链保留。单测覆盖级联、晚写者撤销、abort/估计清掉终局性、洞、间隙成员、提交前唤醒。
3. **已完成** — K=10 扫描，两种类键，`results/stage1c-scan-to` 与 `results/stage1c-scan-code`。门未过：SF C=4 仍高于 OCC C=4，也高于 SF C=1。FullReplay 在 15274915 最大 8；3356896 有一轮 C=8 `(to, selector)` 为 16，其余多工人轮次为 0。去掉预置、跳过未开始的 Predicted、以及在飞窗口都已回退：FullReplay 超帽或墙钟更差。
4. **已完成** — Stage 1c 在 `docs/specfence-v2-stage1.md`。等价测试通过：`sf_matches_onchain_focus_blocks` 与 `sf_seq_par_repeat` 共 3.75s，`sload_static_gas_matches_chain_header` 0.50s。提交并更新 PR #69。

## 未决

- 门未过。剩下的临界路径是 997 人的预置最近写者链：终局性去掉了提交门，下一写者仍要等前一写者执行结束。
- 3356896、C=8、`(to, selector)` 十轮里有一轮 FullReplay 16。
- 草稿在用户验收前保留。
