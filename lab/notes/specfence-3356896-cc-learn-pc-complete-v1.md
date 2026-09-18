# 3356896 完整设计落地：WAW-CC × 学习闭环 × PC 小优化

**基线:** PR #20 `cursor/specfence-pc-learn-arch-7416` @ `cfb83ab`  
**约束:** Soft=0；一条 pevm 脊；A0＝OCC 效果放行；标准 CC 用语。  
**设计:** `uploads/specfence-3356896-cc-learn-pc-complete-design-v1.md`

## 机制

### CC / WAW（C）
- **C1/C2:** begin 只对 **storage-trio CallWaw（3..=4）** 种连续短边（14→16→17）。宽 calldata fan（ERC-20 / 0x209c 17 脊）begin 保持 A0——整链会把同 `to` 不同 slot 串死（p2 `occ_aborts=0`）或 hang。中块写集插边在本脊 race（iter9 / SIGSEGV），因此 4→31 走 D1 快照 + abort promote / L4 prior，不在 execute 热路径插边。
- **C3:** 升边当 `ĉ_reexec(ℓ) > ĉ_ordered`；本块首次冲突 / 已有 earlier writer / hint 后继≥2 视为证明。`THIN_A1_K` 帽的是 **ℓ 数**，不是边数。
- **C4:** commute/ignore 仍服务 21k；lazy / empty-to 不升边。
- **C5:** 不冻信封 A1=3。`edge_ordered_admit`＝真短边数。

### 学习（L）
- **L1:** `promote_short_edge(ℓ, 0)` 用 abort hat / loc EMA 置 `measured=true` 并真正升边。
- **L2:** EffectiveWAW abort 记 ℓ + pair（本块不 mid-seed；下一块 L4 可种）。
- **L3:** 按 ℓ 更新 `cost_ev_keep_ordered` / `demote`；`end_block_learn` 在 abort_cf>prepaid 时抬升短边 prior。
- **L4:** `PromotedLoc` + `short_chain` 跨块保留；同 Pevm 下一块 `should_seed_thin_a1` 可种短边 A1。
- **L5:** A0 热路径仍不走重 HotSet/Bayes 更新。thin begin 跳过 contract walk。

### PC（P）
- **P1:** `validate_a0_fast` — 无冲突路径贴近 OCC；commute 仅 miss。
- **P2:** HotSet 仅 multi-writer D1；量 `end_block_ns`（本盒 ~34–44µs）。
- **P3:** ready-bag / ready_width 仅 gated；A0 不采样 176。
- **P4:** 无 gated → OCC `next_task`。`is_gated`（consumer）≠ `was_queued`（含 producer）。

## 本盒 N=5（3356896 @8 Soft=0）

| | OCC | SpecFence |
|---|---|---|
| walls ms | 1.374, 0.825, 0.849, 0.821, 0.730 | 1.278, 1.251, 1.127, 1.126, 1.009 |
| median | **0.825** | **1.127** (~1.37×) |
| `unfenced` | — | 14, 15, 16, 15, 15 |
| `storage_inc_gt0` | — | **∅**（PR20 常含 16/17） |
| `main_inc_gt0` | — | 脊后段仍 14 左右 |
| `edge_oa` / a1 / ev_keep | — | 10 / 6 / 6 |
| commute / ignore / taxed / soft | — | 77 / 77 / 0 / 0 |
| `edge_4_31` | — | **true** |

PRIMARY `sf_le_occ` **false**。主账本仍是 0x209c 主链 unfenced reexec（`reexec_ns` 212–439µs）。storage 短边已落地。

## 非目标
恢复 Soft；懒同 from 整脊 A1；Basic→Storage PE 伪造；为学而学却不降 wall。
