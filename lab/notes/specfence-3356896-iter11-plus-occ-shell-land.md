# 3356896：iter11 自旋 + OCC 单付路径壳税 — 整包落地

**基线:** PR #30 `cursor/specfence-cut-all-overhead-26b8` @ `47f4a6c`  
**分支:** `cursor/specfence-iter11-occ-shell-59d5`  
**设计:** `uploads/specfence-3356896-iter11-plus-occ-shell-v1.md`  
**Soft=0 · 一条脊 · 无 P0/P1/P2 分期 · 不合并**

`CostPolicy::select_arm(ℓ)` 仍是唯一决策口。长脊 Opt/Defer 单付保留。禁 mid-plant / Instant idle→ĉ / 长脊 Win 前缀双付。

## A. iter11 自旋根因（I1–I3）

PR30 O5 在 `!has_any_gated()` 时跳过 `note_producer_done_stamp`。单付 OCC 上一次 abort persist 会在**下一 pick** `flush_pending_idle_edges` 种 `succ←pred`。若 pred 已 `scheduler.is_done` 但 ReadyEdge 未盖章：

1. `is_writer_done(pred)=false` → 种边成功  
2. succ 进 sleeping，`may_execute` 永远 false  
3. `next_task` 在 `waiting=true` 时**跳过** `min_runnable`  
4. 四核 `yield_now` / spin → ~400% CPU，12min+ 不是 DashMap 死锁  

iter11 的 24-CALL 多 SSTORE 正好走这条：首个 storage abort 排队 1 hop，pred 已成功且当时无闸。

**修复（不回退 Done-on-success / 未闸 OCC pick / 禁 mid-plant）：**

| ID | 落地 |
|----|------|
| **I1** | 成功 incarnation **必** `note_producer_done_stamp`（wake 仍仅 waiter） |
| **I1** | 耗尽 idx：`heal_finished_preds(scheduler.is_done)` + `wake_ready_sleepers` |
| **I1** | 有 `pending_idle` 时仍 `note_started`，flush 不能种 in-flight succ |
| **C4/I1** | 仅 sleeper pred **Executing** 时跳过 `min_runnable`；短洞不挡独立集 |

## B. OCC 单付壳（C1–C4）

| ID | 落地 | 相对 PR30 |
|----|------|-----------|
| **C1** | 接线已有 `batch_park_abort`：未完成 writer 上 park，不立刻对 ESTIMATE 重跑 | 无效 reexec ↓ |
| **C2** | commute accept = 一次 `last_locations` 锁（`try_commute_rebind_invalid`）；非 value-transfer 走 OCC validate | 去掉 collect Vec + 二次 rebind |
| **C3** | `edge_4_31`+thin → `end_block_learn_stable_d1`（跳 morph flush / 再扩窗） | end_block 再削 |
| **C4** | 上表 min_runnable；短 ℓ refuse 不阻塞独立 tx | 与 S1 一致 |

未恢复长脊 Full/Win 前缀双付。Soft=0。Instant idle 不进 ĉ。

## 验收证据（代码）

- `done_stamp_prevents_late_flush_refuse_forever`
- `heal_finished_preds_unsticks_late_plant`
- `heal_done_pred_wakes_late_flush_sleeper`
- `short_hole_does_not_block_independent_min_runnable`
- `stable_d1_learn_does_not_rewiden_leftover_opt`
- `commute_ok_matches_per_location`
- `flush_skips_done_pred_and_started_succ`（已有）

## Compare

```
SPECFENCE_COMPARE_ITERS=7 cargo run -p pevm --release \
  --config 'profile.release.lto=false' --example specfence_3356896_compare
```
