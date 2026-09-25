# Soft0-RegionLearnAvoid v2：同机 ABAB，门是 FAIL-miss

**日期：** 2026-09-25（北京时间）
**测量二进制：** `cace484`（`fix(specfence): wait the writer who actually stored the region`）。后面的文档提交不改变这只二进制。
**起点：** `bb2361f`（PR #57 尖）。本刀是 PR #59，不叠 PR #58。#58（`89197b1`）只作对照。
**块：** 15274915（n=1226）为主，3356896（n=176）对照。大块 Ideal LB = max(1.19, 3.02/4) = **1.19 ms**。薄块沿用本系列已写过的 **0.031 ms**（`specfence-per-tx-vs-parallel-bound`，经 Ideal-timed 笔记引用），不是这次新测出来的。
**协议：** Soft=0 Instant-off，release，LTO off，请求 8 核，宿主 4 核，`taskset -c 0-3`。`SPECFENCE_GLOBAL_IDEAL_READY_POOL=0`，`SPECFENCE_IDEAL_TIMED_ADMIT=0`，`SPECFENCE_COMPARE_CHECK=1`。大块 N=5，薄块 N=3。冷启动 iter 0 不进复用门。复用中位是例程的 `sorted[len/2]`（四次取第三，两次取较慢的那次）。ABAB 是四个独立进程：刀开、刀关、刀开、刀关。旗是进程级 `OnceLock`，不能在一个进程里翻转。
**锁定带不换：** SF **7.0–7.2** / OCC **5.3–5.9**。#58 大块刀开 8.767 / 5.817 / 0.664 / SF−Ideal 7.577，同机关刀 8.252 / 5.941 / 0.720 / 7.062。QuietExit 锚 SF−Ideal **5.875**。`ratio` 与锁定表相同，是例程的 `sf_tps/occ_tps`（等于 OCC 墙 / SF 墙）。`ge_1_5` 只在这个 ratio ≥ 1.5 时为 true。

## 结论

**门是 FAIL-miss。** 非脊位置确实武装了，脊没有被第二次串行，`busy_ns` 没有升，`nontoucher_probes=0`。已经武装的 region 上 FullReplay 仍然超过每轮 1 次，第二轮复用的 Σ`full_from_0` 也没有降到关刀的 0.6 倍。墙的中位在两轮都低于同机关刀 0.30 ms 以上，但这不补上 G3。

`ge_1_5=false`。四次大块进程的 ratio 是 **0.861 / 0.725 / 0.712 / 0.635**。最高的一档 SF/OCC 墙比是 6.724/5.789 = **1.162**，不到 1.5。纸面预期的「单独大约 0.1–0.6 ms」对不上这次的墙差（第一轮中位差 0.991 ms，第二轮 1.576 ms，第二轮关刀本身偏慢）。不把墙差写成已经稳定的产品收益。

八个进程都是 `seq=par ok`。复用轮 `est_block=0`，`soft_wait_arms=0`，SpecFence `occ_picks=0`，`spine_cores_max=1`，`wait_for_dependency=0`。大块复用 `handoff=76`、`chain_ab=76`，刀开刀关一样。region 行里没有脊 basic `abd6bb397881`。

## 门

两轮分开判，再看合并。PASS 要求每一轮都过。

| 门 | 第 1 轮 | 第 2 轮 |
| --- | --- | --- |
| G1 墙 | 中位 6.724 ≤ 7.715−0.30，均值 7.085 < 7.608。过 | 中位 8.154 ≤ 9.730−0.30，均值 7.739 < 9.380。过。关刀这轮偏慢 |
| G2 span 均值 | 2.597 ≤ 2.079+0.30？不过，超 0.22 ms | 1.748 ≤ 3.649+0.30。过 |
| G3 Σ`full_from_0` | 18 ≤ 0.6×39=23.4。过 | 33 ≤ 0.6×31=18.6？不过 |
| G3 已武装 FullReplay | iter2/3/4 为 1 / 1 / **2**。iter4 超 1。不过 | iter2/3/4 为 **19 / 10 / 1**。不过 |
| G4 `busy_ns` | 均值 34.11 ms ≤ 37.12×1.03。`nontoucher_probes=0`。过 | 36.46 ≤ 45.88×1.03。探针 0。过 |
| G5 薄块 | 刀开中位 1.686，两次复用 1.686 / 1.379，`yield_deadlock=0`。刀关有一轮 **16.632**（下面单说） | 刀开 1.370，两次 1.362 / 1.370，都 ≤ 刀关较慢样本 1.821+0.15。`yield_deadlock=0`。过 |
| 不变量 | 复用轮 est=0 soft=0 occ_picks=0 spine_cores_max=1，`seq=par ok` | 同 |

合并八次大块复用，中位仍用 `sorted[len/2]`：刀开 **7.673**、均值 7.412，刀关 **8.043**、均值 8.494。7.673 ≤ 8.043−0.30，G1 仍过。span 均值 2.173 ≤ 2.864+0.30，G2 过。Σ`full_from_0` 51 ≤ 0.6×70=42？不过。已武装 FullReplay 仍多次大于 1。所以合并也是 G3 失败。

**形状：FAIL-miss。** 不是 relabel（region 行是 learner 的 Full ℓ，不是脊），不是 tax（busy 低于关刀，非触及者探测为 0），不是 double-serialize（`handoff=76` 两边相同，脊 ℓ 不在 region 行），也不是 overwait（region `wait_ns` 单槽最高 0.98 ms，多数为 0，墙中位是降的）。纸上的 miss 行写的是「武装位置上 FullReplay≈0，但总数不降」。这次相反：位置武装了，FullReplay 就发生在这些位置上（`full_armed` 到 19），边没有盖住真正让读失效的写者。第一轮复用 iter1 的 radar 仍是 0，武装从 iter2 才开始。

Stretch：第 1 轮刀开 SF−Ideal **5.534**，低于锚 5.875。第 2 轮是 **6.964**，低于不了。不把锚写成已经稳定打穿。

## v1 三条假设，用这次计数核对

1. **v1 只武装了已经 Handoff 的脊 ℓ。** 在 v2 上不成立。大块复用 `handoff=76` 两边相同，`spine_cores_max=1`，region 行没有 `abd6bb397881`。
2. **FullReplay 在大约 7 个非脊 hot ℓ。** 成立，而且 v2 从第二次复用起武装了它们（`bef034365ca24581`、`930831d7501a43bf`、`313a4cd93f128a02`，以及 `d836a55a84878178` 等）。武装之后 `full_armed` 仍可以是 1–19，所以 scope 对了，边没有把 replay 拿掉。
3. **墙差来自每访问探测税，和 busy 相关，不是等待。** v2 上不成立。`nontoucher_probes=0`，两轮 busy 均值都低于关刀。第 1 轮 span 均值仍高 0.52 ms，对不上 region `wait_ns`（多数槽是 0）。

76ed8dc 的刀开大块（中位 7.667，`full_armed` 4/9/9）只说明「没写该槽的完成者被当成数据地板」。那只二进制不进这张门。

## 15274915 墙

OCC 中位含冷启动，SF 中位不含。这是比较例程的原规则。

| 轮 | SF | OCC | ratio | SF/OCC 墙 | SF−Ideal | `ge_1_5` |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| 刀开 1 | **6.724** | 5.789 | 0.861 | 1.162 | **5.534** | false |
| 刀关 1 | **7.715** | 5.592 | 0.725 | 1.380 | **6.525** | false |
| 刀开 2 | **8.154** | 5.802 | 0.712 | 1.405 | **6.964** | false |
| 刀关 2 | **9.730** | 6.178 | 0.635 | 1.575 | **8.540** | false |

### 逐次复用（冷启动不进中位）

刀开 1。冷启动墙 29.898，`full_from_0=26`，radar=0。

| iter | 墙 | span | `full_from_0` | busy_ns | `full_armed` | `full_unarmed` | radar | armed | handoff |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 8.314 | 3.980 | 10 | 43659115 | 0 | 24 | 0 | 0 | 76 |
| 2 | 6.674 | 2.221 | 2 | 30255852 | 1 | 1 | 12 | 9 | 76 |
| 3 | 6.724 | 2.441 | 3 | 31853810 | 1 | 8 | 12 | 9 | 76 |
| 4 | 6.628 | 1.747 | 3 | 30662991 | 2 | 1 | 12 | 11 | 76 |

刀关 1。冷启动墙 55.298，`full_from_0=14`。region 行都是 `enabled=0`。

| iter | 墙 | span | `full_from_0` | busy_ns | handoff |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 7.715 | 3.386 | 12 | 39627358 | 76 |
| 2 | 7.981 | 1.552 | 13 | 38052717 | 76 |
| 3 | 7.249 | 1.584 | 7 | 35603906 | 76 |
| 4 | 7.485 | 1.793 | 7 | 35194384 | 76 |

刀开 2。冷启动墙 41.889，`full_from_0=13`，radar=0。

| iter | 墙 | span | `full_from_0` | busy_ns | `full_armed` | `full_unarmed` | radar | armed | handoff |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 8.154 | 1.671 | 17 | 36706292 | 0 | 40 | 0 | 0 | 76 |
| 2 | 8.270 | 1.693 | 8 | 41676607 | 19 | 1 | 12 | 12 | 76 |
| 3 | 7.673 | 1.658 | 6 | 34495931 | 10 | 1 | 12 | 12 | 76 |
| 4 | 6.859 | 1.970 | 2 | 32974670 | 1 | 1 | 12 | 11 | 76 |

刀关 2。冷启动墙 33.050，`full_from_0=50`。`enabled=0`。

| iter | 墙 | span | `full_from_0` | busy_ns | handoff |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 9.730 | 4.618 | 9 | 51385352 | 76 |
| 2 | 8.043 | 2.399 | 9 | 35050418 | 76 |
| 3 | 8.352 | 1.487 | 6 | 47293417 | 76 |
| 4 | 11.396 | 6.091 | 7 | 49781610 | 76 |

### 已武装 region（只列 `full>0` 或 `wait>0` 的槽）

格式：`loc@armed_at wait/ord/pass/unstart/retain/full/wait_ns`。全部 `drained=0`，`nontoucher_probes=0`。

刀开 1 iter2：`bef034365ca24581@30` 1/0/8/1/0/1/0；`d836a55a84878178@70` 1/1/4/0/1/0/125845；`111015025e6393bd@5` 2/0/3/0/0/0/525773。另有 6 个槽只有 pass。

刀开 1 iter3：`bef0…@30` 1/0/8/2/0/1/5173；`d836…@70` 1/1/3/0/1/0/136801。

刀开 1 iter4：`bef0…@30` 2/1/7/1/0/1/158352；`d836…@70` 1/1/3/0/2/0/186193；`930831d7501a43bf@94` 0/0/2/0/1/1/0。

刀开 2 iter2：`bef0` full=1 wait_ns=565223；`d836` full=3；`313a4cd93f128a02` full=2；`399e49049ba23038` full=1；`c9b7a21e37bae100` full=1；`1110` full=2；`358b0701ba58e54a` full=2；`38c82bebfe2a0a05` full=2；`64f292fc64ba7f6d` full=2；`8dd852ce9b4a8171` full=1；`9308` full=2。这一轮 `full_armed=19`。

刀开 2 iter3：`bef0` full=3；`d836` full=2；`313a` full=1；`9308` full=2；`399e` full=1；`c9b7` full=1。`full_armed=10`。

刀开 2 iter4：`bef0` full=1，wait=3 ord=2。其余槽 full=0。

## 3356896

薄块 Ideal 用 0.031 ms 只为了填 SF−Ideal 列。门本身不看这个差。

| 轮 | SF | OCC | ratio | SF/OCC 墙 | SF−Ideal | `ge_1_5` |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| 刀开 1 | **1.686** | 1.145 | 0.680 | 1.472 | 1.655 | false |
| 刀关 1 | **16.632** | 1.152 | 0.069 | 14.438 | 16.601 | false |
| 刀开 2 | **1.370** | 1.137 | 0.830 | 1.205 | 1.339 | false |
| 刀关 2 | **1.821** | 1.061 | 0.583 | 1.716 | 1.790 | false |

刀关 1 的 16.632 是复用 iter1：span 15.576，`yield_deadlock=1`，handoff=26。这是 #56 重者先弹在 `GLOBAL_IDEAL_READY_POOL=0` 上已知的大约 16 ms 薄块路径，出在**刀关**，不是 v2 把薄块做死。刀开两轮 `yield_deadlock=0`，墙 1.686 / 1.379 和 1.362 / 1.370，不是 1.421→2.442 那种 FAIL-B。#58 薄块用的也是这两个 Ideal 旗都关、而且当时刀关没有撞上 16 ms。这次干净的一对是刀开 2 对刀关 2。

| 轮 | iter | 墙 | span | `full_from_0` | busy_ns | `yield_deadlock` | handoff | radar |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 刀开 1 | 1 | 1.686 | 0.704 | 0 | 4384169 | 0 | 16 | 0 |
| 刀开 1 | 2 | 1.379 | 0.387 | 3 | 4401021 | 0 | 16 | 0 |
| 刀关 1 | 1 | 16.632 | 15.576 | 4 | 23259300 | 1 | 26 | enabled=0 |
| 刀关 1 | 2 | 1.457 | 0.716 | 1 | 4032654 | 0 | 15 | enabled=0 |
| 刀开 2 | 1 | 1.362 | 0.447 | 0 | 4709243 | 0 | 16 | 0 |
| 刀开 2 | 2 | 1.370 | 0.317 | 4 | 3811147 | 0 | 16 | 0 |
| 刀关 2 | 1 | 1.423 | 0.487 | 0 | 3707112 | 0 | 15 | enabled=0 |
| 刀关 2 | 2 | 1.821 | 1.125 | 1 | 4443464 | 0 | 16 | enabled=0 |

薄块两次复用都没有装上 radar（`armed=0`）。冷启动的 Full 没有变成下一轮的槽。大块要到第二次复用才有槽，薄块 N=3 只有两次复用，所以薄块这刀等于没武装。

## 四类

| 类 | 结果 | 证据 |
| --- | --- | --- |
| RAW | 未解决 | 刀开大块复用 `raw_c` 仍是 0–9。`bef034365ca24581` 上有 WaitOnce（wait 1–4），同一槽 `full` 仍是 1–3。未开工的前驱走 pass（`unstart` 1–3），随后的写仍能打出 FullReplay |
| WAR | 未解决 | Retain 只记了计数（单槽 retain 0–2），没有把读者版本钉进多版本历史。`war_c` 刀开仍出现（0–2）。两边都不等，但读者没有因此保住版本 |
| WAW | 未解决 | 非脊槽上有一跳 OrderedTip（`d836…` ord 1–2，`bef0` ord 0–3）。这些槽的 `full` 没有归零。脊 WAW 仍是 Handoff，`handoff=76` 两边相同 |
| 长链 | 交给脊，没有二次串行 | `chain_ab=76` 两边相同，`spine_cores_max=1`，region 行没有 `abd6bb397881`。长链不是这刀要新解决的边 |

## 机制（测量所在的那一版）

`SPECFENCE_REGION_LEARN_AVOID_V2` 默认开。`=0` / `false` / `off` 时表是 disabled：不装槽、不武装、访问路径不进表。关刀日志是 `enabled=0`。

- **Radar。** 上一块的 resolve / Full 位置种进下一块，块初不武装。脊 crit、有序脊位置、受益人、写者名单长度 ≥32 的位置不进槽。
- **Arm。** 预测写者开始访问、非脊 RAW peek、或第一次 resolve。只给证据交易之后的预测触及者置位。
- **边。** 最近的、还没发布这个位置的预测写者。已经完成但没写这个槽的人不是数据地板。地板之上更近的 live 写者优先。RAW 且前驱在跑：帧内等真 tip，离开未发布则 ExactWake。非脊 WAW：访问点一跳。WAR：计数 Retain，双方不等。前驱未开工：pass。Estimate 不是等待输入。
- **Disarm。** 每个预测写者要么发布了这个位置，要么成功结束且没写它，之后 Drained，后续 pass。这次日志里 `drained` 全是 0：早于武装就结束的写者没有被记进 published/skipped。
- **快路径。** 不是任何已武装 region 的预测触及者时，`VmDb` 上一位标志为假，`basic` / `storage` 不再查表。`nontoucher_probes=0`。

```text
CUT: Region arm on spine locations; WaitOnce as the WAR op; per-access region probes for non-touchers; region bookkeeping on the spine hop; Estimate-gated Avoid; admission park; whole-tx wait_edges; protect_hot as the Avoid vehicle; waiting on an unstarted producer
```
