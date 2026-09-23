# SpecFence 统一访问学习整包落地

**日期：** 2026-09-23（北京时间）  
**目标：** 落地 AccessEvent + VersionPointer + 三原语 + 最小 Revm/Handler 面 + Advanced Learn，并做 Soft=0 Instant-off 焦点块 TPS 对比。  
**基线：** `cursor/specfence-sf-ps-full-land-09b0` @ `129bbd0`。新分支 `cursor/specfence-access-event-spine-d66a`。

## 完成标准

- PR base = SF-PS 脊分支。
- Soft=0；`estimate_block_sf=0`；`soft_wait_arms=0`；`seq≡par`；SpecFence `occ_picks=0`。
- 块 3356896、15274915：TPS SF/OCC，N≥3。未达 1.5 如实写缺口。

## 步骤

1. **已完成 — 读 SoT 与现状。** 冲突主路径在 `VmDb`：薄块 Opt 放行 Estimate，大块 `ReadError::Blocking` → `catch_error` 拆栈。
2. **已完成 — AccessSpine。** 访问点发 AccessEvent；RAW 在宿主调用内 WaitTrueVersion（不拆栈）；WAW/WAR 在写生效时记 OrderedTip / RetainHistory；Prior 只做雷达。单测 9 过。
3. **已完成 — Handler。** `YieldWait` 先 resume 保住 frame；二次才 `catch_error`。绝对 2s 超时是死锁阀，不是主路径。
4. **进行中 — 编译对比样例并跑 Soft=0 焦点块 TPS。** 样例打印 `TPS_SUMMARY`（SF_TPS、OCC_TPS、ratio、est_block、occ_picks、raw/waw/war）。
