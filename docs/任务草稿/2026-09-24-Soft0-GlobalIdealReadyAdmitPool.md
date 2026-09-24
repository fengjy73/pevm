# Soft0-GlobalIdealReadyAdmitPool

**日期：** 2026-09-24（北京时间）
**起点：** PR #56 尖 `65e3609`。本分支 `cursor/soft0-global-ideal-ready-pool-d2ad`，PR #57。
**刀：** 跨核 Ideal-ready 先于本核晚波。单池已放弃。
**实测二进制：** `f2e3344`。

## 目标

改 Ideal-ready 的可见性。非脊核先偷远程 Ideal-ready，再弹本核晚波。然后按锁定协议跑两焦点块。

## 约束

- Soft=0：`est=0` `soft=0` `occ_picks=0`。`spine_cores_max≤1`。
- 禁止段内重排当刀，禁止跨核 gas LPT。
- 主 KPI 是无探针墙和 SF−Ideal。不宣称 ≥1.5。不改锁定带。
- 只改 pevm。

## 完成标准

1. 旗默认开，`=0` 回到 #56。单元测试覆盖 SpineHop 与远程 Ideal-ready 先于晚波。
2. 大块 N=5、薄块 N=3，同机刀关，Diff 一轮，`seq=par ok`。
3. PR 写实测表和 PASS/FAIL。

## 步骤

1. **已放弃** — G3 单池。大块 8.104 vs 刀关 7.901。前缀只留给 worker 0 会挂。`seq≡par` 会抖。
2. **已完成** — G2 pop 律。`runnable_set` 23 项通过。提交 `f2e3344`。
3. **已完成** — 无探针两轮大块、两轮薄块、下标段对照、Diff、2 核冷检。日志在测量机 `/tmp/sf-g2/`，不入库。
4. **已完成** — 判门 **FAIL-A**，薄块第 1 轮另是 **FAIL-B** 那一档。笔记 `docs/specfence-soft0-global-ideal-ready-admit-pool.md`。用户验收前保留本草稿。
