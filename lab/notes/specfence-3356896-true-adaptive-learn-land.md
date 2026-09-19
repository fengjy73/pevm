# 3356896：true adaptive learn — 整包落地

**基线:** PR #27 `cursor/specfence-post25-shell-pc-0b5e` @ `3efae19`  
**分支:** `cursor/specfence-true-adaptive-learn-c23d`  
**Soft=0 · 一条脊 · 无 P0/P1/P2 分期**

## 决策口

`CostPolicy::select_arm(ℓ)` 是 begin 种边 / `hops_to_plant` / `select_hint_arm` 的**唯一**入口。块内 `block_arm` 缓存一次选择。

臂：`Opt | Win(w) | Seg | Full(n_pairs≤2) | DeferPlant`。  
选择：UCB1（ĉ − bonus），Win(w) **直接比 ĉ**。  
奖励：`−(gate_stall_wall + reexec_ns + refuse_share)`。Instant idle / `occ_aborts` / `ready_width` 不进 ĉ。  
学习率：`α = 1/(n+2)`，不是 `EMA_ALPHA=0.20`。

## 退役的策略硬编码（决策驱动）

| 已删 | 原作用 |
|------|--------|
| `LEFTOVER_WIN3=3` / `LEFTOVER_SEG=8` | leftover → Win_3 / Seg |
| `escalate_n≥1→Win_2` / `≥2→Win_3` | if-ladder |
| `MIN_SAMPLES` 钉死冷 Opt | 未测则探索，不强制 demote |
| `PREPAID_LOSE_N` 臂降级 | 改为自信区间 streak（过程 prior） |
| `EMA_ALPHA=0.20` | 每臂 `1/(n+c)` |
| S6「thin 最多一条 CallWaw hint」 | 各 ℓ 独立臂；`DeferPlant` 可赢 |
| 短链硬 `FullChain`、长链 leftover 粘滞 last Win | 短链 Full 只是冷先验；可学走 |

## 仍是安全界（非策略阶梯）

Soft=0；禁 mid-execute ReadyEdge；禁全脊 Full；禁空 to / 宽 0x209c 星；`WINDOWED_W_MAX=3` / `SEG_CAP=2` / `THIN_ORDERED_K` 种点上限；`RAW_FANOUT_FLOOR` 分类。S1 未闸 OCC pick、Done-on-success 未动。

## 双系统

`AdaptiveParams` / Bayes / morph **只作特征**。种边口不再另开 Wait/Ordered 闸。

## 本盒 3356896 @8 Soft=0 Instant-off N=7

4 物理核超订 8。`profile.release.lto=false`。

| | OCC med | SF cold | SF reuse | unfenced reuse | learn path | PRIMARY |
|---|---|---|---|---|---|---|
| PR27 land 最佳 | 0.838 | 1.172 | **1.079** | 0–3 | hard Win_2 | false |
| **本 PR run1** | 0.969 | 2.076 | **1.218** | 0 after Win_2 | Opt→Win_1→Win_2→Win_3 | **false** |
| **本 PR run2** | 0.898 | 1.719 | **1.214** | 0–2 late | Opt→Win_1→Win_2→Win_3→Seg | **false** |

Adaptivity (run2): `c_opt` 67k→270k；`c_win2` 12k→25k→89k（prepaid 墙时钟）；`arm_switch_n` 2–4；`win2_deviate_n`>0。不是永远硬 Win_2。

Soft=0；`occ_pick_while_gated` 156–202；iter11 pass；erc20_independent 0.47s；墙 ≪ PR22 ~1.40。
