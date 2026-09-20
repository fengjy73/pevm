# 覆盖窗轻量化 × 多块验证 reexec→CC — 整包落地

**基线:** PR #32 `cursor/specfence-reexec-sf-cc-9c84` @ `fd6d128`  
**分支:** `cursor/specfence-light-cover-mb-3352`  
**设计:** `uploads/specfence-3356896-light-cover-multiblock-v1.md`  
**Soft=0 · 一条脊 · 无 P0/P1/P2 分期 · 不合并**

`CostPolicy::select_arm(ℓ)` 仍是唯一决策口。系统性 reexec → CC 原则保留。覆盖不再钉死 `w = n_pairs−1`。

## 落地

| ID | 内容 |
|----|------|
| **L1** | ĉ 选 **最小 `w_need` / Seg** 吸收系统 reexec。T3 滑 1 hop 续尾。禁止 `hops < w_need` + OCC 尾双付。 |
| **L2** | 独立集零税（S1 未闸 OCC pick）；洞只约束依赖边。 |
| **L3** | 热 sticky 已证实轻窗；冷探 `w_need±1` / Seg，不以全覆盖为默认钉。 |
| **L4** | 墙时钟 ĉ：有序轻预付 vs OCC 整脊。预付爆破可回 OCC。 |
| **M1–M3** | `specfence_all_blocks_sweep` 复用 Pevm（`SPECFENCE_ALL_REUSE=1`）；每块臂 / unfenced / double_pay / SF vs OCC。 |

半截窗 = 短于 `w_need` 的前缀，不是「leftover_hops ≥ 2」。轻窗 leftover 靠 T3 滑；仍漏列车则 `double_pay` 增长 `w_need`。

## 验收证据（代码）

- `sys_reexec_picks_minimal_w_not_full_cover`
- `double_pay_grows_w_need_not_occ_tail`
- `light_cover_ok_allows_leftover_hops`
- `hot_covering_ordered_is_sticky`（轻 Win_4）
- `prepaid_blowout_allows_occ_after_light_cover`
- 既有 sys-reexec / leftover / iter11 / Done-stamp / Full 禁长脊

## Compare 3356896 @8 Soft=0 N=7

| | OCC med | SF reuse | long ℓ | unfenced | PRIMARY |
|---|---|---|---|---|---|
| PR32 | 0.908 | 1.100 | Win_16/17 | 0–1 | false |
| **this** | **0.992** | **1.184** | **Win_2 / Seg_2** (`w_need=2`) | 0–2 on cover | **false** |

Gap still ~0.19ms (Detect prepaid vs OCC overlap) but **without** full-spine prepaid. Reuse walls 1.184, 1.520, 1.326, 1.117, **1.085**, **1.091**. Covering iters unfenced 0–2; Defer trial can bring leftover 12–16 (L4). Wall ≪ PR22 ~1.40.

iter11 0.03s; erc20_independent 0.48s. Multi-block: `lab/notes/light-cover-multiblock.md`.

3-iter reuse sweep 3356896: OCC reuse 1.329 / SF reuse **1.273** / `Win_1→Win_2` / unf=1 — SF reuse ≤ OCC on that harness. 33/52 OCC-gap blocks before 19469097 OOM; 9/33 SF reuse ≤ OCC; 9 last-arm `Win_2`; Soft=0 every row.

```
SPECFENCE_COMPARE_ITERS=7 cargo run -p pevm --release \
  --config 'profile.release.lto=false' --example specfence_3356896_compare

SPECFENCE_ALL_REUSE=1 SPECFENCE_ALL_ITERS=3 SPECFENCE_ALL_PROCESS_TOP=0 \
  cargo run -p pevm --release --config 'profile.release.lto=false' \
  --example specfence_all_blocks_sweep
```
