# Soft=0：OCC 为什么更早做完同一块的 EVM

**日期：** 2026-09-24（北京时间）
**代码：** `59f98c9`。在 `vm.execute`、`finish_execution`、验证和 pick 上加了线程时间。不改 pick、验证或发布的决定。
**块：** 15274915。3356896 只作对照。
**协议：** Soft=0 Instant-off，release，LTO off，请求 8 核，宿主 4 核，`taskset -c 0-3`。大块 N=5，薄块 N=3。复用中位是四次复用墙的 `sorted[2]`（两次复用时是较慢的那次）。2 核冷检 `SPECFENCE_COMPARE_CHECK=1`、`ITERS=1`。

## 结论

OCC 更早结束，是因为同一次 `run_pevm_tx` 更短，加上 SpecFence 在解释器前和每次成功之后的验证更贵。OCC 成功执行的次数更多，abort 也更多。差不在「SF 多做了一遍块」。

`finish_execution` 每次复用都只多大约 **0.8 ms 线程**，摊到 4 核大约 **0.2 ms 墙**。它不是 1.3–1.9 ms 的差。

Soft=0 Instant-off 在这块上到不了 ≥1.5。把整段差抹平，ratio 也只到 1。`ge_1_5=false`。

## 这轮墙，不替换锁定带

锁定带仍是 dig 的 SF **7.065** / OCC **5.306**，以及无探针 FirstExecCut 的 **7.210** / **5.900**。差约 **1.3–1.9 ms**。本探针每次交易打几下 `Instant`，不是每次读都打。大块复用中位：

| 项 | 值 |
| --- | ---: |
| SF / OCC | **8.020 / 5.823 ms** |
| 差 | **2.197 ms** |
| ratio | **0.726** |
| `est` / `soft` / `occ_picks` | 0 / 0 / 0 |
| `spine_cores_max` / 链 | 1 / 77 |
| focus 端 | 116 → 1219，span **3.326 ms** |
| `first_cut` | 1141 |

四次复用 SF 墙是 7.681 / 7.965 / 8.020 / 8.147。OCC 五次是 5.334 / 5.757 / 5.823 / 5.847 / 6.383。8.020 落在先前无探针复用里出现过的 8.198 那一档，不另立一条墙。2 核冷检两块都打印 `seq=par ok`，SF `occ_picks=0`。冷检时还没有学到的链，`spine_cores_max=0`。

## 同一次调用拆开

`vm.execute` 分成三段：进 `run_pevm_tx` 之前（pre）、`run_pevm_tx`（interp）、返回之后的写集和 `MvMemory::record`（post）。`finish_execution_with_wave_fence`、验证、pick 另计。

OCC 的 pick 是整段 `next_occ_task`，含等到有任务为止的 `yield`。SF 的 pick 只是 `schedule::pick`。SF 的空转仍在原来的 `idle_ns` 里，不在 pick 里。

两边都走 `vm.execute` → `chain.run_pevm_tx`。OCC 在 `finish_execution` 之后，要么马上验证这一笔（验证游标已经越过它），要么回到 `next_occ_task`；验证积压时调度器先取验证。SF 每次成功执行之后，同一工人先做 `validate_to_plan` + `resolve_plan::apply`，然后才再 pick。

## 大块复用：次数和线程时间

冷启动（iter 0）SF 墙 52.650 ms，`run_pevm_tx` 360 ms 线程，是第一遍 WaitOnce。下面只看四次复用。`ok` 是 `vm.execute` 返回成功的次数。块长 1226。

| iter | 墙 SF / OCC | SF ok | OCC ok | OCC abort | SF abort | SF block | OCC block |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 7.965 / 5.757 | 1239 | 1342 | 116 | 12 | 30 | 112 |
| 2 | 8.147 / 5.823 | 1239 | 1355 | 129 | 10 | 32 | 95 |
| 3 | 7.681 / 5.334 | 1237 | 1304 | 78 | 10 | 13 | 87 |
| 4（中位墙） | 8.020 / 5.847 | 1236 | 1347 | 121 | 9 | 33 | 252 |

OCC 每次都多大约 70–120 次成功执行，和 abort 数同一档。SF 的成功次数只比块长多大约 10。重复功在 OCC 一侧。

线程时间差（SF − OCC，毫秒）。除以 4 是「若 4 个核一直满、多出来的线程时间摊进墙」的上界，用来和墙差对照，不是另一条测量墙。

| iter | 墙差 | pre | interp | post | finish | val | pick | 合计/4 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | +2.208 | −0.72 | +3.45 | +7.38 | +0.84 | +2.93 | −4.17 | +2.43 |
| 2 | +2.324 | +2.62 | +4.90 | +1.47 | +0.83 | +1.35 | −2.76 | +2.10 |
| 3 | +2.347 | +2.33 | +4.82 | +0.34 | +0.75 | +2.45 | −3.02 | +1.92 |
| 4 | +2.173 | +0.80 | +2.85 | +12.04 | +0.77 | −0.24 | −5.89 | +2.58 |

合计/4 和墙差同号、同一档（1.9–2.6 ms 对 2.2–2.3 ms）。多出来的线程时间够解释这轮墙差。

四次都成立的差：

- **`run_pevm_tx` 多 2.85–4.90 ms 线程。** iter 3 上 SF 10.429 ms / 1237 次 ≈ **8.4 µs**，OCC 5.610 ms / 1304 次 ≈ **4.3 µs**。大约 **4 µs/次**。摊到 4 核大约 **0.7–1.2 ms 墙**。这是四次里唯一每次都有、而且到毫秒墙的桶。
- **`finish_execution` 多 0.75–0.84 ms 线程。** 大约 **0.2 ms 墙**。FirstExecCut 里「约 4%、墙上界约 0.5 ms」这一档得到确认，并且更小。
- **pick：SF 更短 2.8–5.9 ms 线程。** OCC 的调度循环含 yield。SF 的 `schedule::pick` 不是这道差。中位墙那一轮 SF `idle_ns` 3.70 ms、`heal_ns` 1.92 ms（各线程之和）。空转没有把 SF 单独拉开。

其余桶：

- **pre** 三次为正（+0.8 到 +2.6 ms 线程）。这是 `run_pevm_tx` 之前的 SF 前奏（链起点、tip、门控判断）。OCC 的 pre 本身就有 4–6 ms，两边都有 `set_tx` 和清 journal。差大约 **0.2–0.7 ms 墙**。
- **验证** 三次为正。SF 的 `val_n` 贴近 `ok`（约 1245），一次成功配一次 `validate_to_plan`。OCC 的 `val_n` 是 2164–4186，大约 1.7–3.1 倍于自己的 `ok`，单次更便宜（iter 3：SF 3.4 µs，OCC 0.8 µs）。次数少、单次贵，线程时间仍然多大约 **0.6 ms 墙**。
- **post（写提交）不稳定。** iter 3 只多 0.34 ms 线程，墙差仍是 2.35 ms。iter 4 多 12 ms 线程。写提交可以在单轮里胀到墙差那么大，但墙差在写提交几乎打平时还在。它不是这道差的必要条件。

iter 3 把稳定项加起来：interp 4.82 + pre 2.33 + val 2.45 + finish 0.75 + post 0.34 − pick 3.02 = **7.67 ms 线程**，/4 = **1.92 ms**，实测墙差 **2.35 ms**。

## 和「90% 是两边都付的解释器」怎么接

FirstExecCut 的 90% 是 SF 首次 Opt 内部的剩余桶：Detect、跳过分支、MV、`finish_execution` 都扣掉之后，剩下的是解释器加写提交。那次探针的 `Instant` 打在每次读上，绝对墙不能用。它没有和 OCC 的 `run_pevm_tx` 比。

这次拆开之后，复用轮上 SF 的 `run_pevm_tx` 大约是 `vm.execute` 的三分之一，写提交常常同样大，前奏大约四分之一。OCC 的 `run_pevm_tx` 更短。两边都做解释器，SF 的这一次更贵大约 4 µs。跳过分支先前是 0.18 ms 线程，对不上这 3–5 ms。

## 薄块

3356896，N=3，复用中位是较慢的那次：SF **1.536** / OCC **1.214**，差 **0.322 ms**，ratio **0.791**。`est=0`，`soft=0`，`occ_picks=0`，`spine_cores_max=1`，链 4→171。

这一行上 `run_pevm_tx` 只多 0.13 ms 线程，post 多 0.44，验证和 pick 是 SF 更短。线程时间合计 SF 更少，墙仍高 0.32 ms。薄块的零点几毫秒对不上大块那 4 µs×一千次的形状，也不拿来改大块的归因。

## 下一问（只设计）

**停。** 不再把 Soft=0 读元数据跳过、`first_start` 前移、QuietExit，或 `finish_execution` 微刀当作这块的墙。`Soft0-FinishExecPublishCut` 的实测上界大约 **0.2 ms 墙**，小于锁定差，也到不了 1.5。

**一块到不了 ≥1.5。** 定义 ratio = OCC 墙 / SF 墙。本轮 0.726。锁定带 0.751 和 0.818。SF 贴到 OCC 也只是 1。≥1.5 要求 SF 明显快于 OCC。Instant-off 上没有 soft 重放这条杠杆。

若还要看那 4 µs 是不是 SF 的 VmDb 钩子，下一份只做测量、不改调度：`Soft0-InterpDeltaSplit`。同一对时钟，把 `run_pevm_tx` 再分成 revm 操作码和 VmDb。诚实上界是解释器那 **0.7–1.2 ms 墙**。若拆开之后差在操作码上，而不是 VmDb，Soft=0 的路径刀就停：剩下的是两套调度把同一次操作码放在不同时刻，Instant-off 不再有元数据可切。

## 硬边界

无 Estimate 门，无 park-all，无 #47，无加核。不把 Ideal 的 1.19 / 3.02 ms 当成 Soft=0 墙。不宣称 ≥1.5。
