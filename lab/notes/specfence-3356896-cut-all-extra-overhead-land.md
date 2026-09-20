# 3356896：压掉所有额外开销 — 整包落地

**基线:** PR #29 `cursor/specfence-explore-tax-generalize-2151` @ `89cdf5e`  
**分支:** `cursor/specfence-cut-all-overhead-26b8`  
**设计:** `uploads/specfence-3356896-cut-all-extra-overhead-v1.md`  
**Soft=0 · 一条脊 · 无 P0/P1/P2 分期 · 不合并**

`CostPolicy::select_arm(ℓ)` 仍是唯一决策口。冷探热用；`w_cap` / `Seg(seg_len)` 在线；禁 mid-plant / 全脊 Full / Instant idle→ĉ。

## 落地

| ID | 结果 |
|----|------|
| **O1 双付尾** | Win_w + leftover OCC 记 `last_double_pay`，**不**当升窗危机。ĉ 可整脊 Defer/Opt（只付一次 OCC）。下一量子 flush **1 hop** 续尾（非全脊 Full）。`flush_pending_idle_edges` 接进 `next_sf_task`。 |
| **O2 预付前缀** | 冷 hint 只种第一条 CallWaw（14→16→17）；15→19→20 等实测 abort。贵窗 EV 可 Defer。S1 未闸 OCC pick 保留。 |
| **O3 commute 壳** | 成功路径仍 ≡ `validate_occ_stage`。commute accept 去掉 `ignore_conflict` DashMap。 |
| **O4 end_block** | `edge_4_31` + thin 且 D1 已存则跳 persist / inter-prior / sketch。 |
| **O5 per-tx meta** | 无闸时跳 `note_started` / `done_stamp`（≡ OCC）。有闸才盖章。 |
| **O6 调度旁路** | 只在 sleeper 的 pred **Executing** 时 spin（64→16）；未闸仍 OCC pick。 |
| **O7 学习税** | 已证实 w≥2 的 leftover OCC **不**升窗；ĉ 只用墙后果（refuse/reexec）。 |
| **O8 常量** | 退役 `META_FLOOR_NS` / `SERIAL_NS_PER_TX`。thin = `n ≤ THIN_N_MAX`。stall 用测到的 ĉ_ord。 |

## 验收证据（代码）

- `leftover_occ_on_proven_window_is_not_crisis` — leftover 尾 ≠ 升窗危机；`last_double_pay`。
- `double_pay_keeps_opt_defer_not_wider_window` — 双付热集保留 Opt/Defer，不生成 Win_3/Seg。
- `leftover_continuation_is_one_hop` — 续 hop ≤1。
- `thin_begin_each_call_waw_picks_independently` — 冷只种第一条 trio。
- `thin_is_n_cap_not_meta_floor` — n=220/256 thin；257 full。
- Instant idle 不进 ĉ：既有 `successful_detect_prepaid_does_not_decay`。

Compare:

```
SPECFENCE_COMPARE_ITERS=7 cargo run -p pevm --release \
  --config 'profile.release.lto=false' --example specfence_3356896_compare
```
