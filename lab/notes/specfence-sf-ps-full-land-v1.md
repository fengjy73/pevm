# SpecFence Parallel Spine (SF-PS) — 整包落地说明 v1

**日期:** 2026-09-21  
**基线:** PR #43 `cursor/specfence-shell-cut-redig-6a8f` @ `2add58d`（或该分支最新 tip）  
**设计 SoT:** `lab/notes/specfence-first-class-architecture-redesign-v1.md`  
**词汇:** `lab/notes/specfence-cc-glossary.md` + 本文新增 RunnableSet / VisibilityPolicy / ResolvePlan  
**硬约束:** Soft=0 · seq≡par · 无 P0/P1/P2 口惠分期 · 标准 CC 词 · lazy-update **永不** OrderedAdmit 对象 · OCC 模式保留为**对照引擎**（真 Block-STM）  
**目标:** SpecFence 模式的并行脊为 SpecFence 协议服务，不再是「Block-STM/OCC + 挂件」。

---

## 0. 问题陈述（交给实现者）

Block-STM **就是** OCC。当前 `ConcurrencyMode::SpecFence` 仍以 Block-STM scheduler + 乐观 MvMemory validate/abort 为主循环，SpecFence 只是 begin/pick/validate/end 上的钩子。这只会得到 OCC 优化版，结构性赢不了 OCC。

用户已确认：**采纳 SF-PS，直接整包改脊落地。**

成功标准见 §5。允许实现者在核实后调整文件落点，但**不得**保留「SF 主路径调用 `next_occ_task` 当主 pick」或「ungated ≡ 退回 OCC 引擎」作为架构。

---

## 1. 整包范围（一次语义切换）

### A. 调度根替换（必须）

- SpecFence 模式主循环：`Detect → RunnableSet → Schedule.pick → Execute(vis) → Validate.to_resolve → Resolve.apply → Learn`
- **删除/禁用** SpecFence 主路径上「空 wait-set → `next_occ_task`」作为默认 pick
- OCC 模式：保持现有 Block-STM `scheduler` / `next_occ_task` 不动，仅对照
- `refuse_admit` = 从 RunnableSet 换下一个可跑 tx（wave-fill 独立集），不是 OCC ready 袋上的暂留过滤器
- `ProducerStage` / 边释放：producer 提交或证书满足后，后继进入 RunnableSet

### B. VisibilityPolicy（必须）

- 读路径按边策略：`Opt` | `WaitReleased` | `OrderedTip`（名称可微调，语义保留）
- 有冲突边的访问：**不得**默认「全民 OCC MV walk，事后 validate 发现」
- 无边/独立集：可用 Opt 可见性（DAG 独立集算法）——实现可复用现有乐观读代码，但注释与控制流必须标明这是 **SpecFence Avoid=noop**，不是 `ConcurrencyMode::OCC`

### C. ResolvePlan 主路径（必须）

- validate 产出结构化 `ResolvePlan`：`Commit` | `PartialAbortRebind` | `PartialAbortRewind` | `OrderedReplay` | `FullReplay`
- SpecFence 有边路径：Resolve 优先于「bool fail → incarnation++」
- 系统反复 FullReplay：回流 Detect/arm（加深 cover / 改策略），禁止叙事成「这部分属于 OCC」
- 保留 Soft=0；WaitFor 仅在 resume 可兑现时 park

### D. Learn → 图（必须）

- arm / cover_window / sticky 只改冲突图 G 与 Runnable 释放策略
- **删除作为架构杠杆：** `skip_ungated_tx_path_tax` / 同类「假装 OCC 等价」总开关对性能叙事的依赖（代码若暂留兼容，不得再作为主优化路径或学习目标）
- 薄块（n≤176 量级）：禁学过宽 Win（如 Win_8）；under-covered 脊禁 Full 泄漏当成功
- lazy-update / 近独立：候选仅 Opt + Defer；永不 Full/Win 有序对象

### E. 清债与文档（必须）

- `crates/pevm/src/specfence/mod.rs` 与关键注释：改写「同一 Block-STM 脊 + 挂件」叙事为 SF-PS
- 新增/更新 lab note：本文件 land 结果 + 简短 architecture 指针
- 指标：RunnableSet 宽度、按 VisibilityPolicy 的执行计数、ResolvePlan 直方图；弱化「ungated_occ 当胜利」

### 明确不在本包

- 不重写 EVM 语义
- 不改 OCC 模式正确性路径（对照基线）
- 不引入 SoftWait
- 不新开第二套完整执行引擎进程；工程仍在 pevm 内，**协议根**切换

---

## 2. 假设（非绑定，实现者核实）

- 基线 PR43 已有 ReadyEdge / admit / resolve / learner；升格为主循环输入，而非推倒重来所有 Detect 启发式
- 最大风险：把 RunnableSet 做成「ReadyEdge 过滤器包一层 OCC scheduler」——**验收时用调用图否决**
- seq≡par 与 erc20 / iter11 类回归必须绿

---

## 3. 建议文件面（实现者可调）

- 新建（示例名）：`specfence/runnable_set.rs`、`specfence/visibility.rs`、`specfence/resolve_plan.rs`、`specfence/schedule.rs`（或合并进现有 computer/admit）
- 重写：`specfence/computer.rs`（pick）、`pevm.rs` SpecFence worker 环、有边 validate→resolve 接线
- MvMemory：按需扩展读 API 以支持 OrderedTip / WaitReleased；若影响 OCC，用模式门隔离
- 删除或降级：主路径 `next_occ_task` 分支、path-tax 架构依赖

---

## 4. 测试与评测

1. `cargo test` 相关 pevm / specfence 回归（含 seq≡par、iter11、erc20 若存在）
2. Soft=0 · N=3 reuse @8 · 99/all blocks TPS vs OCC（与 PR43 同 harness）
3. Instant-off 至少覆盖：3356896、19807137、14396881、6196166
4. 证明 SpecFence 主 pick **不**经 `next_occ_task`（测试或静态/日志断言）
5. Soft=0 保持；lazy 块无 Full 千写者闸

---

## 5. 验收（PRIMARY）

| # | 条 | 目标 |
|---|----|------|
| P1 | 架构 | SF 模式调用图：主调度为 RunnableSet/SF schedule；OCC 仅 `ConcurrencyMode::OCC` |
| P2 | Soft=0 / seq≡par / 关键回归 | 绿 |
| P3 | lazy | 无 4–27× tail；lazy 非 OrderedAdmit 对象 |
| P4 | TPS | 相对 PR43：赢率与中位 **不得双双明显变差**；冲突代表块（19807137 / 3356896 类）应显示 Resolve/有序语义，而非纯壳差 |
| P5 | 文档 | land note + 设计指针写入 lab/notes（PR 内或同步说明） |

若 P4 短期因改脊抖动，**诚实报告**数字 + 调用图证据；不得为刷 TPS 退回 OCC 主循环。

---

## 6. PR

- 单 PR，标题建议：`SpecFence Parallel Spine (SF-PS): schedule/MV/resolve serve SpecFence, not Block-STM hooks`
- draft OK；正文链到本 note 与 `specfence-first-class-architecture-redesign-v1.md`
