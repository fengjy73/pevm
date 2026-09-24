# Soft0-JoinAfterSpan / QuietExit

**日期：** 2026-09-24（北京时间）
**起点：** `1b2a408e351204b6675440729c531d7e354adfd9`（PR #51 尖，AdmitShard + NonSpanPhaseSplit）
**对照数字：** `8832029` 笔记。薄块 join-out **0.217**，大块 **3.046**。本刀不宣称 ≥1.5。

## 目标

Soft=0 Instant-off 下，工人在 **BlockQuiet** 成立后 **QuietExit**，宿主仍 `thread::scope` join。join 尾从「yield/heal 直到 `all_validated`」收成末次有用完成 + ε。保持 AdmitShard、HandoffSlot、WaitOnce 真 tip、ExactWake、Instant-off。`seq≡par`，`occ_picks=0`，`spine_cores_max≤1`。

## 约束

- 无 Estimate 门 Avoid，无 park-all，无同帧 suspend 当 Avoid，不把 SpineHop 放进可偷 deque。
- 主指标 TPS SF/OCC。成功是 join-out / tax 相对 `8832029` 下降，且 ratio 方向向上。纸面乐观上界仍 <1.5。

## 步骤

1. **已完成：** 读设计笔记与 `worker.rs` 空转出口。出口只认 `all_validated && pending_work==0`，否则 `force_idle_recover` / ExactPark / `yield_now`。`all_validated` 在计数未满时会扫整块。
2. **进行中：** 落地 `quiet_exit`。BlockQuiet = 分片空 ∧ handoff 空 ∧ 无 `ST_RUNNING` ∧ 无未发布 WaitOnce/ordered tip ∧ validate/wave 已排空 ∧ 每个 incarnation 已验证（看标志，不看 `num_validated`）。末工人仍在验证时，已无未执行交易且 tip 已发布的空闲工人先退出。`pick→None` 的 steal 未命中即 AdmitShards 的探测。宿主 join 不变。
3. **待做：** 2 核 `seq≡par` 冷检，再对 3356896 与 15274915 做 release、LTO off、N=5、请求 8 核的 Soft=0。写出 SF/OCC/ratio/span/tax/join-out，并和 0.217 / 3.046 比方向。
4. **待做：** 提交、推送、开 PR。数字进 PR 正文。验证后把新颖点写入 `docs/经验记录.md`。
