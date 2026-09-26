# SpecFence v2 stage 2

## 目标

在 Stage 1d（PR #70，`7a98152`）之上，消掉 C>8 的反缩放和热 RMW 链迁移，并压低 C=1 相对 OCC 的固定开销。新 PR 的基线是 `cursor/specfence-v2-stage1d-0a77`。

## 约束

- 上游 `vm.rs`、`mv_memory.rs`、`scheduler.rs`、`pevm.rs` 字节级不变。
- SpecFence 仍在 `crates/pevm/src/specfence/`，仅 `feature = specfence`。
- 不替换 opcode，保留 `static_gas()`。钩子只在 SF 路径。
- 新鲜运行、无预热。K≥10。持久线程池可跨轮复用，学到的状态每块重置。
- SEQ 只作一次 1 核基线。C=1 保持调用线程内联，不进池。

## 完成标准

- 线程按 CPU 列表钉死，每块不 `spawn`。`--cpu-list` 与 `--workers` 进示例。
- 活跃工人数由就绪深度、空转比、链等待在线学习；多余工人在 condvar 上停，不忙等。
- 热 RMW 的下一跳留在发布者的不可窃取槽里，同线程接着跑，不唤醒停着的线程。
- 给出 15274915 上 C=1 相对 OCC 的分项，并削掉最大项。
- `delta_mismatch` 的事务、位置、原因写进 trace；若是预测错误则修掉，并用单测钉住。
- 上游 OCC 仍是基线。OCC 调度器是私有的，不复制一份进池。
- 正确性测试通过。文档写入 `docs/specfence-v2-stage2.md`。

## 步骤

1. **已完成** — 基线桶计时写在 `/tmp/sf-base-c1.err`（Stage 1d 二进制）。
2. **已完成** — 持久钉死线程池；C=1 走调用线程，不进池。
3. **已完成** — 粘性 RMW 交接、阻塞时交回链主、活跃集从 1 按就绪深度加倍。
4. **已完成** — 第一笔普通转账是锚点 RMW，后续增量等该位置写入后再跑。单测 `first_plain_touch_is_not_a_predicted_delta`。
5. **已完成** — 墙钟 C=1 不记读起源、不建读索引、不重扫。
6. **已完成** — 空 pop 期间到达的唤醒不再睡满 200 ms。本机 K=10 与 `docs/specfence-v2-stage2.md` 已写入。OCC 不进池：`Scheduler::try_execute` / `try_validate` 在冻结的上游文件里是私有的。串行 profile 在 `commit_serial` 之后把 attempt 标成 `kind=1`，Ideal_C 才能从这份 profile 算出来。

## 结果

本机 4 vCPU，K=10，无预热。15274915 `(to, selector)` 中位：C=1 SEQ 3.456、OCC 5.346、SF 4.069；C=4 OCC 3.267、SF 6.711；C=8 OCC 3.870、SF 7.672。热链 `0xabd6bb3978815b97` 在 C=4 与 C=8 都是 76/76，跨度/执行 1.33 与 1.99，C=4 最大跳间隔 19.7 µs，C=8 为 211 µs。`delta_mismatch` 全表为 0。C=1 的 SF/OCC 为 0.76。反缩放与 C=4 对 OCC 的拉伸门未过，原因写在 stage 2 文档。数字与命令以该文档为准。

## 未决

- 本机只有 4 个 vCPU。C=8 是超订，C=16/32 未测。ict21 才是权威数。
- 反缩放门和 C=4 ≤ OCC C=4 在本机未过。不要为了让比值通过而放慢 C=1。
