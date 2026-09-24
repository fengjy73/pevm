# IdleStealWake-core

**日期：** 2026-09-24（北京时间）
**起点：** `5af6822`（Soft=0 长链基线，代码 `663211d`）。不在 HPC 尖 `16bce1f` 上叠提交。

## 目标

在 pevm 落地 SpecFence 并行机最小闭环 IdleStealWake-core，并对焦点块 3356896、15274915 做 Soft=0 Instant-off 对照。基线只引用已测数字：薄 0.690（墙 1.459/1.006，span ~0.42，tax ~+1.0），大 0.707（墙 7.542/5.332，span ~1.44，tax ~+6.1）。

## 约束

- 只做三件事：refuse 后立刻 AdmitSteal；LocalAdmitDeque（所有者 LIFO）+ 跨核 FIFO AdmitSteal（仅 AdmitIndep）；ExactWakeToken + SpineHandoffSlot（`spine_cores≤1`）。
- HelpRelease 仅在 AdmitIndep 空且前驱已发布时，保持薄。
- 不引入 T0–T6 霰弹、私有 spine_q、park-all、整链开工、Estimate 门控、第四冲突原语。
- 三原语 AccessEvent 脊保持不动。`seq≡par`、est=0、soft=0、occ_picks=0。
- 愿望线 ≥1.5，未达如实报。不发明墙时。

## 完成标准

1. 新分支 + 新 PR（优先基分支 `cursor/specfence-sf-ps-full-land-09b0`）。
2. 两焦点块 Soft=0 表，对照 0.690 / 0.707。
3. 短分析：R1/R3 是否移动、`spine_cores≤1` 与 `seq≡par`、tax_ms、若退步则机制。

## 步骤

1. **已完成** — 确认 HEAD=`5af6822`，读调度器 / runnable_set / handoff。现状是全局单 deque，handoff 被塞进 Indep 头。
2. **已完成** — 本地 LIFO、跨核 FIFO steal、gated 立即 steal；handoff 独立槽且 `try_acquire` 同时只许一个所有者；空闲 `park` + 精确 `unpark` 一个。单测 `runnable_set` / `schedule` / `worker` / `spine_owner_stays_one` 通过。
3. **进行中** — Soft=0：release、LTO off、N=5、请求 8 核（宿主 4）。两焦点块。`SPECFENCE_COMPARE_CHECK` 验 seq≡par。
4. **待做** — 对照基线写分析，更新 PR。

## 实现取舍（相对纸面 PickLaw）

- 有空槽且 `spine_owner` 空时，**先** claim 槽再吃本地 Indep。若等本地 deque 排空再推脊，宽反链会把 L=17/77 的链推迟，span 被故意拉长。其余核 claim 失败后立刻吃本地 / steal，Fill 不变式仍在：有 Indep 时空核不争同一 hop。
- ExactWake 用 `thread::park` 的许可（unpark 一次只打一个 parked 核）。拆队 `wake_all` 只在 worker 退出时做，避免 join 挂死。
