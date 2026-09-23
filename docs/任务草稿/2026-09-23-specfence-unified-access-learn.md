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
4. **已完成 — Soft=0 焦点 TPS（release，LTO off，N=5，请求 8 核，宿主 4 核）。** 主指标是 reuse median。未达 1.5，不宣称胜利。

   | 块 | SF_TPS | OCC_TPS | ratio | ≥1.5 | est | soft | occ_picks | seq≡par |
   | --- | --- | --- | --- | --- | --- | --- | --- | --- |
   | 3356896 | 116903.6 | 174278.6 | 0.671 | 否 | 0 | 0 | 0 | 是 |
   | 15274915 | 151063.5 | 210072.6 | 0.719 | 否 | 0 | 0 | 0 | 是 |

   辅证墙时：3356896 OCC 1.010 ms / SF reuse 1.506 ms；15274915 OCC 5.836 ms / SF reuse 8.116 ms。

5. **已测 — 长链这一刀未达 1.5。** 发布 handoff 保留；WAR 原点先快照再 Estimate。停车实验已撤回。

   | 块 | SF_TPS | OCC_TPS | ratio | ≥1.5 | seq≡par | yield_ok | handoff | retain | chain |
   | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
   | 3356896 | 120665.0 | 174863.9 | 0.690 | 否 | 是 | 0 | 16 | 0 | 17 |
   | 15274915 | 162557.1 | 229915.2 | 0.707 | 否 | 是 | 4 | 76 | 2 | 77 |

   墙时：薄块 SF 1.459 / OCC 1.006 ms；大块 SF 7.542 / OCC 5.332 ms。末轮薄块链跨度约 0.42 ms。`est_block=0`，`soft=0`，`occ_picks=0`。下一刀是反链调度税，不是第四原语。
