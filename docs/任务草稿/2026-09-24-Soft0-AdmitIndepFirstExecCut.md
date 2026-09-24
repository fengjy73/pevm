# Soft0-AdmitIndepFirstExecCut

**日期：** 2026-09-24（北京时间）
**起点：** `a6f85fd`（PR #52，QuietExit + join dig）
**对照：** dig 大块中位 SF **7.065** / OCC **5.306** / ratio **0.751** / span **2.089** / tax **4.976** / join-out **3.046**，链 **116/1219**。成功只认墙下降（B），不认只把 `first_start` 提前（A）。不改 QuietExit。不宣称 ≥1.5。

## 目标

Soft=0 Instant-off 下，非链 AdmitIndep 的第一次 Opt 少付冷读元数据，使 SF 墙相对 dig 下降、ratio 上升。Detect 留在链位置和 WaitOnce 位置上。AdmitShard LIFO、HandoffSlot、WaitOnce 真 tip、QuietExit 不动。

## 约束

- 无 Estimate 门 Avoid，无 park-all，无 SpineHop 进可偷 deque。
- `seq≡par`，`occ_picks=0`，`spine_cores_max≤1`，`est=0 soft=0`。
- 单独前载种子不算成功。本刀不做种子重排。

## 步骤

1. **已完成：** `a253d6f`。第一次 Opt（incarnation 0、未门控、非链成员、非 leftover_min）跳过 access ordinal、value snap、finegrain、以及非热位置上的 `spine_before_read` / WaitOnce consult。链位置、crit、protected、非 crit WaitOnce 仍走完整 Detect。MV 读到 Estimate 仍走 `live_writer_act`（不记 `estimate_block_sf`）。中止则整段重放。种子不重排。QuietExit 未改。
2. **已完成：** release、LTO off、`taskset -c 0-3`、请求 8 核、N=5。2 核冷检两块 `seq=par ok`。大块复用中位 SF **7.210** / OCC **5.900** / ratio **0.818** / span **3.312** / tax **3.898** / join-out **2.166**，链 116/1219。相对 dig 7.065 是 **(A)**：墙没有下降，span 变长、join-out 变短。薄块 0.591（1.520 / 0.898，span 0.338，tax 1.182，join-out 0.610）。`ge_1_5=false`。笔记 `docs/specfence-soft0-admitindep-firstexec-cut.md`。
3. **已完成：** 探针确认复用轮约 98% 的首次 Opt 读已跳过 Detect，跳过分支约 0.2 ms 线程时间。首次 Opt 约 90% 在 `basic`/`storage` 之外。无探针中位仍是 (A)。下一刀只设计：缩短 `finish_execution` 写发布，不砍读路径。笔记 `docs/specfence-soft0-admitindep-firstexec-cut-dig.md`。草稿保留到用户验收。
