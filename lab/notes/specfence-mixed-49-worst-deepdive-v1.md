# SpecFence mixed-49 最差 10 块深挖（PC / CC / Learn）

**日期:** 2026-09-22
**位置:** `lab/notes/`（本文件）· 简报 [`specfence-mixed-49-worst-deepdive-brief-v1.md`](specfence-mixed-49-worst-deepdive-brief-v1.md)
**引擎:** [#45](https://github.com/fengjy73/pevm/pull/45) `06fd9d9`（调度未改）。计数来自 `5ad6549` 上的 Instant-off 比较器。
**板:** [`specfence-mixed-49-pr45-soft0-tps-v1.md`](specfence-mixed-49-pr45-soft0-tps-v1.md)（49 块，赢 3/49，中位 SF/OCC **2.15**）
**性质:** 只分析。本轮不改 CC、不改调度、不恢复 `next_task*`。
**Soft=0。** 每个 SpecFence 迭代 `occ_schedule_picks=0`，`soft_wait_arms=0`。

原始 JSON（gitignore）：

- `lab/results/mixed-49-worst-deepdive/<block>.json` — 比较器逐迭代
- `lab/results/mixed-49-worst-deepdive-summary.json` — 慢复用行 + DAG 下界
- `lab/results/mixed-49-worst-deepdive-upper.json` / `.csv` — 串行与有效 DAG

---

## 0. 结论

十个输家都跑在并行脊上，而且都输。本挖 Instant-off（N=3，请求 8 核，宿主 4 核）的 SF/OCC 是 **2.05–4.42**。板上的 5.42（`3356896`）是一次 7.97 ms 的慢复用样本；同规则再跑，复用是 2.83 ms 和 3.75 ms，比冷启动 5.04 ms 快。比值会晃，机制不晃。

两把尺子：

1. **结构下界** `LB = max(crit, serial/C)`。`crit = serial × L / n`（等权跳），`L` 是本机 `specfence_all_blocks_upper_bound` 的有效 DAG（去掉 beneficiary / `basic_lazy`）。请求 `C=8`。十个输家的 SF 墙是 LB@8 的 **13–54×**，所以相对这条界全部 **FAR**。同一条界上 OCC 已经是 **5–25×**。这段距离里，大部分是两边共有的并行元开销。SpecFence 独有的一段是 SF/OCC。
2. **引擎内界** `max(gate_stall, busy/8)`，取慢的那次复用。`gate_stall` 是最长一条 Detect 洞的等待（makespan，不是求和）。`busy` 是执行+校验任务的墙钟合计，含重执行，也含 8 线程挤在 4 核上的抢占。贴着这条界（墙 / 内界 **< 1.4**）仍输 OCC，税在界里面。远离这条界（**≥ 1.6**），界上面还有一段 SpecFence makespan。

| 贴内界（NEAR） | 内界已经高于 OCC 的原因 |
|---|---|
| `8889776` 1.16、`19469097` 1.14、`19638737` 1.32、`14383540` 1.28 | 最长 Detect 洞 **大于整段 OCC 墙**，同时 FullReplay 把 `busy/8` 抬过 OCC |
| `19469101` 1.13 | `busy/8` 17.1 ms，OCC 7.27 ms。税是 FullReplay / unfenced 重执行 |

| 远离内界（FAR） | 界之上的 SpecFence 开销 |
|---|---|
| `3356896` 2.51 | `busy/8` 1.49 ms 已经贴近 OCC 1.70 ms。多出来的墙是调度/Resolve 壳（176 笔、345 次 pick） |
| `15274915` 2.68、`16146267` 1.85、`19860366` 1.76、`19505152` 1.88 | 洞和任务时间都解释不了整段墙。`19505152` / `19860366` 宽度约 51，pick 约 6 次/笔 |

对照块 `19933122`（以及板上另外两场 “赢”：`19934116`、`19426587`）`gas_used < 4_000_000`，两边都走顺序回退。脊柱计数全 0。那不是脊赢。

---

## 1. 方法

- **墙:** `specfence_3356896_compare`，Soft=0，Instant-off（一个 SpecFence `Pevm` 复用，OCC 每次新建），N=3，`SPECFENCE_COMPARE_CORES=8`。主墙 = 两次复用里较慢的一次（`sorted[len/2]`），与 49 块板同一规则。
- **宿主:** `nproc=4`，请求 8。SF/OCC 仍可比。绝对毫秒和 `busy/8` 含超订。串行毫秒是单线程，不超订。
- **PC:** `runnable_set_width_mean`、`steal_n`、`refuse_fill_n`、`sf_schedule_picks`、`skip_gate_n`、`pick_gate_n`、`ungated_occ_while_gated`、`gate_stall_ns`、`worker_busy_ns`、`idle_core_ns`。`idle_core_ns` 在这 10 块上是 0：空队列采样几乎不发生。`gate_stall_ns` 来自 `note_stall_end` 的**最大**洞等待，并被记进 `prepaid_ns` / `ordered_ns`，三者数值相同，不是三笔独立的税。
- **CC:** `visibility_*`、`resolve_*`、`detect_resolve_double_charge_n`、`reexec_entries`、`unfenced_reexec`、`occ_aborts`。
- **Learn:** `began_from_prior`、`explore_n`、`selected_arms`、`learn_e1_n`…`e6_n`、`mid_promote_n`、`prior_plant_n`。语义：E1 = Commit 观察，E2 = FullReplay，E3 = Partial，E4 = 一次 `refuse_fill` 立刻跑了独立交易，E5 = unfenced storm 上的 FullReplay，E6 = lazy 上 `force_opt`。E5 路径 `mark_under_covered`（sticky Opt，探索预算 0）并且**不**排队 IntraPatch；非 storm 的 FullReplay 才 `maybe_queue_promote`。
- **没加计数器。** `leftover_min` 没有单独的次数。`skip_gate`、`prior_plant_n`、`visibility_wait_released`、`gate_stall` 已经能回答：植物次数不是主税，主税是那条最长的洞。`15274915` 和 `14383540` 的复用行上 `reexec_entries` / `unfenced_reexec` / `incarnation_gt0` 读成 0，同时 `resolve_full_replay`、E5、`occ_aborts` 仍然很高。那三列在这两块的复用上不能当 “没有重执行”。

---

## 2. 总表（慢复用 vs OCC 中位）

`dig` 是本挖比值。`board` 是 49 块板上的比值。`self` = 墙 / max(洞, busy/8)。`SF/LB` 用 LB@8。

| block | 型 | n | L | indep | OCC | SF 冷 | SF 复用 | dig | board | 洞 ms | busy/8 | self | 类 | Full | E5 | picks | 宽 |
|------:|:-:|--:|--:|------:|----:|------:|--------:|----:|------:|------:|-------:|-----:|:--:|-----:|---:|------:|---:|
| 3356896 | E | 176 | 17 | 0.86 | 1.70 | 5.04 | 3.75 | 2.20 | 5.42 | 1.31 | 1.49 | 2.51 | FAR | 30 | 29 | 345 | 53 |
| 19638737 | E | 381 | 20 | 0.91 | 5.18 | 11.80 | 10.64 | 2.05 | 4.09 | 8.06 | 5.61 | 1.32 | NEAR | 27 | 26 | 914 | 93 |
| 8889776 | C | 330 | 56 | 0.31 | 2.99 | 12.58 | 13.09 | 4.38 | 3.50 | 11.27 | 10.56 | 1.16 | NEAR | 218 | 217 | 1232 | 122 |
| 16146267 | E | 473 | 50 | 0.76 | 4.54 | 18.12 | 12.47 | 2.75 | 3.27 | 6.31 | 6.75 | 1.85 | FAR | 116 | 112 | 1102 | 145 |
| 15274915 | B | 1226 | 77 | 0.90 | 5.39 | 25.77 | 21.26 | 3.95 | 3.23 | 4.67 | 7.92 | 2.68 | FAR | 160 | 155 | 2293 | 429 |
| 19469097 | C | 336 | 47 | 0.55 | 7.88 | 28.07 | 24.62 | 3.13 | 3.19 | 21.64 | 18.68 | 1.14 | NEAR | 178 | 175 | 1319 | 85 |
| 19860366 | C | 430 | 33 | 0.59 | 8.47 | 39.29 | 37.45 | 4.42 | 3.17 | 12.11 | 21.22 | 1.76 | FAR | 159 | 152 | 2581 | 52 |
| 19505152 | C | 417 | 30 | 0.59 | 8.34 | 39.69 | 33.90 | 4.07 | 2.84 | 14.18 | 18.03 | 1.88 | FAR | 141 | 140 | 2665 | 51 |
| 19469101 | C | 469 | 36 | 0.50 | 7.27 | 16.62 | 19.29 | 2.65 | 2.78 | 12.60 | 17.05 | 1.13 | NEAR | 205 | 205 | 1252 | 163 |
| 14383540 | B | 722 | 21 | 0.88 | 6.68 | 15.73 | 14.90 | 2.23 | 2.78 | 11.64 | 8.11 | 1.28 | NEAR | 65 | 62 | 1531 | 182 |

结构界（本机串行）：

| block | serial | crit | LB@8 | LB@4 | SF/LB8 | OCC/LB8 |
|------:|-------:|-----:|-----:|-----:|-------:|--------:|
| 3356896 | 0.55 | 0.05 | 0.07 | 0.14 | 54 | 25 |
| 19638737 | 6.20 | 0.33 | 0.78 | 1.55 | 14 | 6.7 |
| 8889776 | 2.02 | 0.34 | 0.34 | 0.51 | 38 | 8.7 |
| 16146267 | 6.06 | 0.64 | 0.76 | 1.52 | 16 | 6.0 |
| 15274915 | 4.00 | 0.25 | 0.50 | 1.00 | 42 | 11 |
| 19469097 | 8.25 | 1.15 | 1.15 | 2.06 | 21 | 6.8 |
| 19860366 | 11.27 | 0.86 | 1.41 | 2.82 | 27 | 6.0 |
| 19505152 | 12.03 | 0.87 | 1.50 | 3.01 | 23 | 5.5 |
| 19469101 | 11.51 | 0.88 | 1.44 | 2.88 | 13 | 5.0 |
| 14383540 | 8.19 | 0.24 | 1.02 | 2.05 | 15 | 6.5 |

`end_block_ns` 在慢复用上是 0.05–0.14 ms，解释不了任何一块的 SF−OCC。`detect_resolve_double_charge_n` 在全部复用行上是 0（冷启动 0–3）。`steal_n` 是 0–8。`prior_plant_n` 只有 `14383540` 的复用是 1，其余是 0。

---

## 3. C / B / E 的共性

### 3.1 CC：Resolve 几乎只有 Commit 和 FullReplay

十块慢复用上 Partial rebind 是 0，Partial rewind 是 0 或 1（`14383540`）。OrderedReplay 远小于 FullReplay。E5 与 E2 差 0–6，也就是几乎每一次 FullReplay 都走了 unfenced storm。

慢复用上 SF 路径的 `occ_aborts` 相对 OCC 自己的 abort：

| block | SF aborts | OCC aborts（三次） | FullReplay |
|------:|----------:|-------------------:|-----------:|
| 8889776 | 309 | 83–96 | 218 |
| 19469097 | 249 | 123–135 | 178 |
| 19469101 | 288 | 75–87 | 205 |
| 19860366 | 207 | 73–98 | 159 |
| 19505152 | 173 | 44–58 | 141 |
| 15274915 | 295 | 80–131 | 160 |
| 16146267 | 162 | 69–101 | 116 |
| 14383540 | 111 | 25–38 | 65 |
| 19638737 | 51 | 23–31 | 27 |
| 3356896 | 39 | 8–19 | 30 |

C 块的 FullReplay 是 141–218，和 `n` 同一量级。B 的 `15274915` 也有 160。E 的 `16146267` 是 116；`3356896` / `19638737` 的次数小，但相对它们很短的 OCC 墙仍然贵。

可见性已经是 Opt 为主：`visibility_opt` 数百到一千多，`visibility_ordered_tip` 是洞的起点次数（13–204），`visibility_wait_released` 是 8–41。再把更多边收成 OrderedAdmit，是在一条已经比 OCC 还长的洞上加预付。

### 3.2 PC：反链是宽的，输在洞和尾部

`runnable_set_width_mean` 从 51 到 429，都高于 8。`ungated_occ_while_gated` 几乎等于 `ungated_occ_n`：独立交易在 “还有人被闸” 的时候就已经启动了。`skip_gate_n` 只有 4–29，睡眠停车不是主项。

所以 `leftover_min` 的植物税不表现为 “全体停工”。它如果在场，是折进那条最长洞里的。洞占墙的比例，在 NEAR 的 C 块上是整段墙：`19469097` 洞 21.6 / 墙 24.6，`8889776` 11.3 / 13.1。

FAR 块的额外墙在洞和 `busy/8` 之上。`idle_core_ns=0` 说明工人没有在空队列上记账。请求 8、宿主 4，一部分差距是超订。计数上站得住的是 pick：`19505152` 2665 次 / 417 笔，`19860366` 2581 / 430，宽度却只有约 51。

### 3.3 Learn：E5 把中块补丁关掉了，复用粘在 Opt

冷启动：`began_from_prior=false`。除 `15274915`（冷就是 Opt）和 `3356896`（Win_1）外，主导臂是 Win_2。`explore_n` 是 0–3。`full_locs` 在冷启动上可以到 8–40。

复用：十块全部 `began_from_prior=true`，`explore_n=0`，主导臂 Opt。`arm_switch_n` 是 0–5。`mid_promote_n` 除 `14383540` 冷启动的 1 以外是 0。`mid_promote_veto_n` 是 0。

这和 E5 的代码路径一致：storm 把该位置收成 sticky Opt，探索预算写 0，IntraPatch 不入队。`apply_pending_patches` 在 pick 边界无事可做。E1 的大数只是 Commit 次数（和 `resolve_commit` 相同），不是中块有用的补丁。E6 是 0–5，lazy 的 `force_opt`，也不是中块宽度控制。

复用因此没有把 FullReplay 打下去。`19469101` 冷 176，慢复用 205，墙从 16.6 ms 升到 19.3 ms。`8889776` 慢复用 13.09 ms 高于冷 12.58 ms。`19860366` 两次复用是 23.4 ms 和 37.4 ms，慢的那次贴着冷启动 39.3 ms，快的那次并没有变成稳定赢。`3356896` 本挖的复用比冷启动快；板上 3.14→7.97 是样本，不是一条稳定的 “越学越差” 定律。粘性 Opt 解释的是 “学了也不降 FullReplay”，不是每一次复用都更慢。

---

## 4. 逐块

### 3356896 · E · n=176 · L=17 · 板上 5.42，本挖 2.20 · FAR

PC：宽 53，steal 1，refuse_fill 2，skip 4，pick_gate 21，pick 345。洞 1.31 ms，busy/8 1.49 ms，墙 3.75 ms。CPU 时间已经贴近 OCC 1.70 ms。多出来的约 2.2 ms 在任务计时和最长洞之外。

CC：Commit 203，OrderedReplay 9，FullReplay 30，partial 0，double charge 0。unfenced 17，reexec 41，SF abort 39，OCC abort 最多 19。vis opt/wait/tip = 223/8/13。

Learn：冷 Win_1，两处 Full。复用 Opt，`began_from_prior`，explore 0，E6=1，mid_promote 0。主冲突交易集合在三次里不变（31, 66, 67, …）。

结构界 0.07 ms，indep 0.86。短链、宽反链。SpecFence 独有开销是壳，不是一条盖不住的长脊。

### 19638737 · E · n=381 · L=20 · 板上 4.09，本挖 2.05 · NEAR

PC：宽 93，pick 914，skip 14，pick_gate 81。洞 **8.06 ms > OCC 5.18 ms**，墙 10.64 ms，self 1.32。独立集已经在闸旁跑（ungated while gated 440/441）。

CC：FullReplay 只有 27，但 unfenced 83、inc>0 84、SF abort 51。abort 列车比 FullReplay 次数宽。OrderedReplay 24，partial 0，double charge 0。

Learn：冷 Win_2，explore 1。复用 Opt，explore 0。E5=26。

indep 0.91。不必要的税是这条 8 ms 的洞，加上没有变成 Partial 的 abort。

### 8889776 · C · n=330 · L=56 · 板上 3.50，本挖 4.38 · NEAR

PC：宽 122，pick 1232，skip 29，pick_gate 151。洞 11.27 ms，busy/8 10.56 ms，墙 13.09 ms。洞和 CPU 都约是 OCC 2.99 ms 的 3–4 倍。

CC：FullReplay **218**，E5 **217**，reexec 466，unfenced 201，SF abort 309，OCC abort 约 90。Commit 591。partial 0，double charge 0。冷启动 `full_locs=22`，复用掉到 3，主导臂仍是 Opt，FullReplay 几乎没降（冷 233）。

Learn：冷 Win_2，explore 2。复用 explore 0，mid_promote 0。E1=591 只是 Commit 体积。

脊是真的（L=56，indep 0.31，单点最多 56 个写者）。洞比 OCC 整块还长，Resolve 仍整笔 FullReplay。

### 16146267 · E · n=473 · L=50 · 板上 3.27，本挖 2.75 · FAR

PC：宽 145，pick 1102，洞 6.31 ms，busy/8 6.75 ms，墙 12.47 ms，self 1.85。冷启动 18.1 ms 比慢复用还重，另一次复用 9.54 ms。尾部晃。

CC：FullReplay 116，E5 112，reexec 206，unfenced 99，SF abort 162，OCC abort 约 70–100。

Learn：冷 Win_2 + 11 个 Full。复用 Opt，explore 0。

L=50 的脊和 1.85× 的壳叠在一起。内界本身（6.7 ms）已经高于 OCC 4.54 ms。

### 15274915 · B · n=1226 · L=77 · 板上 3.23，本挖 3.95 · FAR

PC：宽 **429**，pick 2293，skip 10，pick_gate 188。洞只有 4.67 ms，busy/8 7.92 ms，墙 **21.26 ms**，self 2.68。反链极宽，makespan 仍是 busy/8 的 2.7 倍。另一次复用 14.7 ms，慢样本把比值从板上的 3.23 拉到 3.95。

CC：FullReplay 160，E5 155，SF abort 295，OCC abort 80–131。复用行的 `reexec_entries` 读 0，冷启动是 307；以 FullReplay 和 abort 为准。vis opt/wait/tip = 1497/41/204。double charge 0。

Learn：冷启动已经是 Opt（`full_locs=15`），explore 1。复用仍是 Opt，explore 0，prior_plant 0。

indep 0.90，同时 L=77。宽块里面有一条长链。壳（FAR）和 FullReplay（界内的税）都在。

### 19469097 · C · n=336 · L=47 · 板上 3.19，本挖 3.13 · NEAR

PC：宽 85，pick 1319。洞 **21.6 ms**，墙 24.6 ms，busy/8 18.7 ms。OCC 7.88 ms。洞单独就是 OCC 的 2.7 倍，self 1.14。

CC：FullReplay 178，E5 175，reexec 407，unfenced 178，SF abort 249，OCC abort 约 130。partial rewind 0，double charge 0。

Learn：冷 Win_2，explore 3，`full_locs=8`。复用 Opt，`arm_switch=0`，explore 0，FullReplay 仍 178。

和 `8889776` 同一形状：真脊、洞即墙、storm 把臂粘成 Opt。

### 19860366 · C · n=430 · L=33 · 板上 3.17，本挖 4.42 · FAR

PC：宽 **52**，pick **2581**（约 6 次/笔），steal 2，refuse_fill 15。洞 12.1 ms，busy/8 **21.2 ms**，墙 37.4 ms。CPU 已经是 OCC 8.47 ms 的 2.5 倍，墙上再乘 1.76。另一次复用只有 23.4 ms：慢样本的 `edge_optimistic_read` 是 4464，快样本是 1099。

CC：FullReplay 159，E5 152，reexec 335，unfenced 155，SF abort 207。冷 `full_locs=27`，复用 2。

Learn：冷 Win_2，explore 2。复用 Opt，explore 0，mid_promote 0。

L=33、多写者位置 116。内界里的税是重执行；界外是窄宽度上的反复 pick。

### 19505152 · C · n=417 · L=30 · 板上 2.84，本挖 4.07 · FAR

PC：宽 **51**，pick **2665**，steal 8（这十块里最高），洞 14.2 ms，busy/8 18.0 ms，墙 33.9 ms，self 1.88。OCC 8.34 ms。

CC：FullReplay 141，E5 140，reexec 254，unfenced 148，SF abort 173，OCC abort 44–58。冷 `edge_optimistic_read` 3252，慢复用 4309。

Learn：冷 Win_2，`full_locs=40`，explore 1。复用 Opt，explore 0，Full 位置掉到 2，FullReplay 仍 141。

和 `19860366` 一对：C 型里 “学完把 Full 标签收成 Opt，重执行还在，pick 爆炸”。

### 19469101 · C · n=469 · L=36 · 板上 2.78，本挖 2.65 · NEAR

PC：宽 163，pick 1252，pick_gate 173。busy/8 **17.1 ms** 是内界，洞 12.6 ms，墙 19.3 ms，self 1.13。OCC 7.27 ms。墙几乎就是这笔重执行 CPU。

CC：FullReplay **205**，E5 **205**，reexec 411，unfenced 195，SF abort 288，OCC abort 75–87。冷 FullReplay 只有 176。复用在变重。partial 0，double charge 0（冷是 3）。

Learn：冷 Win_2，explore 3，`full_locs=28`。慢复用 Opt，explore 0，`arm_switch` 在较快的那次复用是 5，FullReplay 仍升到 211/205。`began_from_prior` 没有把列车缩短。

这是 “冷→复用更差” 在本挖里最清楚的一块，机制是 sticky Opt + FullReplay 上升，不是 explore 被关掉之后的另一次测量噪声那么简单。比值仍只有 2.65，因为冷启动本身已经 16.6 ms。

### 14383540 · B · n=722 · L=21 · 板上 2.78，本挖 2.23 · NEAR

PC：宽 182，pick 1531，洞 **11.6 ms > OCC 6.68 ms**，busy/8 8.11 ms，墙 14.9 ms，self 1.28。另一次复用 10.1 ms。

CC：FullReplay 65，E5 62，SF abort 111，OCC abort 25–38。相对 C 块，FullReplay 温和。复用行 `reexec_entries` 读 0，冷启动是 147。vis opt/wait/tip = 1032/21/81。double charge 0。

Learn：冷 Win_2，explore 1，**mid_promote 1**（十块里唯一的中块提升）。复用 Opt，`prior_plant_n=1`（十块里唯一的非零植物），explore 0，E6=2。一次植物和一次提升没有把洞压到 OCC 以下。

indep 0.88，L=21。税主要是 11.6 ms 的洞。

---

## 5. 一个包里的杠杆（按证据，不分期）

下面五条是同一次落地要一起碰到的机制。顺序是证据覆盖面，不是施工阶段。

1. **Unfenced FullReplay 改成 Partial，不要整笔重放。** 十块里 Partial 合计可以忽略，E5≈E2，SF abort 是 OCC 的约 2–4 倍。C 块上这是 `busy/8` 超过 OCC 的主体（`19469101` 17 ms vs 7.3 ms，`19860366` 21 ms vs 8.5 ms，`8889776` 10.6 ms vs 3.0 ms）。结构界说明 OCC 自己也远，所以这一条追的是 SF−OCC，不是追到 `serial/8`。

2. **同一条 storm 不要 sticky Opt、不要跳过 IntraPatch。** 复用十块 explore=0、主导臂 Opt、`began_from_prior=true`，FullReplay 不降，`19469101` 还升。E1 的大数是 Commit 计数。`mid_promote` 除一个冷样本外是 0，因为 storm 根本不入队。中块 E1–E6 的图效果，在这些输家上等于 “把臂钉死”。

3. **把多写者链上的 Detect 洞压到不超过 OCC 的 abort makespan，同时保持反链已在闸旁启动的事实。** `ungated_occ_while_gated ≈ ungated_occ`，独立集没有被洞整块堵住。被堵住的是链本身：`19469097` 洞 21.6 ms（OCC 7.9），`8889776` 11.3（OCC 3.0），`14383540` 11.6（OCC 6.7），`19638737` 8.1（OCC 5.2）。`skip_gate` 很小，所以不是睡眠次数的问题。`visibility_wait_released` 和 `prior_plant_n` 也小，植物笔数不是这条洞的度量；洞的度量就是 `gate_stall`。

4. **砍掉窄宽度上的重复 pick，以及宽块上 busy 解释不了的尾部。** `19505152` 和 `19860366` 宽度约 51、pick 约 6 次/笔，self 1.8–1.9。`15274915` 宽度 429、墙仍是 busy/8 的 2.7 倍。`steal` 可以忽略，空闲计数是 0。宿主只有 4 核，绝对毫秒里有超订；pick 次数没有这个问题。

5. **短而高独立的块，CPU 已经贴近 OCC 之后剩下的是壳。** `3356896`：busy/8 1.49 vs OCC 1.70，墙 3.75，L=17，indep 0.86，pick 345 / 176。`end_block` 只有 0.09 ms。同一形状的轻度版是 `19638737` 的非洞部分。这条不要求把更多边收成有序。

明确不进这个包的测量结论：`detect_resolve_double_charge_n` 在复用上是 0；`prior_plant_n` 几乎是 0；Soft>0 和 `next_task*` 不在本脊上（本挖 picks 仍是 0）。可见性已经是 Opt 为主，加 OrderedAdmit 预付对不上洞已经长过 OCC 的事实。

---

## 6. 对照：19933122（以及板上另外两场赢）

| block | n | gas | 本挖 OCC | 本挖 SF 复用 | 比 | 脊柱计数 |
|------:|--:|----:|---------:|-------------:|--:|---|
| 19933122 | 45 | 2,056,821 | 0.512 | 0.488 | 0.95 | 全 0 |
| 19934116 | 58 | 3,365,857 | （板 0.92） | | | 未进本挖；gas 低于 4e6 |
| 19426587 | 37 | 2,633,933 | （板 0.97） | | | 同上 |

`Pevm::execute` 在 `gas_used < 4_000_000` 时两边都 `execute_revm_sequential`。`19933122` 的比较器输出里 OCC 与 SpecFence 的 abort、pick、Resolve、Learn 全是 0。0.95 和板上的 0.88 是顺序路径的启动差（SpecFence `Pevm` 复用，OCC 每次新建）。

上界工具为了画出 DAG 把这块的 gas 抬过 4e6 之后，有效图是 L=4、宽 38、indep 0.73，串行 0.58 ms。那是另一条路径。Instant-off 没有走它。49 块板上的 3 场赢都是这条顺序门，不能当作 Learn 或脊的胜场。
