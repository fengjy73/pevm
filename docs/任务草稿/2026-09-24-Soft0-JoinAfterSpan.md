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
2. **已完成：** 落地 `quiet_exit`（`a5d23e9`）。BlockQuiet = 分片空 ∧ handoff 空 ∧ 无 `ST_RUNNING` ∧ 无未发布 WaitOnce/ordered tip ∧ validate/wave 已排空 ∧ 每个 incarnation 已验证（看标志，不看 `num_validated`）。末工人仍在验证时，已无未执行交易且 tip 已发布的空闲工人先退出。`pick→None` 的 steal 未命中即 AdmitShards 的探测。宿主 join 不变。热路径只看验证计数。
3. **已完成：** release、LTO off、Instant-off、`taskset -c 0-3`、请求 8 核、N=5 两轮。2 核冷检两块 `seq=par ok`。中位墙协议 `est=0 soft=0 occ_picks=0 spine_cores_max=1`。`ge_1_5=false`。join-out / tax / ratio 相对 `8832029` 没有稳定同向改善。数字在 PR #52。
4. **已完成：** 分支 `cursor/soft0-joinafterspan-quietexit-0f0e`，PR #52（基线是 #51 分支）。经验写入 `docs/经验记录.md`。草稿留到用户验收。

## 实测（中位墙那一轮）

宿主 4 核。ratio 是 harness 的 OCC 中位墙 / SF 复用中位墙。join-out 用 focus `[head_ms, tail_ms)` 与 `[join_mark, join_mark+join_wait)` 的差。

| 轮 | 块 | ratio | SF | OCC | span | tax | join-out | before | end_block | 链端 |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 基线 `8832029` | 3356896 | 0.697 | 1.442 | 1.006 | 0.582 | 0.860 | 0.217 | 0.215 | 0.101 | 过滤器头 31 ≠ focus 4 |
| 1 | 3356896 | 0.651 | 1.457 | 0.948 | 0.285 | 1.172 | 0.524 | 0.252 | 0.086 | 4/171 对齐 |
| 2 | 3356896 | 0.692 | 1.502 | 1.039 | 0.483 | 1.019 | 0.454 | 0.217 | 0.105 | 4/171 对齐 |
| 基线 | 15274915 | 0.683 | 7.615 | 5.202 | 2.394 | 5.221 | 3.046 | 0.672 | 0.338 | 116/1219 对齐 |
| 1 | 15274915 | 0.642 | 8.213 | 5.276 | 3.606 | 4.607 | 2.712 | 0.573 | 0.076 | 116/1219 对齐 |
| 2 | 15274915 | 0.697 | 8.144 | 5.680 | 2.085 | 6.059 | 3.925 | 0.685 | 0.076 | focus 与过滤器头都是 122，不是 116 |

大块第 1 轮 join-out 和 tax 低于基线，但 span 拉到 3.606，SF 墙更高，ratio 更低。其余中位行 join-out 高于 0.217 / 3.046。

## 续：join 窗里还在跑什么（2026-09-24）

**目标：** 解释 QuietExit 为什么没有砍掉大块 span 结束之后的多毫秒 join-out。只挖 15274915。不追 1.5，不改调度。

**做法：** 现有 `idle_ns` / `heal_ns` / `post_exec_*` 是各核之和，并且 post-exec 按窗口起点分类，回答不了「span 结束到最后一名工人离开」这段墙上谁占着。补一组最小探针：span 结束快照、最后一次执行/验证、BlockQuiet 首次成立、最后离开、span 之后的执行/验证/heal/yield/park/steal 核时间，以及 BlockQuiet 失败条款的位。然后只对 15274915 重跑 Soft=0。

**步骤：**

1. **已完成：** 探针 `114cdda`。
2. **已完成：** 2 核两块 `seq=par ok`。大块 N=5：中位墙 ratio 0.751（7.065 / 5.306），join-out 3.046，最后有用工作到离开 0.114 ms。`ge_1_5=false`。薄块没再跑 N=5，大块已经能分开执行和空转。
3. **已完成：** 根因写在 `docs/specfence-soft0-joinafterspan-dig.md`。下一刀是缩短 AdmitShard 里非链交易的第一次 `execute`，不是更早退出。
