# Detect 预付压税 × 多块输 OCC 类型 — 整包落地

**基线:** PR #33 `cursor/specfence-light-cover-mb-3352` @ `5be75bd`  
**分支:** `cursor/specfence-prepaid-losers-b5de`  
**设计:** `uploads/specfence-3356896-prepaid-plus-loser-classes-v1.md`  
**Soft=0 · 一条脊 · 无 P0/P1/P2 分期**

`CostPolicy::select_arm(ℓ)` 仍是唯一决策口。轻覆盖 + reexec→CC + S1 未闸 OCC pick 保留。

## 落地

| ID | 内容 |
|----|------|
| **P1** | `note_started` 只在本 tx 有闸或 leftover flush 排队时打点。兄弟闸不再税独立集。 |
| **P2** | Detect 洞打开（未闸，或有闸但 pred 已 Done）走 `validate_optimistic_fast` ≡ OCC。 |
| **P3** | ĉ 重叠感知：薄块 Detect 墙是 stall 工期，不是 hops×stall。U 欠覆盖才加 leftover OCC 尾。 |
| **U** | `train_hat`（cores 标度，176@8 → 7/8）允许 `w_need` 越过 oversub hat=2。`loc_w_need` 不再把已学 need 钳回 2。 |
| **O** | 已 cover_ok 不再 T3 滑 leftover Detect。肥块 sticky 可留 Win_1 / Opt。 |
| **D** | 欠覆盖列车（unf≥8 或 loc reexec≥4 且 leftover>planted）记 double_pay 并升 `w_need`；`hops < w_need` + OCC 尾仍禁。 |
| **L4** | sys-reexec 后即使先验 cover 贵，也抬 Opt/Defer 到 cover+δ，禁止钉 Opt。无 sys 的预付爆破仍可回 OCC。 |
| **S** | n≥512 复用且 D1 已存 → 跳 HotSet / inter-prior / sketch。`train_hat` 肥块 ≤4。 |

半截窗 = 短于 `w_need` 的前缀。轻窗 leftover 仅当 loc 仍漏列车才 T3 滑。

## 验收证据（代码）

- `under_cover_train_grows_w_need_past_oversub_hat`
- `sys_reexec_after_blowout_does_not_nail_opt`
- `leftover_slide_off_when_cover_ok` / `cover_ok_does_not_queue_leftover_slide`
- `train_hat_exceeds_oversub_light_hat`
- 既有 light cover / double_pay / prepaid blowout / sys-reexec / leftover / Full 禁长脊

```
SPECFENCE_COMPARE_ITERS=7 cargo run -p pevm --release \
  --config 'profile.release.lto=false' --example specfence_3356896_compare

SPECFENCE_ALL_REUSE=1 SPECFENCE_ALL_ITERS=3 SPECFENCE_ALL_PROCESS_TOP=0 \
  cargo run -p pevm --release --config 'profile.release.lto=false' \
  --example specfence_all_blocks_sweep
```
