# 3356896 完整设计落地：WAW-CC × 学习闭环 × PC 小优化

**基线:** PR #20 `cursor/specfence-pc-learn-arch-7416` @ `cfb83ab`  
**约束:** Soft=0；一条 pevm 脊；A0＝OCC 效果放行；标准 CC 用语。  
**设计:** `uploads/specfence-3356896-cc-learn-pc-complete-design-v1.md`

## 机制

### CC / WAW（C）
- **C1/C2:** begin 对 CallWaw 信封种**连续短边链**（14→16→17、31→66→…；宽 fan 也链，不当 RAW 星、不当探针星）。空 to 仍 A0。有效非 lazy 发表后 D1 + `if_idle` 立即后继（4→31）。冷启动不再整块无序锁死。
- **C3:** 升边当 `ĉ_reexec(ℓ) > ĉ_ordered`；本块首次冲突 / 已有 earlier writer / hint 后继≥2 视为证明。`THIN_A1_K` 帽的是 **ℓ 数**，不是边数。
- **C4:** commute/ignore 仍服务 21k；lazy / empty-to 不升边。
- **C5:** 不冻信封 A1=3。`edge_ordered_admit`＝真短边数（链上每一对）。

### 学习（L）
- **L1:** `promote_short_edge(ℓ, 0)` 用 abort hat / loc EMA 置 `measured=true` 并真正升边。
- **L2:** 首次 EffectiveWAW abort → 记 ℓ + `admit_seed_next_successor`（本块后续短边）。
- **L3:** 按 ℓ 更新 `cost_ev_keep_ordered` / `demote`；`end_block_learn` 在 abort_cf>prepaid 时抬升短边 prior。
- **L4:** `PromotedLoc.{pred,succ}` + `short_chain` 跨块保留；同 Pevm 下一块 `should_seed_thin_a1` 可种短边 A1。
- **L5:** A0 热路径仍不走重 HotSet/Bayes 更新。

### PC（P）
- **P1:** `validate_a0_fast` — 无冲突路径贴近 OCC；commute 仅 miss。
- **P2:** 去掉 A0 全块 HotSet flush；量 `end_block_ns`。
- **P3:** ready-bag 仅 gated wake；A0 不采样 176 宽。
- **P4:** A0 ≡ OCC `next_task`；出现短边后才走 wave/refuse。

## 非目标
恢复 Soft；懒同 from 整脊 A1；Basic→Storage PE 伪造；为学而学却不降 wall。
