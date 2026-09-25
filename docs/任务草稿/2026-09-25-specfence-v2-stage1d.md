# SpecFence v2 stage 1d

## 目标

在 Stage 1c 的依赖闭合终局性之上，把位置链上的成员分成 RMW 与输入确定的可交换增量。武装读只等最近较低的 RMW 终局写或终局洞；中间的增量不挡读，其金额折进读到的值。验证仍按 PR #65 用最终版本重算并要求相等。门：15274915 上 SF C=4 墙钟不超过 OCC C=4，且快于 SF C=1；热位置跨度不超过其 RMW 执行合计的约 2 倍；FullReplay 15274915 ≤ 12、3356896 ≤ 5。PR 基线是 `cursor/specfence-v2-stage1c-d3e2`。

## 约束

- 上游 `vm.rs`、`mv_memory.rs`、`scheduler.rs`、`pevm.rs` 字节级不变。
- 不替换 opcode，保留 `static_gas()`。
- SEQ = OCC = SF = 链上头，块 15274915 与 3356896。
- 预置链、值比较和最终重扫保留。增量不是 admission 边。
- 参数保持自适应，不写死阈值。
- 3356896 的 C=4 墙钟不设门，只报告。
- 新鲜运行、无预热。K=10，SEQ 只作一次 1 核基线。

## 完成标准

- 修正 Stage 1c admission 停车的聚合（求和超过墙钟×线程）。若修正后 `(code_hash, selector)` 的 admission 仍是大串行点，增量同学不作为 admission 边。
- 单测：增量失败、预测增量变成 RMW、更低处后出现 RMW、与受益人 lazy 奖励交错。
- 15274915 C=4 `(to, selector)` 热位置的前后跨度、每跳阻塞成员数、跳间空隙、执行时间。
- 与 Stage 1c 相同的时间线切分，前后都有。增量预测不符的次数和因此 abort 的次数。
- 两块、两种类键、C=1/4/8 的 SF/OCC 与 SF/Ideal_C。等价测试通过。
- `docs/specfence-v2-stage1.md` 增加 Stage 1d。

## 步骤

1. **已完成** — Stage 1c 二进制时间线在 `/tmp/stage1d-before/`。停车脚本改为区间并集（coverage）。`(code_hash, selector)` C=4 admission 的 coverage 是 0.084 ms（1 次停车），不是 4220 ms。4220 ms 是 1064 段重叠驻留时间之和。
2. **已完成** — 链成员分型。无代码收款方的 `tx.value` 在预置时标成增量。发布时若不是 lazy credit 则改回 RMW。验证不等则 abort 并撤销读者。
3. **已完成** — `nearest_blocker` 跳过增量。折入值用 `net_lazy`，原点是 `SfReadOrigin::Folded`。
4. **已完成** — `fold_mismatch` 用最终版本重算。终局性仍等折进的增量。单测四例已过。
5. **已完成** — 时间线、K=10、等价测试、Stage 1d 文档。PR #70。

## 结果

- admission 的 4220 ms 是重叠驻留之和。Stage 1c 二进制重跑后 `(code_hash, selector)` C=4 的 coverage 是 0.084 ms。未再给类键加一套增量分型。
- `0xabd6bb3978815b97` 是 77 笔有代码调用，全是 RMW。997 笔空代码转账在 `0x7ec8be01af547316`（996 增量 + 1 RMW）。
- 只分型时热位置跨度 5.144 ms。后继 RMW 推到发布者队底之后，`(to, selector)` C=4 跨度 1.522 ms / 执行 0.691 ms = 2.20 倍，76 跳的阻塞成员都是 0，跳间隙合计 0.830 ms。
- K=10（本机 model 207，4 vCPU，L3 320 MiB）：15274915 `(to)` C=4 SF 5.744 ms、OCC 3.317 ms，SF/OCC 1.73；SF C=4 / SF C=1 = 0.98。`(code_hash)` C=4 SF/OCC 1.94，SF C=4 / SF C=1 = 1.08。FullReplay 15274915 最大 12；3356896 `(code_hash)` 有一轮 10 和一轮 9，`delta_mismatch` 为 0。扫描内每轮 delta 计数为 0。
- 等价：`sload_static_gas_matches_chain_header` 0.58s，`sf_matches_onchain_focus_blocks` 与 `sf_seq_par_repeat` 3.67s。

## 未决

- 15274915 的 SF C=4 墙钟仍高于 OCC C=4。跨度降了约 3 ms，无计时墙钟只从 6.049 ms 到 5.744 ms。
- `(code_hash, selector)` 的 C=4 仍慢于 C=1。该键一次时间线上的跨度比是 2.92。
- 3356896 的固定开销不在本阶段消掉。两轮 `(code_hash)` FullReplay 超过 5，且不是增量预测失败。
- 草稿保留到用户验收。
