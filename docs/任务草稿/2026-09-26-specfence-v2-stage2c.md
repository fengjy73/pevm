# SpecFence v2 Stage 2c

## 目标

在 Stage 2b（`cursor/specfence-v2-stage2b-2c54`，`4352911`）上降低 15274915 的跨核解释器膨胀、前驱停放和控制器中段塌缩。SpecFence 仍只在 `crates/pevm/src/specfence/`、`specfence` feature 后；上游 `vm.rs` / `mv_memory.rs` / `scheduler.rs` / `pevm.rs` 字节不变。正式说明写 `docs/specfence-v2-stage2c.md`。新草稿 PR 的基线是 Stage 2b 分支。

## 约束

- opcode `static_gas()` 不动。钩子只在 SF 路径。
- 先测后改。分配器若采用，OCC 与 SF 必须同一分配器。
- 测量：无预热，每轮新引擎（池可复用，学习状态重置），K≥10，中位数加 CI，SEQ 只测一次。
- 正确性：`specfence::`、`sf_matches_onchain_focus_blocks`、`sf_seq_par_repeat`、`sload_static_gas_matches_chain_header`。SF 在两块、每个 C 上等于 SEQ。

## 完成标准

- 实验 (a)(b)(c)(d) 有解释器与墙钟差值；采用有实测收益的项。
- 停放的读者不等待一个当前没在执行的前驱；有针对性测试。追踪里单次 park ≤ 0.5 ms。
- 控制器不再在块中段把活跃集收到 1；C=16 的 sched 自旋/锁竞争有解释和修改。
- 合约内部触及的账户在第一次 abort 后武装已观察到的写者（优先级低于 1–4）。
- 门禁在本 VM 能测的 C 上报告；ict21 的 C=16/32 留给复测。

## 步骤

1. **已完成** — 读 Stage 2b 的调度、停放、冷缓存和分桶。
2. **进行中** — 基线构建与 (a)–(d) 对照。
3. **待做** — 采用赢家；停放声明；控制器按在飞事务数封底；目录代际修复写者链。
4. **待做** — 正确性与门禁，写正式文档，开草稿 PR。

## 初步做法

- (a) 示例进程的全局分配器在 system 与 mimalloc 之间切换，OCC/SF 同进程同分配器。
- (b) 块内字节码只分析一次，`Arc` 共享；对照每 worker 本地缓存。
- (c) 分片、预填 from/to/beneficiary 的只读基态缓存；对照每 worker 缓存。
- (d) `SPECFENCE_INFLATION=1` 的 per-tx 解释器时间，按并行/快速路径比值列前 10。
- 停放：沿 Parked 链走到正在执行的根，否则本 worker 把 Ready/Sticky 根标记为 Executing 并立刻跑。
- 控制器：活跃集下限是 `ready + executing`，不再只用衰减后的就绪队列。
- 写者：目录替换时递增代际，避免线程本地 slot 缓存把刚武装的位置当成缺失；abort 时把观察到的写者本人写入链。
