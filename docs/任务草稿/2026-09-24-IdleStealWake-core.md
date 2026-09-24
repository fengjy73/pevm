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
2. **已完成** — 本地 LIFO、跨核 FIFO steal、gated 立即 steal；handoff 独立槽且 `try_acquire` 同时只许一个所有者；空闲 `park` + 精确 `unpark` 一个。
3. **已完成** — 两处正确性修正：槽被挡时归还并离开 pick（否则 defer 上千）；只在 live producer 上 park。AdmitIndep 整块种在 worker 0，避免轮转把每个核都放进前缀（2 核上 15274915 必现 seq≠par）。
4. **已完成** — Soft=0 表见下。愿望线 ≥1.5 **未达到**。草稿保留到用户验收。

## Soft=0 实测（tip `21802b1`）

协议：release、LTO off、N=5、请求 8 核、宿主 4 核、est=0、soft=0、occ_picks=0。主指标是 reuse median。`tax_ms = SF_wall − span`，span 取墙时等于该中位数的那一轮。末轮计数来自 `TPS_SUMMARY`（与中位墙不是同一轮）。

| 块 | 基线 ratio / SF / OCC | 本次 ratio / SF / OCC | 中位轮 span | tax | seq | spine_cores_max |
| --- | --- | --- | --- | --- | --- | --- |
| 3356896 | 0.690 / 1.459 / 1.006 | **0.673** / 1.538 / 1.036 | 0.348 | 1.190 | par | 1 |
| 15274915 | 0.707 / 7.542 / 5.332 | **0.740** / 7.776 / 5.756 | 1.496 | 6.280 | par | 1 |

薄块 reuse span：0.254、0.348、0.344、0.655。大块：1.607、1.496、2.466、1.706。两边 handoff = L−1（16 / 76）。大块中位轮 defer=0、claim_denied=3、steal=1081；薄块中位轮 defer=1、claim_denied=0、steal=147。

## 分析

- **R3（唤醒/重试）** 从首轮的 defer 上千降到 0–15。`spine_cores≤1` 保持。税没有跟着掉。
- **R1（空核吃反链）** 仍是税的主体：span 接近基线（~0.42 / ~1.44），SF 墙不低于基线。steal 计数高，墙时没降。
- 薄块 ratio 下降，因为 SF 墙 1.459→1.538，OCC 只到 1.036。大块 ratio 上升，主要是这次 OCC 墙 5.756 慢于引用的 5.332，SF 墙 7.776 仍慢于 7.542。
- 8 核冷检在本尖 6/8 通过，在 `5af6822` 上同宿主 4/6 通过。上表这次 N=5 两边都是 seq≡par。2 核在轮转播种时 0/4，改单 deque 后 8/8。

## 实现取舍（相对纸面 PickLaw）

- 有空槽且 `spine_owner` 空时，**先** claim 槽再吃本地 Indep。若等本地 deque 排空再推脊，宽反链会把 L=17/77 的链推迟，span 被故意拉长。其余核 claim 失败后立刻吃本地 / steal，Fill 不变式仍在：有 Indep 时空核不争同一 hop。
- ExactWake 用 `thread::park` 的许可（unpark 一次只打一个 parked 核）。拆队 `wake_all` 只在 worker 退出时做，避免 join 挂死。
