# Soft0-GlobalIdealReadyAdmitPool

**日期：** 2026-09-24（北京时间）
**起点：** PR #56 尖 `65e3609`。本分支 `cursor/soft0-global-ideal-ready-pool-d2ad`，PR #57。
**刀：** 跨核 Ideal-ready 先于本核晚波（G2）。单池 G3 已实测并放弃。

## 目标

改 Ideal-ready 的可见性，不是段内再排序。非脊核在自己的 Ideal-ready 空了之后，先偷别的核的 Ideal-ready，再弹本核 `local_late`。SpineHop / Ordered 仍 `mark_wait` / Handoff，不进 AdmitIndep。然后按锁定协议跑 Soft=0 Instant-off 两焦点块。

## 约束

- Soft=0：`est=0` `soft=0` `occ_picks=0`。`spine_cores_max≤1`。QuietExit 冻结。
- Detect 保留。WaitOnce 只在真 publish tip。无 Estimate。无 park-all。
- 禁止段内 Ideal-ready 重排当刀，禁止跨核 gas LPT。
- 下标分段保留。worker 0 弹低端，其余核弹自己段的高端。不把低前缀只留给 worker 0 而别人够不着（那一版会挂）。
- 主 KPI 是无探针墙和 SF−Ideal。探针墙不入锁定。不宣称 ≥1.5。不改锁定带 7.0–7.2 / 5.3–5.9。
- 只改 pevm。不改用户私有 lab。

## 完成标准

1. `SPECFENCE_GLOBAL_IDEAL_READY_POOL` 默认开。`=0` 回到 #56 段内 Ideal-timed（含段内重的先弹）。
2. 单元测试：SpineHop 不进池；非脊核在本段晚波非空时仍先弹远程 Ideal-ready。
3. 大块 15274915 N=5、薄块 3356896 N=3，release、LTO off、8 workers、`taskset -c 0-3`。同机刀关。Diff 一轮。`seq=par ok`。
4. 草稿 PR 写实测表和 PASS/FAIL。

## 步骤

1. **已放弃** — G3 单池。同机大块刀开复用中位 8.104，刀关 7.901，双关分段 7.866。SF−Ideal 6.914 / 6.711 / 6.676，锚 5.875。薄块 1.656 / 1.544。fill@1.19=9.4%，中位进入 2.890。`seq≡par` 有抖动。只让 worker 0 弹低前缀的栅栏会挂。不作为产品。
2. **已完成** — G2：`pop` 顺序是本段 Ideal-ready → 偷远程 Ideal-ready → 本段晚波 → 再平衡晚波。无 gas 排序。`runnable_set` 23 项通过。
3. **进行中** — 提交后跑 Soft=0 双块 + 刀关 + Diff。
4. **待做** — 按实测写笔记、经验记录和 PR 判门。假设以实测为准。
