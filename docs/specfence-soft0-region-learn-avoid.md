# Soft=0：热点边上的 WaitOnce 没有把墙和 SF−Ideal 打下来

**日期：** 2026-09-25（北京时间）
**二进制：** `89197b1`。测量在这一版上跑。后面的文档提交不改变二进制。
**起点：** PR #57 尖 `bb2361f`。本刀是新 PR #58，不追加进 #57。
**块：** 15274915 为主。3356896 对照。Ideal `L_crit` 大块 **1.19 ms**（锁定）。SF−Ideal = SF − 1.19。
**协议：** Soft=0 Instant-off，release，LTO off，请求 8 核，宿主 4 核，`taskset -c 0-3`。大块 N=5，薄块 N=3。复用中位是去掉冷启动后 `sorted[len/2]`（四次复用取第三，两次取较慢的那次）。`SPECFENCE_COMPARE_CHECK=1`。主墙不设 `SPECFENCE_IDEAL_PROXIMITY_DIFF`，也不设 `SPECFENCE_INTERP_SPLIT`。

## 结论

**门是 FAIL。** 主墙把 Ideal 供给两旗都关掉（`SPECFENCE_GLOBAL_IDEAL_READY_POOL=0` 且 `SPECFENCE_IDEAL_TIMED_ADMIT=0`），只留下标段，避免叠 #57 的 FAIL-A，也不把 #56 的段内重者先弹重新打开。同机刀开复用中位 SF **8.767** / OCC **5.817**，ratio **0.664**，SF−Ideal **7.577**。同机关刀 SF **8.252** / OCC **5.941**，ratio **0.720**，SF−Ideal **7.062**。刀开比刀关高 **0.515 ms**。QuietExit 锚是 **7.065** / SF−Ideal **5.875**。刀开没有低于同机对照，也没有低于锚。`ge_1_5=false`。

刀开的中位那次 `learn=1 raw=0 waw=1 war=1 chain=1`，`full_from_0=11`，链 **77**，头 **116** / 尾 **1219**，span **4.243**。关刀中位 `learn=0`，`full_from_0=12`，span **1.727**。类位翻了，FullReplay 几乎没少，span 更长。这是标签交换，不是墙胜。

薄块同协议刀开 **1.583** / OCC **0.986**，ratio **0.623**。关刀 **1.825** / OCC **1.165**，ratio **0.639**。刀开没有落到 1.421→2.442 那一档。薄块不构成 FAIL-B，也不把大块门抬成 PASS。

单独看，离 ≥1.5 还远。不宣称 ≥1.5。锁定带 7.0–7.2 / 5.3–5.9 不换。本机 8.767 / 8.252 是这次下标段的实测，不是新的锁定带。

## 刀在做什么

`SPECFENCE_REGION_LEARN_AVOID` 默认开。`=0` 是同机关刀。块初只记 spine 链雷达，不武装。本块第一个链成员被 pick 时，对该 `ℓ` 装 WaitOnce（peer 保持 0，不写 `wait_edges`，不调用 `protect_hot`）。之后这条边上的读用 `region_pred` 看前一个写者的真 tip。ExactWake 看 `is_region_armed`。薄块 admission 仍然直接返回。beneficiary / NeverWait 不入。无关 Indep 不离队。没有 Estimate 门。

第一版曾调用 `protect_hot` 并让薄块 admission 把整笔交易停到前驱结束。那是整 tx 离队，设计禁止。单元测试现在断言 `protect_live` 保持关、`has_wait_once_peer_before` 对链成员为假、`region_pred` 指向相邻写者。

## 为什么主墙两旗都关

只设 `SPECFENCE_GLOBAL_IDEAL_READY_POOL=0`、留下 `SPECFENCE_IDEAL_TIMED_ADMIT` 默认开，会回到 #56 的段内重者先弹。#57 笔记里这台机器的薄块复用中位是 **16.562 / 16.270**，`span_head=31`，`defer` 数千。本会话在同一二进制上又看到刀开 **17.531**、刀关 **16.775 / 16.426**，形状相同（`yield_deadlock=1`，`defer` 七千到九千）。那是 #56 路径，不拿来当 RegionLearn 的地板，也不拿来当本刀已经赢了薄块。下标段（两旗都关）才是不叠 FAIL-A、也不重开 #56 的主墙。

## 15274915（n=1226）

下标段。`est=0`，`soft=0`，`occ_picks=0`，`spine_cores_max=1`，`seq=par ok`。

| 轮 | SF | OCC | ratio | SF−Ideal | `ge_1_5` |
| --- | ---: | ---: | ---: | ---: | --- |
| 刀开 | **8.767** | 5.817 | 0.664 | **7.577** | false |
| 同机关刀 | **8.252** | 5.941 | 0.720 | **7.062** | false |
| QuietExit 锚（锁定，非本机新数） | 7.065 | 5.306 | 0.751 | 5.875 | false |

四次复用 SF 墙：刀开 8.140 / 8.342 / 8.767 / 9.264；关刀 7.025 / 7.855 / 8.252 / 8.324。冷启动 26.234 / 21.366，不进中位。

刀开中位（墙 8.767，iter 1）：`REGION_LEARN learn=1 raw=0 waw=1 war=1 chain=1`。`defer=0`，`handoff=76`，`yield_deadlock=0`。关刀中位（墙 8.252，iter 1）：`learn=0`，`defer=1`，`handoff=76`，`yield_deadlock=0`。

## 3356896（n=176）

同一下标段。`est=0`，`soft=0`，`occ_picks=0`，`spine_cores_max=1`，`seq=par ok`。`yield_deadlock=0`。

| 轮 | SF | OCC | ratio | `ge_1_5` |
| --- | ---: | ---: | ---: | --- |
| 刀开 | **1.583** | 0.986 | 0.623 | false |
| 同机关刀 | **1.825** | 1.165 | 0.639 | false |

两次复用 SF 墙：刀开 1.583 / 1.448，中位取较慢的 1.583；关刀 1.825 / 1.310，中位 1.825。刀开中位 `learn=1 raw=0 waw=1 war=1 chain=0`，`full_from_0=0`，`defer=2`。关刀中位 `learn=0`，`full_from_0=0`，`defer=6`。

## 判词

FAIL。大块墙和 SF−Ideal 相对同机关刀和 QuietExit 锚都更差。薄块没有 FAIL-B 式回退，但 ratio 仍远低于 1.5。不是 Estimate 改名，不是 park-all，也不是把全部 Indep 串行；是访问边上的 WaitOnce 类位换了，冲突形的 SF−Ideal 没有缩小。
