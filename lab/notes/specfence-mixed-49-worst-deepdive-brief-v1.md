# SpecFence mixed-49 最差 10 块 — 简报

**日期:** 2026-09-22
**全文:** pevm `lab/notes/specfence-mixed-49-worst-deepdive-v1.md`（[#45](https://github.com/fengjy73/pevm/pull/45) `06fd9d9`，分析 only）
**板:** `lab/notes/specfence-mixed-49-pr45-soft0-tps-v1.md`（49 块，3/49，中位 2.15）
**镜像:** 本文件按 lab 惯例应对齐 [fengjy73/specfence-lab](https://github.com/fengjy73/specfence-lab)。本轮凭证访问该仓库得到 “repository not found”，简报只落在 pevm `lab/notes/`。

Soft=0。Instant-off N=3，请求 8 核，宿主 4 核。不改 CC / 调度，不恢复 `next_task*`。`occ_schedule_picks=0`。

## 判决

十个输家的本挖 SF/OCC（慢复用 / OCC 中位）是 **2.05–4.42**。板上 `3356896` 的 5.42 是一次 7.97 ms 样本；再跑复用 2.83 / 3.75 ms，快于冷启动 5.04 ms。

结构下界 `max(serial×L/n, serial/8)`：十块 SF 墙是该界的 **13–54×**（FAR）。OCC 同时是 **5–25×**。共有的是并行元开销。SpecFence 独有的是 SF/OCC 这一段。

引擎内界 `max(最长 Detect 洞, busy/8)`：

- **NEAR**（墙 / 内界 < 1.4，界本身已输给 OCC）：`8889776`、`19469097`、`19638737`、`14383540` 的洞长过整段 OCC；`19469101` 的 `busy/8` 是 FullReplay CPU（17 ms vs OCC 7.3 ms）。
- **FAR**（≥ 1.6，界之上还有 makespan）：`3356896`（busy/8 已贴近 OCC，多出来的是壳）、`15274915`、`16146267`、`19860366`、`19505152`。后两块宽度约 51、pick 约 6 次/笔。

## 三面

- **CC:** Partial ≈ 0。E5 ≈ E2，几乎每次 FullReplay 都是 unfenced storm。C 块 FullReplay 141–218。SF abort 约为 OCC 的 2–4 倍。double charge 复用上是 0。可见性已是 Opt 为主。
- **PC:** 可运行宽度 51–429，steal 可忽略，`idle_core_ns=0`。独立交易在仍有闸时就启动（`ungated_occ_while_gated ≈ ungated`）。`skip_gate` 4–29，`prior_plant` 几乎 0。主闸税是最长洞，不是植物笔数。`19469097` 洞 21.6 / 墙 24.6 ms。
- **Learn:** 复用十块 `began_from_prior`，`explore_n=0`，臂从 Win_2 粘成 Opt。storm 路径把位置收成 sticky Opt 并且不排队 IntraPatch，所以 `mid_promote` 基本是 0。E1 只是 Commit 次数。FullReplay 不降；`19469101` 冷 176 → 复用 205，墙 16.6 → 19.3 ms。

## 一个包（按证据，不分期）

1. Unfenced FullReplay 改 Partial，不要整笔重放。
2. 这条 storm 不要 sticky Opt，不要跳过中块 IntraPatch。
3. 多写者链的 Detect 洞不要长过 OCC 的 abort makespan；反链已经在闸旁跑。
4. 砍窄宽度上的重复 pick，以及 `15274915` 这种宽块里 busy 解释不了的尾部。
5. 短高独立块（`3356896`）在 CPU 已贴近 OCC 之后，剩的是 pick / Resolve 壳。

不进包：double charge、`prior_plant`、Soft>0、`next_task*`、再多收 OrderedAdmit。

## 对照赢

`19933122` gas 2.06e6 < 4e6，两边顺序回退，脊柱计数全 0，比 0.95（板 0.88）。板上另外两场赢 `19934116`（gas 3.37e6）和 `19426587`（gas 2.63e6）是同一扇门。3/49 不是脊或 Learn 的胜场。
