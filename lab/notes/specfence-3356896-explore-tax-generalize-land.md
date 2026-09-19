# 3356896：压探索税 × 自适应范围可泛化 — 整包落地

**基线:** PR #28 `cursor/specfence-true-adaptive-learn-c23d` @ `7139350`  
**分支:** `cursor/specfence-explore-tax-generalize-2151`  
**设计:** `uploads/specfence-3356896-explore-tax-generalize-v1.md`  
**Soft=0 · 一条脊 · 无 P0/P1/P2 分期 · 不合并**

## 决策口（仍唯一）

`CostPolicy::select_arm(ℓ)` 是 begin 种边 / `hops_to_plant` / `select_hint_arm` 的**唯一**入口。  
块内 `block_arm` 缓存一次选择。Bayes / morph **只作特征**（G4 prior 共享），不是第二张嘴。

臂（生成，不是死表）:

`Opt | DeferPlant | OrderedWindow(w*) | Seg(s*) | Full(短链)`

- **G1** `w ∈ [1, w_cap(ℓ)]`，`w_cap = f(n_pairs, cores, block_n)` 在线。有效候选只扩到 `w±1`（冷）。
- **G2** `Seg(seg_len)` 在线；`seg_cap` 由块宽/核数导出。
- **G3** 候选由 posterior 的 `w*` / `s*` 生成，不是 `ARM_N=7`。
- **E1** 冷：σ(ĉ)-UCB。热（样本够 + 置信窄 + 上轮臂 OK）：greedy min-ĉ，探索→0。
- **E2** 热 ℓ 块级探索预算；176/8 超订 → `B=0`。
- **E3** 高预付（Seg / Full / 宽窗）只在冷或置信危机。热路径保留已证实的 `w*` / 上轮臂。
- **E4** Instant idle 仍不进 ĉ。
- **G5** 探索强度 = `σ(ĉ)≈ĉ/√n`，退役 `UCB_SCALE_NS` 策略钉。

## 退役的范围常量（不再当策略钉）

| 已删 | 原作用 | 现在 |
|------|--------|------|
| `WINDOWED_W_MAX=3` | 窗宽死上限 / 唯一 {Win_1,2,3} | `w_cap(ℓ)` 在线；`WINDOW_SAFETY_HAT=32` 只防 runaway |
| `WINDOWED_K=1` | Win_1 hops | `OrderedWindow { w }` |
| `SEG_TX=4` | 段长钉死 | `Seg(seg_len)` + `default_seg_len(n_pairs, cores)` |
| `SEG_CAP=2` | 段数钉死 | `seg_cap()` ← cores / block_n；`SEG_CAP_SAFETY_HAT=4` |
| `ARM_N=7` | 固定离散臂表 | 稀疏 `ArmStat` + 生成器 |
| `UCB_SCALE_NS=5000` | 固定探索强度 | `explore_score` ← σ(ĉ)/samples |

**仍是安全界（非策略阶梯）:** Soft=0；禁 mid-execute ReadyEdge；禁全脊 Full（`ORDER_WINDOW_K`）；禁空 to / 宽星；`THIN_ORDERED_K` 种点上限；S1 未闸 OCC pick；Done-on-success。

## 验收证据（代码）

- `w_cap_varies_with_n_pairs_and_cores` — 短/长脊、176@8 vs 32@16 的 `w_cap` 不同，不是永远 3。
- `seg_len_and_candidates_change_with_spine` — `seg_len` / 窗候选随脊长变。
- `generate_arms_not_fixed_seven_slot_table` — 冷生成邻域；热不生成 Seg/宽窗。
- `hot_phase_is_greedy_zero_explore` — 热 greedy Win_2，`explore=0`；超订预算 0。
- `morph_prior_seeds_new_location` — 新 ℓ 继承同形态 `w*` / ĉ。
- `explore_budget_zero_when_oversubscribed` — E2 在线。
- Instant idle 不进 ĉ：既有 `successful_detect_prepaid_does_not_decay`。

## 3356896 墙（对照）

| | OCC med | SF reuse | learn | explore_n (reuse) | PRIMARY |
|---|---|---|---|---|---|
| PR27 最佳 | 0.838 | **1.079** | 硬 Win_2 | — | false |
| PR28 run1/2 | 0.97/0.90 | **1.218 / 1.214** | Opt→Win_1→2→3(→Seg) | 热 UCB 每块 | false |
| 本 PR run1 (pre-crisis-fix) | 0.906 | **1.185** | Opt→Win_1→Win_2→Win_3→Seg_7 | 0 then 2 | false |

run1 已优于 PR28、≪PR22 1.40；仍被 leftover-hop 误当危机推上 Win_3/Seg。随后把危机改成 leftover-*abort*，并禁止未证实宽窗的廉价 prior。

Compare:

```
SPECFENCE_COMPARE_ITERS=7 cargo run -p pevm --release \
  --config 'profile.release.lto=false' --example specfence_3356896_compare
```
