# 3356896：系统性 reexec → SpecFence CC — 整包落地

**基线:** PR #31 `cursor/specfence-iter11-occ-shell-59d5` @ `3484920`  
**分支:** `cursor/specfence-reexec-sf-cc-9c84`  
**设计:** `uploads/specfence-3356896-reexec-belongs-in-sf-cc-v1.md`  
**Soft=0 · 一条脊 · 无 P0/P1/P2 分期 · 不合并**

`CostPolicy::select_arm(ℓ)` 仍是唯一决策口。退役「leftover 后长脊永远 Opt/Defer-only」。

## 落地

| ID | 内容 |
|----|------|
| **R1** | 系统 reexec（同 ℓ 多 incarnation / 高 `reexec_ns` / unfenced 列车）→ `last_sys_reexec`。下一 begin **开放** covering OrderedWindow/Seg。无信号时仍挡住未测 Win prior。 |
| **R2** | ĉ 用**整脊墙**（hops×stall + leftover×abort）。`leftover≥2` 有序臂不进合格集。covering = `w = n_pairs−1`（禁 FullChain）。 |
| **R3** | 热：证实 covering 则 sticky。冷：只探 covering `w±1`。 |
| **R4** | 保留 PR31：Done-on-success、iter11、未闸 OCC pick、禁 mid-plant / 长脊 Full。hops=0 仍不 `queue_idle`。 |
| **R5** | `sys_reexec_n` / `covering_n`；`selected_arms` 显示 Opt/Defer → Win(w)。 |

半截 Win + 尾 OCC 仍是双付（O1 正确半）。不正确的产品读法是 leftover 后钉死 Opt。

## 验收证据（代码）

- `sys_reexec_reopens_covering_ordered_arm`
- `sys_reexec_full_spine_wall_rejects_half_window`
- `stable_d1_sys_reexec_reopens_covering_ordered`
- `double_pay_reopens_covering_not_half_window`
- `hot_covering_ordered_is_sticky`
- `leftover_long_measured_opt_does_not_explore_window`（无信号仍挡未测 Win）
- 既有 iter11 / Done-stamp / leftover hops=0 / Full 禁长脊

## Compare

```
SPECFENCE_COMPARE_ITERS=7 cargo run -p pevm --release \
  --config 'profile.release.lto=false' --example specfence_3356896_compare
```
