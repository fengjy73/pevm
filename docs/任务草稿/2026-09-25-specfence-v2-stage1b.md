# SpecFence v2 stage 1b

## 目标

修掉 Stage 1 的两道门：已武装位置上的读者仍提交过期读（FullReplay 双峰），以及 C=1 相对上游 OCC 的多余开销。PR 基线是 `cursor/specfence-v2-stage1-0989`。

## 约束

- 上游 `vm.rs`、`mv_memory.rs`、`scheduler.rs`、`pevm.rs` 字节级不变。
- 不替换 opcode，保留 `static_gas()`。
- SEQ = OCC = SF = 链上头，块 15274915 与 3356896。
- 盲写和 lazy 写可见于读者，但不成为 admission 边。
- 新鲜运行、无预热。两种类键都报：15274915 FullReplay ≤ 12，3356896 ≤ 5，C=4 与 C=8，约 10 次无坏模式。
- C=1 时 SF ≤ 1.10× 上游 OCC（两块，两种类键）。
- 计时跑关掉计时器。

## 完成标准

- 用读时链快照说明每次 abort：读者、位置、作废写者、读时是否在链上、为何没等。
- 读者不会提交一次读，如果该位置已武装且链上最近较低写者还没完成最终写。
- 链插入提前到：首次写发布或 step、abort 证据、上一 incarnation 写集、同类预测。
- 开销按桶归因并去掉可避免部分。文档 Stage 1b 含根因、归因表、改动、前后对比、未决项。

## 步骤

1. **已完成** — 读时记录协调决定。根因有两条：第一名写者停在 `seen`、读时链上没有最近较低写者（15274915 上 72/78 落在 `0xabd6bb3978815b97`，`no_lower_writer=61`）；`Published` 不是已提交的最终写（3356896 上读者 66/67/69/70 接受了交易 31 随后被重写的值）。`full_replay_after_arm` 在 `arm_failure` 之后采样。
2. **已完成** — 已武装读等到最近较低写者提交；预置重复收款人；第一名发布者立刻占槽；同类预测只对 read-then-write；合约类头屏障；abort 回填并钉住。10 次新鲜运行 FullReplay 最大 7 与 1，无坏模式。C=1 内联 worker 之后的 3 次抽查仍在同一区间。
3. **已完成** — `SPECFENCE_BUCKETS` 分桶（`perf` 无对应 linux-tools）。C=1 去掉链、类哈希、预置、估计模板、原点值克隆、逐笔时钟和线程 spawn。同一扫描 SF/OCC：1.05、0.83、1.07、0.79。
4. **已完成** — `docs/specfence-v2-stage1.md` 已写 Stage 1b。最终二进制上 `sf_matches_onchain_focus_blocks`（3.89s）、`sf_seq_par_repeat`（1.17s）和 `sload_static_gas_matches_chain_header`（0.66s）通过。提交 `556830b`，草稿 PR：https://github.com/fengjy73/pevm/pull/68 （基线 `cursor/specfence-v2-stage1-0989`）。

## 未决

- C>1 的墙钟仍高于 OCC。跨类 RAW 的少量 FullReplay 在上限以内。`code_hash` 建类要查存储。`TPS_ideal` 仍来自 profile，不是 opcode 步迹。
