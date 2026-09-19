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
