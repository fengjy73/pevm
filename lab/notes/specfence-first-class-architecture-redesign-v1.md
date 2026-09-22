# SpecFence 一等架构重设计 v1 — 不为 Block-STM/OCC 服务，而为 SpecFence 服务

**日期:** 2026-09-21  
**地位:** 设计 SoT 候选（**取代**「同一条 Block-STM 脊 + SpecFence 挂件」叙事）  
**触发:** 用户指出最大问题 — Block-STM **本身就是 OCC**；SpecFence 要比 OCC 好，必须大改并行脊，否则永远只是 OCC 优化版  
**约束保持:** Soft=0 · seq≡par · PC/CC/Learn 是透镜不是仓 · 标准 CC 词汇 · 禁 P0/P1/P2 分期口惠  
**非目标:** 本篇不落代码；确认后再整包改脊。

---

## 0. 认错：之前那句话为什么错

文档里曾写：

> SpecFence 和 OCC 共用同一条 pevm 并行脊（Block-STM 调度 + MvMemory）。  
> `ConcurrencyMode::OCC` = 零 SpecFence 计数的纯 Block-STM。  
> `ConcurrencyMode::SpecFence` = 同一脊上挂 Detect→Avoid→Resolve。

**错在把「实现宿主」当成了「并发语义」。**

| 事实 | 推论 |
|------|------|
| Block-STM = 乐观执行 + 验证失败则 abort/reincarnate | **Block-STM ≡ OCC 协议** |
| SpecFence = Detect / Avoid / Resolve + 学习闭环（已定义） | **另一套并发协议** |
| 在 OCC 脊上「挂」Avoid/Resolve | 语义仍由 OCC 主导；SpecFence 变成旁路优化 |
| 压壳、`skip_ungated_*`、Win 微调 | 在 OCC 天花板下抠常数，**结构性赢不了 OCC** |

因此：不是「SpecFence 要学着像 OCC 一样便宜」，而是 **并行执行架构要改造成 SpecFence 协议的载体**。OCC 模式可以保留为对照基线（真·Block-STM），但 SpecFence 模式不应再假装自己是「带插件的 Block-STM」。

v9/v10「one pevm spine」里正确的一半是：**禁止双计算机、禁止 pc/cc 分仓**。  
错误的一半是：把 spine **等同于** 未改语义的 Block-STM 调度+OCC validate 环，再把 SpecFence 焊上去。

---

## 1. 两套协议，不是一个模式开关

```text
┌─────────────────────────────┐     ┌──────────────────────────────────────┐
│  OCC / Block-STM（基线）     │     │  SpecFence（产品协议）                 │
│  乐观执行 → validate → abort │     │  Detect → Avoid → Resolve → Learn     │
│  incarnation 是一等公民      │     │  依赖边 / 有序窗 / 部分修复是一等公民   │
│  ready = 未完成 tx 的 OCC 袋 │     │  ready = 依赖满足 ∪ 独立可跑集合       │
│  MV = 乐观版本可见性         │     │  MV = 为 Avoid/Resolve 服务的版本平面 │
└─────────────────────────────┘     └──────────────────────────────────────┘
         对照评测专用                        默认并行执行路径
```

**禁止再出现的设计语言：**

- 「ungated 路径 ≡ OCC，所以尽量走 OCC」作为架构目标  
- 「OptimisticRead 编译成 shared OCC MV walk」作为**唯一**读路径（可作为独立边的实现技巧，不能定义整脊）  
- 「Shell cut / path tax skip」作为击败 OCC 的主杠杆  
- 「同一 `next_task` + 若干 if SpecFence」作为长期形态

**允许保留的：**

- 同一进程、同一 EVM、同一存储后端（工程一体，不是协议一体）  
- 独立交易在无边上的「先跑再验」实现（那是 SpecFence 对**无冲突子图**的 Avoid=noop，不是「退回 OCC 引擎」）

---

## 2. SpecFence 协议才是架构需求（从机制反推脊）

已定义机制（摘要）：

| 阶段 | SpecFence 要什么 | Block-STM/OCC 默认给什么 | 必须改什么 |
|------|------------------|---------------------------|------------|
| **Detect** | 块前/访问时得到 ℓ 上的真依赖（RAW/WAW）、PE、形态 | 事后 validate 才「发现」冲突 | 调度**前**边表是一等输入，不是旁路 hint |
| **Avoid** | `refuse_admit` / `wait_for_dependency` / `OrderedAdmit` 按 EV 选 | 一律乐观跑，靠 abort 纠错 | **取任务与门控是主循环**，不是 ready 袋上的过滤器 |
| **Resolve** | `partial_abort`（rebind/rewind）优先于整 tx 重跑 | 几乎只有 full abort + reincarnate | 修复图与 incarnation 解耦；前缀证书是一等状态 |
| **Learn** | 按 ℓ 的墙钟后果选 arm，服务下一轮 Detect/Avoid | 无 | 学习输出直接改调度平面，不是只改「是否多挂一点壳」 |
| **PC** | 拒头时 wave-fill 独立工作；有序只收缩冲突子图 | 全局乐观宽度，冲突时集体重跑 | ready 宽度由**依赖 DAG 的反链**定义，不是由「谁还没 Done」 |

一句话：**脊的主循环应是「维护冲突图上的可运行集合」**，而不是「尽量多乐观 incarnation，验不上再扔」。

---

## 3. 目标架构：SpecFence Parallel Spine（SF-PS）

名称：**SpecFence Parallel Spine**（实现可仍在 `pevm` crate，但语义模块以 SpecFence 为根）。

### 3.1 一等对象（替换 OCC 一等对象）

| OCC / Block-STM 一等 | SpecFence 一等 | 说明 |
|---------------------|----------------|------|
| `TxStatus` + incarnation | **`Process` + `Certificate` + `EdgeKey` 状态** | incarnation 降为 Resolve 的一种实现手段，不是调度主键 |
| 全局 ready 堆 | **`RunnableSet` = AntiChain(独立) ∪ Released(依赖已满足)** | 由 Detect 边驱动 |
| validate-bool → abort | **`ResolvePlan`：rebind / rewind / ordered replay / full replay** | validate 产出计划，不是只产出 bool |
| 乐观 MV 读 | **`VisibilityPolicy(ℓ)`：Opt | WaitReleased | OrderedTip** | 读路径按边策略选，默认不再「全员乐观」 |
| 块尾无学习 | **`ArmTable(ℓ)` → 下一 begin 的边与窗** | 学习改图，不改「壳开关」 |

### 3.2 主循环（伪代码）

```text
begin_block:
  G ← Detect.seed(prior, hints, morph)     # 冲突图 / PE / 短边 / 脊窗
  R ← RunnableSet.from(G)                  # 反链 + 已释放消费者
  ArmTable.apply_sticky(G)

loop until block done:
  t ← Schedule.pick(R)                     # 不为 OCC ready 服务；为 G 服务
  vis ← VisibilityPolicy.for(t, G)
  exec ← Execute(t, vis)                   # EVM；读按 vis，不按「全局 OCC walk」
  outcome ← Validate.to_resolve(exec, G)   # 结构化冲突，不是单 bool
  match Resolve.apply(outcome, G):
    Commit     → release_successors(G,R); Learn.observe(ok)
    Rebind     → 同 incarnation 修读集；可能不重跑 EVM 头
    Rewind     → 保留证书前缀；只重跑后缀
    OrderedReplay → 在 OrderedTip 可见性下重放冲突段
    FullReplay → 整 tx 重入 R（计 Learn 惩罚）
  R ← refresh(G)
end_block:
  Learn.update_arms(G, wall_vs_counterfactual)
  persist prior for next block
```

**与现状的关键差别：** `Schedule.pick` / `VisibilityPolicy` / `Resolve.apply` 都不再调用「OCC 内核 + SpecFence if」。它们 **就是** SpecFence。

### 3.3 与「独立交易乐观执行」的关系（容易再说错的点）

- 无边交易：Avoid=noop，可并行跑，失败则 FullReplay — **这是 DAG 上独立集的自然算法**，不是「切换到 OCC 模式」。  
- 有边交易：默认 **不** 走「先瞎跑再 abort」；先 refuse / wait / ordered。  
- 基线评测：`ConcurrencyMode::OCC` 仍跑真 Block-STM，用于对比；**禁止** SpecFence 路径内部再 `next_occ_task` 当主调度。

### 3.4 MvMemory 要为什么服务

今天 MvMemory 为「多 incarnation 乐观写、验证读集」优化。  
SF-PS 需要至少：

1. **按策略的读：** OrderedTip / WaitReleased / Opt 三种可见性，而不是单一 OCC 读。  
2. **证书前缀：** 与 Resolve rewind 对齐的已提交前缀视图。  
3. **发布即释放：** producer Validated/Committed 时，边表驱动消费者进入 RunnableSet（不是仅靠 OCC 依赖估计）。  
4. **WAW 有序窗：** 窗内写按序安装；窗外不伪造成「全局锁」。

这些是 **存储/版本平面的重做**，不是在 OCC MV 外再包一层 ReadyEdge 过滤器。

### 3.5 Scheduler 要为什么服务

拆掉：

- 「先 OCC fetch，再 SpecFence refuse 打回」  
- 「ungated ≈ 跳过 SpecFence」作为性能主路径叙事  

建成：

- `RunnableSet` 优先取独立反链（PC）  
- 冲突子图按 arm（Win/Seg/Full/Opt）释放  
- refuse 的语义是 **换另一个 runnable**，不是「OCC 袋里暂留」  
- 不再用 `skip_ungated_tx_path_tax` 假装协议相同

---

## 4. 映射：现有模块哪些留下、哪些降级、哪些重写

| 现有 | SF-PS 中角色 |
|------|----------------|
| `admit` / `ready_edge` / `producer_stage` | **升格为主调度输入**（Detect→RunnableSet），不是 begin 旁路 |
| `decide` / `ordered_admit_act` / `rem` | **Avoid 核**；进入 Execute 前必经 |
| `resolve` / `repair` / `partial_abort` | **Resolve 核**；替换「validate bool → OCC abort」主路径 |
| `learner` / `bayes` / arm 表 | **改图**（下一 begin 的 G），不是改 shell flag |
| `computer::next_sf_task` | **重写为 `Schedule.pick(RunnableSet)`**；删除「空 wait-set 则 next_occ」主分支 |
| `scheduler.rs`（Block-STM） | OCC 模式专用；SF 模式不再以它的状态机为根 |
| `validate_optimistic_fast` + commute | 降为 **无边/Opt 子图** 的校验实现；有边走 ResolvePlan |
| `skip_ungated_*` / path tax | **删除作为架构杠杆**；性能靠协议少 abort、少双付 |
| 薄块 Win_8 / Full 泄漏等 | 变为 **错误的 G/arm**，在 Detect/Learn 修，不在壳上修 |

---

## 5. 为什么这才能「比 OCC 好」

OCC/Block-STM 的渐近行为：冲突密度 ↑ → abort 列车 ↑ → useful_EVM ↓。  
SpecFence 的设计意图：冲突密度 ↑ → Detect 边 ↑ → Avoid 把冲突变成 **有序或等待的局部串行**，独立集仍满核 → useful_EVM 接近 DAG 界。

若执行脊仍是「全民乐观 + 事后 abort」，则：

- Avoid 只是减少进入乐观的人数（洞）→ 容易变成 **整块预付串行**（lazy Full 惨案）或 **洞+仍 abort**（双付）  
- 所有优化都表现为「少付一点 OCC 税」→ 中位永远贴着 OCC，难稳定全面超过

SF-PS 把「局部串行冲突子图 + 并行独立集」做成 **调度不变量**，OCC 的 abort 风暴从主路径拿掉，这才是机制对机制的赢法。

---

## 6. 阶段迁移（仍禁止口头 P0/P1；这里是架构切割，一次设计、分 PR 只因工程体量）

设计确认后，**语义上一次切到 SF-PS**；工程可按文件面提交，但每条 PR 不得保留「OCC 主循环 + SF 挂件」为默认 SpecFence 路径。

| 切割面 | 内容 | 验收 |
|--------|------|------|
| **A. 调度根替换** | SpecFence 模式：`RunnableSet` 主循环；OCC 模式保留旧 scheduler | SF 路径调用图不再进入 `next_occ_task` 作为主 pick |
| **B. 可见性策略** | MvMemory 读按 `VisibilityPolicy` | 有序边不再依赖「跑完再 validate 发现 WAW」 |
| **C. Resolve 主路径** | validate→`ResolvePlan`；partial 默认优于 full | 真脊上 abort ≪ OCC，且无 Detect+abort 双付 |
| **D. Learn→图** | arm 只改 G/窗/ sticky，删除 shell-flag 学习目标 | 薄块不再学出 Win_8 当「壳开关」 |
| **E. 清债** | 删除 ungated path-tax 架构；OCC 仅对照 | 文档与指标与 SF-PS 一致 |

**评测不变：** Soft=0 · 99 块 TPS vs OCC · Instant-off · seq≡par。  
**北极星改变：** 不再追求「SF 路径像 OCC 一样便宜」；追求 **冲突块上 SF_wall < OCC_wall 且独立块不显著劣于 OCC**。

---

## 7. 对近期工作的重估（诚实）

| 工作 | 在错误叙事下的意义 | 在 SF-PS 下的去留 |
|------|--------------------|-------------------|
| PR36 lazy 不做 OrderedAdmit | 修错对象 | **保留**（Detect 对象律） |
| PR42 cover / segmented | 在 OCC 脊上补窗 | **升格进 G/arm**，不是旁路 |
| PR43 shell cut | 压 OCC 等价壳 | **降级**；不作为下一主杠杆 |
| 薄 `train_hat` / Full 泄漏 | 学坏了壳参数 | 改为 **G 的错误形态**，在 Detect/Learn 修 |
| 「OptimisticRead ≡ OCC-cost」 | 当整脊目标 | 收窄为 **无边子图实现细节** |

---

## 8. 文档与旧 SoT

- `specfence-complete-architecture-v10-raw-mixed.md`：形态与 Avoid 动词仍有用；**「焊在 Block-STM 上」的 spine 定义作废**。  
- `specfence-stages-flow-vs-occ-v1.md`：阶段描述可保留为 OCC 对照；**开篇「共用 Block-STM 脊」改正为本文 §0–§3**。  
- Glossary：增加 `RunnableSet` / `VisibilityPolicy` / `ResolvePlan`；弱化「OptimisticRead compiles to OCC walk」的全局表述。

---

## 9. 待你确认的决策点

1. **是否采纳 SF-PS：** SpecFence 模式以 Detect 图驱动的并行脊为根，OCC/Block-STM 仅作对照引擎。  
2. **独立集是否仍允许乐观执行+校验：** 建议 **是**（DAG 独立集算法），但不得叫「退回 OCC 引擎」。  
3. **工程节奏：** 确认设计后，按 §6 A→E **整包语义切换**（可多 PR 文件面，但禁止长期双主循环）。  
4. **是否立刻开写 A（调度根替换）设计细则 + 落地：** 等你点头。

---

## 10. 结论

你抓到的最大问题成立：

> **Block-STM 就是 OCC；SpecFence 要更好，必须改脊。**  
> 现在这套 Detect/Avoid/Resolve 已经是新机制；架构必须反过来为它服务，而不是继续当 Block-STM 的优化挂件。

下一篇（确认后）：`specfence-sf-ps-scheduler-mv-resolve-spec-v1.md` — RunnableSet / VisibilityPolicy / ResolvePlan 的接口级规范与文件落点。
