# Soft0-GlobalIdealReadyAdmitPool

**日期：** 2026-09-24（北京时间）
**起点：** PR #56 尖 `65e3609`（`cursor/soft0-ideal-timed-admit-prox-d7fe`）。实测 Ideal-timed 二进制是 `6516f6d`。
**刀：** G3。Ideal-ready AdmitIndep 进全局池；Detect-pred 晚波仍按连续下标段进 `local_late`。

## 目标

改 Ideal-ready 的库存可见性，不是段内再排序。任一空闲非脊核先弹全局 Ideal-ready，再弹本段晚波。脊核仍先 Handoff。SpineHop / Ordered 不进池。然后按锁定协议跑 Soft=0 Instant-off 两焦点块，同机刀关对照，并开 IdealProximityDiff 作辅。

## 约束

- Soft=0：`est=0` `soft=0` `occ_picks=0`。`spine_cores_max≤1`。QuietExit 冻结。
- Detect 保留。WaitOnce 只在真 publish tip。无 Estimate。无 park-all。
- 禁止段内 Ideal-ready 重排当刀，禁止跨核 gas LPT。
- 池只收 Ideal-ready AdmitIndep。低端由 worker 0 `pop_low`，其余核 `pop_high`，避免 `tx % C` 那种人人从低前缀开工。
- 主 KPI 是无探针墙和 SF−Ideal。探针墙不入锁定。不宣称 ≥1.5。不改锁定带 7.0–7.2 / 5.3–5.9。
- 只改 pevm。不改用户私有 lab。

## 完成标准

1. `SPECFENCE_GLOBAL_IDEAL_READY_POOL` 默认开。`=0` 回到 #56 段内 Ideal-timed。
2. 单元测试：SpineHop 不进池；非脊核在本段晚波非空时仍先弹全局 Ideal-ready，且能弹到原低段的 Ideal-ready；池弹不算 steal。
3. 大块 15274915 N=5、薄块 3356896 N=3，release、LTO off、8 workers、`taskset -c 0-3`。同机刀关。Diff 一轮。`seq=par ok`。
4. 草稿 PR 写实测表和 PASS/FAIL。

## 步骤

1. **已完成** — 全局双端池 + 晚波分片 + pop 律（池先于晚波，steal 只再平衡晚波）。`runnable_set` 23 项通过，含 SpineHop 不进池、非脊核先弹全局 Ideal-ready、跨段可见且 `steal_n=0`。
2. **已完成** — 单元测试见上。默认 `SPECFENCE_GLOBAL_IDEAL_READY_POOL` 开；`=0` 回到段内 Ideal-timed。
3. **待做** — 提交后跑 Soft=0 双块 + 刀关 + Diff。
4. **待做** — 按实测写笔记和 PR 判门。假设（填满率升 / 进入时刻降）以实测为准，错了就丢掉。
