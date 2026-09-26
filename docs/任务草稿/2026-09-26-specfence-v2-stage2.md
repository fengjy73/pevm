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
6. **进行中** — 正确性已过一轮；本机 K=10 与文档还没写完。OCC 不进池：`Scheduler::try_execute` / `try_validate` 在冻结的上游文件里是私有的。

## 结果

（测量后填写）

## 未决

- 本机只有 4 个 vCPU。C=16/32 只能超订，ict21 才是权威数。
