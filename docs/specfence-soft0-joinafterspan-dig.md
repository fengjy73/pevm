# Soft=0：大块 span 结束之后 join 里还在跑什么

**日期：** 2026-09-24（北京时间）
**代码：** `114cdda`（QuietExit 之上的探针，调度没改）
**块：** 15274915。对照 Ideal 用库内 `lab/notes/specfence-per-tx-vs-parallel-bound-v1.md`（2026-09-22，`95b635d`，不是这次二进制）。
**协议：** release、LTO off、Instant-off、N=5、请求 8 核、宿主 4 核、`taskset -c 0-3`。`ge_1_5=false`。

## 结论

QuietExit 已经把「最后一次有用工作」到「最后一名工人离开」收成 **0.114 ms**。中位墙上多出来的 **3.046 ms** join-out 是链尾 `first_start` 之后还在做的 AdmitIndep 执行，不是 BlockQuiet 成立之后的 heal / yield 空转。

中位墙（四次复用 `sorted[2]`，iter 4，链 **116 / 1219** 与 focus 一致）：

| 项 | 值 |
| --- | ---: |
| ratio | 0.751 |
| SF / OCC | 7.065 / 5.306 ms |
| span / tax / join-out | 2.089 / 4.976 / 3.046 ms |
| span 结束 → 最后离开 | 2.982 ms |
| 其中最后一次执行/验证之前 | 2.868 ms（96%） |
| 最后一次验证 → 最后离开 | 0.114 ms（4%） |
| BlockQuiet 首次成立 | 最后一次验证之后 0.042 ms |

`est=0`，`soft=0`，`occ_picks=0`，`spine_cores_max=1`。2 核冷检（探针二进制）两块都打印 `seq=par ok`。这一轮 ratio 高于 `8832029` 的 0.683，也高于 QuietExit 落地后无探针的 0.642 / 0.697。宿主 4 核上墙本来就晃，**不把 0.751 当成这刀变快**。

## span 结束时 BlockQuiet 为什么为假

探针在先验链尾写下 `first_start` 的那一刻拍一张快照，并在此后每次空转采样把仍为假的条款 OR 起来。条款位：`pending` AdmitShard/released/ordered/revalidate，`wave`，`handoff`，`wait_live`，`running`，`spine`，`gated`，`sleeping`，`unfinished`，`owed`。

中位墙这一拍：

| 计数 | 值 |
| --- | ---: |
| 还没有 `first_start` | 490 |
| 未 Executed/Validated | 497 |
| 已执行、还欠验证 | 0 |
| `ST_RUNNING` | 8 |
| `pending_work` | 545 |
| 仍在 AdmitShard | 486 |
| 这一拍为假的条款 | pending、running、spine、gated、sleeping、unfinished |
| 之后空转样本里出现过 | 再加 wave、wait_live、owed |
| 两次都没出现 | **handoff** |

所以：

- **AdmitShardsEmpty 失败。** 486 笔还在分片里。
- **NoRunning 失败。** 8 个工人都在跑。
- **TipsPublished 失败。** `gated` 和 `sleeping` 在这一拍就是真的。这一行 `yield_ok=5`、`idle_parks=0`、`exact_wakes=0`，WaitOnce 没有把墙站住。
- **ValidateDrained 在这一拍成立**（owed = 0）。空转样本里后来出现过 `owed`，但最后一次验证只比最后一次执行晚 0.002 ms，验证没有把墙再拉开。
- **HandoffEmpty 成立。** 单槽不是这段 join 的原因。

## 这段墙的时间花在哪

span 结束到最后离开是 2.982 ms。下面的核时间是各线程之和，可以重叠，不能加进 tax。`execute` / `validate` 按 `exec_origin` 把跨过 span 结束的那一段切开。`heal` 若在 span 结束前就开始、结束时 span 已经开了，整段算进 post-span，所以 heal 是上界。

| 类别 | 核时间 ms | 相当于几个线程（÷ 2.982 ms） |
| --- | ---: | ---: |
| execute | 14.054 | 4.71 |
| validate / resolve | 2.234 | 0.75 |
| heal | 2.697 | 0.90 |
| yield | 2.368 | 0.79 |
| park | 0 | 0 |
| steal miss（`pick → None`） | 0.022 | 0.01 |

516 次执行的起点落在 span 结束之后，平均约 **27 µs**。其中大约 490 次对应「这一拍还没开工」的那些笔，其余是链尾自己和少量再执行。执行占这段墙上约 4.7 个线程，宿主只有 4 个核：这段 join 被执行打满。空转只占最后 0.114 ms。

## 和 Ideal 比，这是不是「必须留下的反链」

库内那次追踪（不是这次二进制）对 15274915 的结构是：`n=1226`，写者链 `L=77`，层宽 `W_lvl=1120`，干净反链 1111 笔（`preds=0` 且一次执行且没有门），关键路径 13 笔，`L_crit = 1.19 ms`，成功执行的串行工作 `Σwork = 3.02 ms`。请求 8 核时 `Σwork/C = 0.38 ms`，理想 makespan 就是 `L_crit`，因为反链本来和关键路径重叠。

这次 span 不是那 13 笔。过滤器是学到的 77 笔写者链 **116 → 1219**，span 是这条链头到链尾的 `first_start` 之差（2.089 ms），链尾的执行体在 span 外面。DAG 并不要求那 490 笔等链尾开工。它们留在 join 里，是因为 handoff 把链尾送上去的时候，AdmitShard 里还有一半块没 pop。

就「这一拍已经如此」而言，这 490 笔必须再跑完，join 不能更早结束。就 Ideal 而言，这段不是关键路径剩下的长度。`L_crit` 1.19 ms 是整块的下界，不是这段 join 的下界。

## 为什么会出现 join-out 下降、span 上升、ratio 下降

同一次 N=5 里，对齐的三轮（focus 与过滤器都是 116 / 1219）：

| iter | SF 墙 | span | join-out | 链尾 `first_start` | 当时还没开工 |
| --- | ---: | ---: | ---: | ---: | ---: |
| 3 | 6.355 | 1.551 | 2.881 | 2.198 | 736 |
| 4（中位墙） | 7.065 | 2.089 | 3.046 | 2.672 | 490 |
| 2 | 6.754 | 3.505 | 1.074 | 4.116 | 12 |

链尾开工越晚，span 越长，开工前已经做掉的反链越多，join-out 越短。iter 2 的 span 比 iter 3 长 1.95 ms，join-out 只短 1.81 ms，墙还高 0.40 ms。工作从 join 挪进 span，墙留在 **6.3–7.1 ms**。

先前无探针的大块第 1 轮（span 3.606、join-out 2.712、墙 8.213、ratio 0.642）就是这种「链尾偏晚」的样本，不是 QuietExit 把空转从 join 里删掉了。基线 join-out 3.046 和这次中位 join-out 3.046 落在同一档；那次 span 更长，所以墙更高、ratio 更低。

iter 1 的 focus 头是 116、过滤器头是 122，`defer=1636`，不进这张表。

## 下一刀（只设计，这次不落地）

**缩短仍排在 AdmitShard 里的非链交易的第一次 `execute`。**

依据是：中位墙上这段 join 的 96% 被执行盖住，516 次调用约 27 µs，而 Ideal 笔记里整块成功执行的串行工作只有 3.02 ms。OCC 这次的墙是 5.306 ms，SF 是 7.065 ms，差 1.76 ms，比 3.046 ms 的 join-out 小。join-out 里的大部分工作 OCC 也要做，只是 SF 把它放在链尾开工之后，并且每次调用更贵。

边界：

- 对象是从 LocalAdmitDeque pop 出来、不是 handoff、也不在有序写者链上的那一笔。
- 保留它读集上的 Detect。不拿 Estimate 当门，不 park-all，不把 SpineHop 放进可偷 deque，不改 QuietExit。
- 成功看两件事同时发生：每次这种 `execute` 变短，并且 SF 墙下降。只把 `first_start` 提前会让 span 变长、join-out 变短，墙仍会落在上面那条 6.3–7.1 ms 的带里，这不算成功。
