# SpecFence v2 Stage 2d

## 目标

从 Stage 2c（`cursor/specfence-v2-stage2c-2a99`，`c820ed1`，PR #73）消除 ict21 上 15274915 的提交前缀空洞，并让默认 harness 的 SEQ 回到 PR 72 的分配器水平。SpecFence 仍只在 `crates/pevm/src/specfence/`、feature `specfence`。上游 `vm.rs` / `mv_memory.rs` / `scheduler.rs` / `pevm.rs` 与 `c820ed1` 字节相同。正式说明写 `docs/specfence-v2-stage2d.md`。新草稿 PR 的基线是 Stage 2c 分支。

## 约束

- opcode `static_gas()` 不动。钩子只在 SF 路径。
- 默认二进制没有运行时 `#[global_allocator]` 包装。mimalloc 只作为编译期 feature。
- 工人在有就绪或可认领任务时，停车不超过 0.5 ms。每次停车都有唤醒者；0.5 ms 超时只是漏唤醒的后盾。
- 测量：无预热，每轮新引擎（池可复用，学习状态重置），K≥10，中位数加 CI，SEQ 只在 C=1 测一次。定时区内不写时间线。
- 本机 4 个 CPU（0–3）。C=16/32 留给 ict21。

## 完成标准

- 默认同主机 SEQ 与 PR 72 二进制相差不超过 5%。
- 追踪：没有由空闲工人造成的、超过 1 ms 的提交前缀空洞；没有停在“前驱没在跑”上超过 0.5 ms 的 park。
- 15274915：SF(4) < SF(1)；SF C=4 ≤ 1.5× OCC C=4（中间），≤ OCC C=4（拉伸）。
- 正确性：全部 specfence 测试，`sf_matches_onchain_focus_blocks`、`sf_seq_par_repeat`、`sload_static_gas_matches_chain_header`。两块每个 C 上 SF 等于 SEQ（`delta_mismatch=0`）。
- 等待（武装读、估计、链、准入、锁）进等待桶，不进解释器。sched 的 11 ms 有解释和削减。

## 步骤

1. **已完成** — 分配器：去掉运行时包装。默认 system；`specfence-mimalloc` 直接安装 `MiMalloc`。SEQ ab：PR72 3.769 ms，Stage 2c 包装 4.870 ms，新默认 3.790 ms（1.006×）。
2. **已完成** — 提交前缀空洞。旧时间线 C=4：tx 11→12 空 4.722 ms，四名工人在跑更高序号，tx 12 的执行只有 7 µs。原因是 LIFO/窃取不优先提交前缀，加上 `notify_one`、`park` 不唤醒、`active` 从 1 起步。修复：每次取任务前认领提交前缀；`poke_work` 改为 `notify_all` 并把活跃集抬到 ready+executing；`seed` 一开始就把活跃集设满；`park`/`rescue` 入队后唤醒；condvar 超时 500 µs。新时间线：无 >0.5 ms 的提交空洞，最长空闲 0.368 ms 且当时 ready=0。
3. **已完成** — tx 102 停在未进解释器的 tx 101 上。相位在 pop 时就标成 Executing，`close_from` 又夹在 `depend` 和 `handler.run` 之间。现在先 `close_from`，只有 `in_interpreter` 才 Wait；属主已 idle 且未进解释器则窃取执行。单测 `estimate_claim_runs_a_predecessor_that_is_not_in_the_interpreter`。新时间线里 tx 102→101 的 213 µs 停放落在 tx 101 的执行跨度内。
4. **进行中** — 等待桶与膨胀榜、干净 K=10、集成测试、sched 桶。
5. **待做** — `docs/specfence-v2-stage2d.md`、经验记录、提交并开草稿 PR。

## 初步做法

- 提交前缀：`claim_commit_frontier` 在 sticky/pop 之前。Ready 直接执行；Parked 走 `claim_blocker`；Executing 且已在解释器里则放过；Executing、属主 idle、未进解释器则窃取。属主正在做前置检查时不窃取，避免把同一笔在工人之间弹来弹去。
- 唤醒：`work_seq` 在 `mu` 下增加，然后 `cv.notify_all()`。活跃集变大时再 `sleep_cv.notify_all()`。
- 分桶：`WaitGuard` 计入等待桶，并在解释器内时累加到 `EXCLUDE`，从解释器、类桶和 `interp_ns` 里减掉。`SPECFENCE_BUCKETS` 与 `SPECFENCE_INFLATION` 才打点。
- sched：`take_sticky` 在 `sticky_flag` 为空时不加锁；注入器长度为 0 时不加锁。C=16 的 11 ms 是每次调度都对空 sticky 槽加全局锁，再乘上窃取循环。
