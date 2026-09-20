# 3356896：压掉所有额外开销 — 整包落地

**PR:** https://github.com/fengjy73/pevm/pull/30（draft）  
**基线:** PR #29 `cursor/specfence-explore-tax-generalize-2151` @ `89cdf5e`  
**分支:** `cursor/specfence-cut-all-overhead-26b8` @ `90c021a`  
**设计:** `uploads/specfence-3356896-cut-all-extra-overhead-v1.md`  
**Soft=0 · 一条脊 · 无 P0/P1/P2 分期 · 不合并**

`CostPolicy::select_arm(ℓ)` 仍是唯一决策口。冷探热用；`w_cap` / `Seg(seg_len)` 在线；禁 mid-plant / 全脊 Full / Instant idle→ĉ。

## 落地

| ID | 结果 |
|----|------|
| **O1 双付尾** | Win_w leftover OCC → `last_double_pay`（≠升窗危机）。ĉ 整脊 Defer/Opt（只付一次 OCC）。hops=0 仍写 `e.decision`。短 n Full 缓存不得 FullChain 长脊。下一量子 flush **1 hop**。 |
| **O2 预付前缀** | 冷 hint 只种第一条 CallWaw（14→16→17）。贵窗 EV 可 Defer。S1 未闸 OCC pick 保留。 |
| **O3 commute 壳** | 成功路径仍 ≡ `validate_occ_stage`。commute accept 去掉 `ignore_conflict` DashMap。 |
| **O4 end_block** | `edge_4_31` + thin 且 D1 已存则跳 persist / inter-prior / sketch。 |
| **O5 per-tx meta** | 无闸时跳 `note_started` / `done_stamp`（≡ OCC）。 |
| **O6 调度旁路** | 只在 sleeper pred **Executing** 时 spin（64→16）；未闸仍 OCC pick。 |
| **O7 学习税** | 已证实 w≥2 leftover OCC **不**升窗；`last_double_pay` 清 `last_crisis`。ĉ 只用墙后果。 |
| **O8 常量** | 退役 `META_FLOOR_NS` / `SERIAL_NS_PER_TX`。thin = `n ≤ THIN_N_MAX`。 |

## 验收证据（代码）

- `leftover_occ_on_proven_window_is_not_crisis`
- `double_pay_keeps_opt_defer_not_wider_window` — 双付后候选只有 Opt/Defer
- `unused_win_prior_does_not_undercut_measured_opt_on_leftover_spine`
- `commit_arm_persists_defer_decision`
- `long_spine_never_keeps_short_n_full_cache`
- `leftover_long_measured_opt_does_not_explore_window`
- `leftover_continuation_is_one_hop`
- `thin_begin_each_call_waw_picks_independently` / `thin_is_n_cap_not_meta_floor`

## Compare 3356896 @8 Soft=0 N=7（`90c021a`）

| | OCC med | SF reuse | long loc (reuse) | unfenced | PRIMARY |
|---|---|---|---|---|---|
| PR27 best | 0.838 | **1.079** | hard Win_2 | 14 | false |
| PR29 typical | ~0.97 | **1.18–1.35** | Win_1/2 | 14 | false |
| this @ `9516f95` | 0.914 | **1.196** | Win_1 | 14–15 | false |
| **this @ `90c021a`** | **0.875** | **1.273** | **Opt/Defer** | 14–16 | **false** |

Reuse SF walls: 1.288, 1.376, 1.273, 1.228, 1.181, 1.071（med 1.273）。  
Cold SF 1.384 vs OCC 2.129。`w_cap=2`；explore 0–2；Soft=0。  
长脊不再种 Win_1 前缀（O1 产品）；unfenced≈14 是 Opt 一次 OCC，不是 Detect+OCC 双付。  
剩余墙 ≈ leftover OCC reexec 132–405µs + storage 预付 refuse + commute 77 + end_block ~66µs。  
墙 ≪ PR22 ~1.40 的**结构**（本盒 OCC 0.78–0.97；SF 不回满脊 Full）。PRIMARY 未过。

```
SPECFENCE_COMPARE_ITERS=7 cargo run -p pevm --release \
  --config 'profile.release.lto=false' --example specfence_3356896_compare
```

Lib `specfence` 298 passed；policy 42 passed；`erc20_independent` 0.45s release。  
`specfence_iter11_…seq_eq_par` 未确认（release 跑 ~12 min @~400% CPU 后杀掉，像自旋不是 DashMap 死锁）。
