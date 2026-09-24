# Soft0-RegionLearnAvoid

**日期：** 2026-09-25（北京时间）
**起点：** `bb2361f`（PR #57 尖）。新分支，不叠在 #57 上。
**目标：** 小面落地 Region Learn Avoid：本块首证后，对热点访问边的剩余触及者在真 publish tip 上武装 WaitOnce / OrderedTip。然后用 Soft=0 Instant-off 在 15274915 与 3356896 上证伪墙和 SF−Ideal。

## 约束

- Soft=0 Instant-off；release；LTO off；8 workers；`taskset -c 0-3`
- `est=0` `soft=0` `occ_picks=0` `spine≤1` `seq≡par`
- 主墙 `SPECFENCE_GLOBAL_IDEAL_READY_POOL=0`（暂停 #57 G2）
- 不重落地段内 Ideal-ready 重排，不加跨核 gas LPT
- 禁止 Estimate 门、park-all、serialize-all Indep、SoftWait/Blocking 进解释器作为 Avoid
- 不发明墙，不替换锁定带 7.0–7.2 / 5.3–5.9
- Ideal `L_crit` 大块 1.19 ms；SF−Ideal 锚 5.875
- `ge_1_5=false`，除非实测 ratio ≥ 1.5
- 不改用户私有 lab 仓

## 完成标准

- 产品码：IntraPatch 在真 tip 武装热点 region 的剩余触及者
- 测试：热区武装、NeverWait 排除、无 Estimate 门
- 同机关刀对照 + 两块实测表 + PASS/FAIL
- 草稿 PR

## 步骤

1. **已完成。** 读设计纸、predictions、steal dig，并对照现有 `protect_hot` / WaitOnce / spine 链。
2. **已完成，并改过一版。** `SPECFENCE_REGION_LEARN_AVOID` 默认开。块初只记 spine 链雷达。本块第一个链成员被 pick 时对该 `ℓ` 装 WaitOnce（peer=0，不写 `wait_edges`，不调用 `protect_hot`）。访问点用 `region_pred` 等真 tip；ExactWake 看 `is_region_armed`。薄块 admission 仍直接返回。beneficiary / NeverWait 不入。
   - 第一版调用了 `protect_hot` 并放开薄块 admission。3356896 刀开复用中位 **16.213 ms**（defer=9391），是整 tx 离队，不是访问点 Avoid。已丢掉。
3. **已完成。** 三则单元测试通过：不 `protect_live`，peer 不门控 OCC-shaped，`region_pred` 指向相邻写者，NeverWait 不武装。
4. **已完成。** `89197b1`，release，LTO off，`taskset -c 0-3`。主墙两旗都关（池和 Ideal-timed）。大块刀开 8.767 / 关刀 8.252。薄块刀开 1.583 / 关刀 1.825。`seq=par ok`。只关池、留下 #56 的薄块约 16 ms，不作为本刀地板。
5. **已完成。** 判词 FAIL。笔记 `docs/specfence-soft0-region-learn-avoid.md`。`ge_1_5=false`。草稿留到用户验收。
