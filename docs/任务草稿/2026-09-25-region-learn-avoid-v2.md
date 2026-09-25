# Soft0-RegionLearnAvoid v2

**目标：** 在 `bb2361f`（PR #57 尖）上新分支落地非脊 Region Avoid v2，同机 ABAB 刀开/关刀测量，写结果文档并开新 PR。不叠 PR #58。

**约束：** `SPECFENCE_REGION_LEARN_AVOID_V2` 默认开；`=0` 行为与 bb2361f 相同。非脊、边级、无 Estimate 门、无 park-all。脊交给 Handoff（spine≤1）。非触及 tx 访问路径不查 region 表。

**完成标准：** seq≡par；大块 N=5 / 薄块 N=3 逐轮表；门 PASS 或带形状的 FAIL；`ge_1_5` 仅在实测 SF/OCC≥1.5 时为 true。

## 步骤

1. **已完成 — 读设计与基线。** v2 纸是 SoT。v1 在大块只武装已有脊 Handoff 的 crit ℓ，FullReplay 在非脊 hot ℓ。
2. **已完成 — 实现。** 新 `region_avoid`：radar、E1/E2/E3、每 tx 标志、WaitOnce / OrderedTip / Retain / pass、最后写者发布或跳过后 Drained、逐 region 计数。首轮刀开诊断：武装了非脊 Full ℓ，但 `drained=0` 且 `full_armed` 4–9。原因：预测写者完成但未写该槽被当成数据地板。已改为 `pick_pred`（无数据的完成者继续向下找）+ `on_skip`（唤醒但不计入发布）。
3. **进行中 — 单测与编译。** 补 pick_pred / skip 计数断言后编译。
4. **待做 — Soft=0 测量。** 76ed8dc 刀开日志只作诊断，不进门。新二进制重测大块 N=5、薄块 N=3，ABAB。
5. **待做 — 文档与 PR。** `docs/specfence-soft0-region-learn-avoid-v2.md`，门判决与四类记分。
