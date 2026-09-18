# 3356896：PR #25 后开销尸检（unfenced≈0 仍输 OCC）

**Block:** [3356896](https://etherscan.io/block/3356896)（n=176）  
**Tip:** PR #25 `cursor/specfence-tail-multihop-learn-4eba` @ `7bfbc76bdd57af9e88e8cf4f75a38f04efee57bd`  
**本盒:** 4 物理核；harness `SPECFENCE_COMPARE_CORES=8`（与 PR25 land 同口径，本机超订）  
**Soft=0**（全部 compare / PROFILE / reuse `soft_wait_arms=0`）  
**PRIMARY:** reuse SpecFence median wall ≤ OCC median（本盒 **false**）  
**用语:** OptimisticRead / OrderedAdmit（不用产品向 A0/A1）  
**规则:** 不发明 ns；未测桶标「未测」；worker 求和 Instant 桶标 **Instant-tax**（不可当墙时加法分解）。  
**Raw（本机，`lab/results/` gitignore）:** `lab/results/pr25-3356896-overhead/`  
（`compare_n7.{log,json}`、`compare_n9.{log,json}`、`compare_n7_profile.{log,json}`、`summary.json`）  
**参照:** land 附件 `specfence-pr25-tail-multihop-learn-land` · 设计 `specfence-3356896-tail-multihop-learn-v1` · PR20 尸检风格 `specfence-3356896-overhead-learn-pc-autopsy-pr20`  
**Harness:** `specfence_3356896_compare` interleaved OCC / SF；同一 `Pevm` reuse；`profile.release.lto=false`。

---

## 0. 悖论总述（局部 Win_3 赢、整块墙仍输）

**一句话:** PR25 把 Basic(0x32be) 脊从冷 Opt 的尾段 abort 列车收成 reuse **Win_3**，`unfenced_reexec` 落到 **0–1**；但 reuse 墙仍稳定在 **~1.09 ms**，OCC 暖机后 **~0.73–0.83 ms**。省下的 Resolve 重跑**没有**变成墙时；Win_3 预付的 3 hop + 两条短 FullChain 把调度打出 7 个 begin 洞，再叠加 OptimisticRead 壳（`validate_optimistic_fast` / commute 77 / `next_sf_task` 有闸路径 / 块末 ~50 µs），**整块仍慢于 OCC**。

| 视角 | 观测 | 含义 |
|------|------|------|
| **局部赢** | Instant-off reuse：`unfenced=0–1`、`main_inc_gt0=[]`、`wait_for_dependency=0`、`taxed_indep_blocked=∅`、commute/ignore **77/77**、`edge_4_31=true`、storage 基本干净 | 尾段 CC 到位；不再付 PR20/PR24 那种脊 abort 列车 |
| **全局输** | 本盒 N=7 主证：**OCC med 0.828 ms / SF reuse 1.088 ms ≈ 1.31×**；N=9：**0.734 / 1.102 ≈ 1.50×**；作者机 land：**0.946 / 1.091 ≈ 1.15×** | 预付串行 + SF 独有 meta > 省下的 unfenced |
| **结构** | 冷 Opt：`begin_blocked=[16,17,19,20]`（4）；reuse Win_3：`[16,17,19,20,31,66,67]`（7）。`ready_width_mean≈1.0`（袋深，不是 176） | 闸从「两条短链」扩成「短链 + 脊头 3 hop」；独立集仍走 SF 调度壳 |

**悖论机制（因果链）:**

1. 冷 begin：`should_seed_thin_ordered` 无 promoted → 脊 **Opt**；`admit_seed_hint_short_edges` 仍种两条 CallWaw 短 FullChain（14→16→17、15→19→20）。`unfenced=14`，`main_inc` 落在 67…171。  
2. 块末 F7：leftover 尾 + 实测 abort → **下一 begin 升 Win_3**（`take(3)` 对 4→31、31→66、66→67）。  
3. reuse：脊头 31/66/67 **不能**与 4 重叠执行；调度 `has_any_gated==true` → **整块**走 `next_task_with_wave_ready`（独立交易也付这条 pick，而不是 `next_occ_task`）。`wait_for_dependency` 计数为 0——预付体现为 **refuse/defer + `execution_idx` 跳洞**，不是 park Instant。  
4. 独立集「看起来像 OCC」：`occ_kernel_execs=176`、`refuse_ns` 产品路径未记、`maybe_wait` Instant=0；但 validate 走 `validate_optimistic_fast`（miss 后 `collect_invalid_reads` + commute），不是 `validate_occ_stage`。commute **77** 冷/热皆然（块结构，不是 Win_3 新造的）。  
5. 墙由 **预付关键路径 + SF 壳** 决定，不由「平均每 tx 更优 / unfenced=0」决定 → **局部优、整块劣**。

**和 PR20 的关键对照（不编造跨机对齐，只比结构）:**

| | PR20 本盒 N=7 | PR25 本盒 N=7 |
|---|---|---|
| 脊策略 | OptimisticRead 为主，`edge_oa=0` | reuse **Win_3**，`edge_oa=34` |
| unfenced | **8–17** | **0–1** |
| `reexec_ns` | 110–323 µs | reuse 多数 **0**，偶发 9–27 µs |
| 墙 gap | ≈0.281 ms（1.142−0.861） | ≈0.260 ms（1.088−0.828） |

unfenced 列车被砍掉后，**本盒 gap 几乎没缩**。因此剩余账本**不是**「还剩一点 abort」，而是当初与 abort **重叠**的 SF 壳 + 这次为灭列车而预付的宽度。PR20 的 `reexec_ns` 是 Instant/worker 记账，本来就不能加进墙。

---

## 1. 本盒实测墙时（主证据）

### 1.1 Interleaved Soft=0 · Instant **关**（PRIMARY）

| 跑 | OCC walls (ms) | OCC med | SF walls (ms) | SF cold | SF reuse med | SF/OCC | sf_le_occ |
|----|----------------|---------|---------------|---------|--------------|--------|-----------|
| **N=7 主证** | 3.085, 1.110, 0.878, 0.828, 0.822, 0.762, 0.733 | **0.828** | 1.219, 1.174, 1.076, 1.088, 1.137, 0.997, 1.012 | 1.219 | **1.088** | **≈1.31×** | **false** |
| **N=9 副证** | 1.640, 0.790, 0.905, 0.846, 0.687, 0.710, 0.734, 0.702, 0.710 | **0.734** | 1.350, 1.223, 1.102, 1.223, 1.025, 1.126, 1.000, 0.966, 1.084 | 1.350 | **1.102** | **≈1.50×** | **false** |
| 作者机 land N=7 | — | **0.946** | — | 1.228 | **1.091** | **≈1.15×** | false |
| 作者机 land N=9 | — | **0.875** | — | 1.515 | **1.265** | **≈1.45×** | false |

OCC[0] 冷启动污染（N=7 的 3.085）不作主证，但 **median 口径与 harness 一致**（含 iter0）。排除 OCC[0] 后 N=7 OCC 更低（~0.82），gap 更大，方向不变。

**暖机后半段（仍 false）:**

| | OCC 末三 | SF 末三 | 末 iter 差 |
|---|---|---|---|
| N=7 | 0.822 / 0.762 / 0.733 | 1.137 / 0.997 / 1.012 | 1.012−0.733 = **0.279 ms** |
| N=9 | 0.734 / 0.702 / 0.710 | 1.000 / 0.966 / 1.084 | 1.084−0.710 = **0.374 ms** |

OCC 随 iter 继续掉；SF reuse 钉在 **~1.00–1.22 ms**。这不是「一次噪音」，是 SF **地板**。

**本盒 vs 作者机:** 本盒 OCC 更快（4 核超订 8 的绝对值不可跨机对齐）；SF reuse **~1.09** 与 land N=7 的 1.091 同量级。PRIMARY 在两台机器上都是 false。

### 1.2 Instant **开**（`SPECFENCE_PROFILE=1`）——只作 Instant-tax，不作 PRIMARY

N=7 PROFILE：OCC med **0.869** / SF reuse **1.108**（墙被 Instant 污染，**不**用来验收 PRIMARY）。

更严重：PROFILE 把 `idle_core_ns` / `refuse_ns` 写进 `prepaid_ns`，learn 以为 Win_3 预付输了：

| iter | learn | begin_blocked | unfenced | prepaid_ns | abort_cf_ns | prior_decay |
|------|-------|---------------|----------|------------|-------------|-------------|
| 0 | Opt | 4 | 13 | 725415 | 210381 | 0 |
| 1 | Win_3 | 7 | 0 | 107705 | 0 | **1** |
| 2 | **Win_1** | 5 | **14** | 114661 | 236113 | 0 |
| 3–6 | **Win_2** | 6 | 0–1 | 1–8k | 0–27k | 0 |

→ PROFILE 会 **改策略**。Instant-off 主证全程 reuse **Win_3**。下文 Instant 桶只比较同 iter 的 worker 求和，不解释墙。

### 1.3 Serial / 完整 schedule dump

- **Serial 墙:** 未测（本仓库无 PR20 那种 `schedule_dump` 串行档；`cores=1` 仍是并行调度器，不能冒充 serial）。  
- **Schedule dump 工具:** 未测（无 in-repo dump 二进制）。  
- **D1 MV 快照:** 已从 `last_location_writers` 抽出（§4），不是调度时间线。

### 1.4 Soft=0 确认

主证 / 副证 / PROFILE：`soft=0`、`soft_wait_arms=0`。

---

## 2. 开销桶表（必要 vs 不必要）

对照 N=7 主证 gap ≈ **0.260 ms**（1.088−0.828）。**没有**把 Instant-tax 加总成这 0.260。

| 桶 | 量级（测到） | 归类 | 说明 |
|----|--------------|------|------|
| **脊 WAW unfenced reexec** | Instant-off reuse：`unfenced` **0–1**；`reexec_ns` 多数 **0**，N=7 偶发 **9013 / 26876 ns**；`main_inc_gt0=[]`；off_edge 偶发 `[9]` | **相对 PR20：已不是主账本**。相对 OCC：偶发 1 次 abort 解释不了 0.26 ms | 局部赢；**不解释**全局输 |
| **OrderedAdmit 预付宽度** | 冷 4 洞 / Win_3 **7 洞**（§4）；`edge_oa` 10→**34**；`refuse_admit` 0–33；`wait_for_dependency=0` | **Detect 预付**。产品路径 `refuse_ns`/`prepaid_ns`/`idle_ns` 全 0——计数器挂在 `SPECFENCE_PROFILE` Instant 上，**不是真零成本** | **全局输主嫌疑（墙结构）** |
| **短链 FullChain（与 Win_w 无关）** | 冷就已种 14→16→17 与 15→19→20 | 设计 T4 / `ORDER_WINDOW_K=2`；即使脊 Opt 也付 4 洞 | 必要（storage）+ 第二条合约短链（§4.2） |
| **commute / ignore** | **77 / 77** 冷热稳定；`batch_repair` 冷 1、reuse 0 | 吸收 lazy/可交换；**不解释** SF>OCC（PR20 同判） | 局部赢信号；但是 **validate miss 路径税**（§5） |
| **end_block** | Instant-off **47–72 µs**（N=7 reuse 53–60 µs；N=9 47–63 µs）。**单时钟**，可与墙比量级 | SF 独有 HotSet/prior/D1/learn | 约占 gap 的 ~1/4 **上限**（若完全暴露在关键路径）；单独不够 |
| **admit_seed Instant** | Instant-off **0**（代码：`profile_timing_enabled()` 才打点）；PROFILE **10–14 µs** | **Instant-tax / 未测产品 ns** | 小 |
| **optimistic_path_tax_ns** | Instant-off = `end_block`（admit_seed=0、refuse_ns=0）≈ **47–72 µs** | 定义= seed+end+refuse，**不是**独立第三桶 | 勿重复加 |
| **maybe_wait** | PROFILE **0** | 无 Await Instant | 与 `wait_for_dependency=0` 一致 |
| **handler Instant-tax** | PROFILE 暖 SF ≈ 0.32–0.51 ms worker 和；OCC ≈ 0.37–0.50 ms | **Instant-tax**；两边同量级 | 不解释 gap |
| **validate Instant-tax** | PROFILE 暖 SF 0.17–1.14 ms；OCC 0.21–0.71 ms（OCC[2] 2.87 ms 离群） | **Instant-tax**；噪声大 | 证明两边都做读集走；**不能**拆墙 |
| **sched Instant-tax** | PROFILE 暖 Win_3/Win_2：SF **185–284 µs**；OCC **116–166 µs**（同跑 iter 1/3–6） | **Instant-tax**；SF pick 更重 | 与 `has_any_gated` 后走 wave/refuse 一致 |
| **ReadyEdge / ready-bag** | `ready_width_mean≈1.00–1.12`（有闸时采样袋深）；A1=0 宽采样 176 **已不出现** | 袋几乎空；税在 **pick 分支** 不在袋深度 | 勿再误读 176 |
| Soft / idle（产品） | Instant-off idle=0 | idle Instant 只在 PROFILE 记 | 产品路径未测 idle ns |
| 独立集 begin 税 | `taxed_indep_blocked=∅` | 无 PR15 那种 6…13 / 76…86 闸 | OK |

**必要 vs 不必要（本块，PR25 后）:**

- **必要（结构真相）:** 主链 WAW（4 写 Basic(0x32be)；31/66/67 经 0x209c 再写同一 ℓ）与 storage 14→16→17 必须有人付——要么短 OrderedAdmit，要么 OCC abort。PR25 选 **Win_3 预付头 3 hop**，灭了列车。  
- **不必要 / 可铲（相对 OCC 墙）:**（1）`has_any_gated` 一旦为真，**整块 pick** 离开 `next_occ_task`；（2）`validate_optimistic_fast` 在 77 次 miss 上 `collect+commute+rebind`，OCC 同块只 `occ_aborts=2–25`；（3）产品路径不记 refuse/width Instant，learn 的 `prepaid_ns=0` 与真实预付墙脱节；（4）块末 D1/learn 每 iter ~50 µs；（5）Win_3 是否必须第三 hop（66→67）——Instant-off **未测** Win_2 产品墙（只在 PROFILE 污染下见过 Win_2 且 unfenced 仍 0–1）。

---

## 3. 学习 / 遥测可信度

### 3.1 Instant-off 主证（可信的策略迹）

| 源 | chosen_strategy | win_w | win1/2/3/seg/full | ev_keep / ev_demote | prior_decay | c_ord / c_opt |
|----|-----------------|-------|-------------------|---------------------|-------------|---------------|
| N=7 SF[0] | Opt | 0 | 0/0/0/0/0 | 6 / 0 | 0 | 8000 / 60257 |
| N=7 SF[1..4] | **Win_3** | 3 | 0/0/1/0/**2** | 14 / 0 | 0 | 8000 / 60k→45k |
| N=7 SF[5..6] | **Win_3** | 3 | 0/0/1/0/**3** | 14 / 0 | 0 | 8000 / 45382 |
| N=9 SF[0] | Opt | 0 | 0/0/0/0/0 | 6 / 0 | 0 | 8000 / 67259 |
| N=9 SF[1..8] | **Win_3** | 3 | 0/0/1/0/2 | 14 / 0 | 0 | 8000 / 67k→51k |

F7 冷 Opt → 首 reuse **Win_3**，之后钉死。`seg_locs=0`（T2 非默认，与 land「撤回首 reuse 全 Seg」一致）。`full_locs=2`（两条短链）或 3（N=7 后段多一个 promoted 短 ℓ）。

`mean_c_ordered` 钉在冷 prior **8000**（`PRIOR_C_ORDERED_NS`）：产品路径 `refuse_ns=0` → 块末不 ema 更新 ĉ_ord。`prepaid_ns=0`、`ordered_ns=0` 同因——**不是**「预付真的免费」，是 **仪表关了**。  
`c_opt` 只在有 `reexec_ns` 的 iter 下降。多数 reuse `abort_cf=0` → EV 看不到「Win_3 预付 vs OCC abort」的墙时差。

### 3.2 闭环指标（F6，禁止 `occ_aborts` 主导）

| 指标 | Instant-off reuse | 可信？ |
|------|-------------------|--------|
| `incarnation_gt0` | 0–2 | 是 |
| `unfenced_reexec` | 0–1 | 是 |
| `reexec_ns` | 0 或 9–30 µs | 是（执行计时） |
| `ordered_ns` | **0** | **半可信**：refuse Instant 关 |
| `refuse_admit` | 0–33 | 是（次数） |
| `refuse_ns` | **0** | **不可当 0 成本** |
| `wait_for_dependency` | 0 | 是（没走 park 动词） |
| `occ_aborts`（SF） | reuse 0（N=7[4] 为 1） | 勿用来学 |
| OCC `occ_aborts` | 2–25 | OCC 侧有 abort；SF reuse 几乎无 —— **口径不可比**（OCC 无 commute 计数） |
| `optimistic_read_occ_fast` | **0** | 该辅助函数本块未进（lean 读路径不走 `occ_optimistic_read`）；**不能**读成「执行不是 OccKernel」（`occ_kernel_execs=176`） |

### 3.3 PROFILE 学习不可信

Idle Instant 进入 `prepaid_ns` → Win_3 被判输 → `prior_decay=1` → 下一 iter **Win_1 列车回归**。这是仪表污染，不是产品 F7。

---

## 4. OrderedAdmit 预付宽度（Basic(0x32be) + 短链）

### 4.1 begin 种了谁（`last_begin_blocked`，admit_seed 之后）

代码：`Win_w` = `select_pairs_for_strategy` 对已存连续 D1 pair **take(w)**；短链 `n_pairs≤2` 且未 demote → **FullChain**。

| 策略 | 本盒证据 | begin_blocked | 脊 hops | 短链 hops |
|------|----------|---------------|---------|-----------|
| **Opt（冷）** | Instant-off / PROFILE iter0 | `[16, 17, 19, 20]`（**4**） | **0** | 2+2 |
| **Win_1** | 仅 PROFILE iter2（仪表污染） | `[16, 17, 19, 20, 31]`（5） | 1（4→31） | 2+2 |
| **Win_2** | 仅 PROFILE iter3–6 | `[16, 17, 19, 20, 31, 66]`（6） | 2 | 2+2 |
| **Win_3** | Instant-off 全部 reuse | `[16, 17, 19, 20, 31, 66, 67]`（**7**） | **3**（4→31→66→67） | 2+2 |

相对 Opt，Win_3 在脊上多种 **3 条 ReadyEdge / 3 个 wait-for 洞**。  
`wait_for_dependency=0`：这些洞走 `is_gated && !may_execute` → `execution_idx.fetch_max` + `ReadyEdgeTable::defer`（`refuse_admit++`，mutex），**不是** `add_wait_for_dependency` park。

### 4.2 洞对应的真实交易

| tx | from / to | 角色 |
|----|-----------|------|
| **4** | `0x32be343b…` → `0x18183116…`（空 input） | 脊头，写 Basic(0x32be) |
| **31, 66, 67** | 空 input → **`0x209c4784…`** | 经合约再写同一 Basic(0x32be)（T4 禁止的是 **宽 0x209c 星**，不是这条隐式 WAW 脊） |
| **14, 16, 17** | calldata 68B → `0xedbaf3c5…` | storage CallWaw 短 FullChain（T4 保留） |
| **15, 19, 20** | calldata 68B → `0xe94b04a0…` | **第二条** 3-tx CallWaw，冷 begin 就种（`hint_short_edges` 的 `3..=4`） |

D1 快照（N=7 SF[6]）长脊 loc `16138400061442938452` =  
`[4, 31, 66, 67, 69, 70, 93, 96, 103, 115, 131, 132, 135, 138, 141, 166, 171]`（17 writer，`edge_4_31=true`）。  
Win_3 **只预付头 3 hop**；69…171 仍 OptimisticRead。reuse `main_inc_gt0=[]` → 尾段不再 abort 列车（局部赢）。

### 4.3 预付如何变成墙（不是 park ns）

`next_sf_task`：`!has_any_gated && !stages.has_reserved` 才 `scheduler.next_task()`（OCC pick）。  
本块 **冷就开始** `has_any_gated`（4 个短链洞）→ **从未**走 OCC pick。Win_3 只是把闸从 4 加到 7，并把 31/66/67 从「可与 4 重叠」改成「必须等 pred Publish」。

31 等 4、66 等 31、67 等 66：这是 **makespan 上的串行前缀**。OCC 让它们重叠，WAW 用 abort 收。本块这些 tx 很小（31/66/67 gas=40000，空 input）；OCC abort 便宜。Win_3 把便宜的 Resolve 换成关键路径上的 Detect 等待。

---

## 5. OptimisticRead 路径 vs OCC（独立交易 meta）

产品路径（`pevm.rs`）：

| 步 | OCC | SF 未闸（独立 / 多数 tx） | SF 已闸（begin_blocked） |
|----|-----|---------------------------|--------------------------|
| pick | `next_occ_task` | **`next_sf_task` → `next_task_with_wave_ready`**（因为块内已有闸） | 同上；命中洞则 refuse/skip |
| execute | `try_execute` | 同 `try_execute` + `note_started` + **每笔** `note_producer_done_stamp` | `try_execute` + fence/wave |
| validate | `validate_occ_stage`（bool walk，miss→abort） | **`validate_optimistic_fast`**：先同一 bool walk；miss→`collect_invalid_reads`+`commute_ok`+rebind | `validate_specfence` |
| 块末 | 无 learn/D1 | HotSet / prior / D1 / F7（**47–72 µs**） | 同左 |

测到的独立集信号：

- `occ_kernel_execs=176`（干净 reuse）= n_tx → 执行核是 OccKernel。  
- `optimistic_read_occ_fast=0` → `occ_optimistic_read()` 本块未调用（lean 读不进那条辅助函数）。**不要**写成「执行不是 OCC」。  
- `occ_kernel_validates` reuse 几乎 0（N=7[4] 为 1）→ validate **不是** `validate_occ_kernel` 计数路径，是 `validate_optimistic_fast`。  
- commute **77** 冷热相同：`commute_ok` 只对 value-transfer + lazy Basic miss。本块 166 笔空 input、122 笔 gas=21000，lazy 冲突是结构。OCC 同块 `occ_aborts` 仅 2–25——**要么** OCC 更少 miss（调度顺序不同），**要么** miss 直接 abort 且计数口径不同。未做 per-tx validate-miss 对拍 → **未测**「77 次 collect 的墙 ns」。  
- 独立集 **没有** begin 税（`taxed_begin=0`）。

**Meta 税结论:** 独立交易不是「字节级 OCC」。最硬的测到差异是：（1）整块 pick 已是 wave/refuse；（2）validate miss 走 commute；（3）每笔 done_stamp；（4）块末 ~50 µs。其中（1）（2）无产品路径墙 ns（未测拆分）；（4）有单时钟 ns。

---

## 6. 分析问题（直接作答）

### Q1. unfenced≈0 之后，~0.15–0.25 ms（作者机）/ 本盒 ~0.26–0.37 ms 还在哪？

**测到、可与墙比的：** `end_block` **47–72 µs**。其余 **没有** 墙时钟拆分。  
**结构上必须记账、但 ns 未测（产品路径）：** Win_3+短链的 7 洞串行与 refuse/skip；`next_task_with_wave_ready` 相对 `next_occ_task`；`validate_optimistic_fast`+77 commute。  
**已排除作为主账本的：** unfenced 列车（0–1）；`maybe_wait` Instant（0）；独立集 begin 税（∅）；Soft；ready-bag 深度（~1）。  
**噪声：** OCC median 随 N/暖机从 0.83 掉到 0.73，会放大倍数；SF reuse 地板稳定，**方向不是噪声**。

作者机 land gap ~0.15 ms 与本盒 0.26 ms 差一截：本盒 OCC 更低。SF reuse 两台都 ~1.09（land N=7）。

### Q2. 主要是 (a) 预付 / (b) OptimisticRead meta / (c) end_block / (d) 调度袋 / (e) 噪声？

**排序（证据，不是精确 ms）:**

1. **(a) + (d) 绑在一起** —— 有闸 ⇒ pick 换函数 + 脊头 3 hop 上关键路径。`ready-bag` 本身不是（宽度≈1）；(d) 应读成 **scheduler/ready-edge 控制流**，不是 min-heap 深度。  
2. **(b)** —— commute 77 + 非 OCC validate；Instant-tax validate 噪声大，**未测**墙贡献。  
3. **(c)** —— 测到 ~50 µs，必要嫌疑，单独不够。  
4. **(e)** —— 解释不了 SF 地板，只解释 OCC median 漂移。

不能诚实地说「0.26 ms = a 的 x + b 的 y」。那是发明 ns。

### Q3. 具体优化点（方向 / 风险）——分析排序，不是要落地的分期代码

| 序 | 点 | 预期方向 | 风险（seq≡par / 回归） | 依据 |
|----|----|----------|------------------------|------|
| 1 | **有闸时仍让未闸 tx 走 `next_occ_task` 热路径**（wave/refuse 仅 `is_gated`） | 砍墙 + 砍 sched Instant-tax | 漏 refuse / 洞上的 idx 自旋；ERC-20 已有 mid-plant 翻车史 | 冷 4 闸已迫使整块离开 OCC pick |
| 2 | **产品路径给预付打墙时钟**（begin 洞上的 stall，不进 Instant-tax ĉ） | 不直接砍墙；让 F7 看见 Win_3 vs OCC abort 的**墙** | 若把 Instant idle 再写进 prepaid，会重演 PROFILE：Win_3→Win_1 列车 | `prepaid_ns=0` 而墙仍 1.09 |
| 3 | **验证 Win_2 是否已够灭列车**（Instant-off，不要 PROFILE） | 少 1 hop（67）；可能微砍关键路径 | 67 与 66 竞写 → unfenced 回升。PROFILE Win_2 的 0–1 **不能**当产品证据 | Instant-off 从未选 Win_2 |
| 4 | **`validate_optimistic_fast` 成功路径 ≡ `validate_occ_stage`；commute 只留 lazy 快路径** | 砍 validate Instant-tax；墙方向不确定 | 错误 rebind → seq≠par；关掉 commute 则 77 变 abort | 77 稳定；OCC abort 2–25 |
| 5 | **块末 D1/HotSet/learn 再削**（`edge_4_31` 已真则少走 MV merge） | 砍 ~数十 µs 墙 | 学丢短边 / 下一 begin 不种 storage | `end_block` 47–72 µs 已测 |
| 6 | **第二条短链 15→19→20 是否必须 begin FullChain** | 冷少 2 洞；可能让更多 iter 贴近 OCC pick（若只剩 storage 仍有闸，收益小） | 漏真实 CallWaw → 那条合约 abort | D1 确认 [15,19,20] 是真 writer 集 |
| 7 | **不要**恢复 16-writer FullChain / mid-execute ReadyEdge / 热路径 T3 flush | — | PR22 ~1.40 ms；ERC-20 / iter11 | land 已撤回 |
| 8 | **不要**用 `ready_width` 或 `occ_aborts` 做奖励 | — | F6 已禁 | `ready_width≈1`；OCC abort 口径不可比 |

### Q4. 悖论：局部 Win_3 赢、全局墙输 —— 机制

Win_3 的「赢」是 **CC 计数**：头 3 hop 盖住 4→31→66→67，尾段不再 `main_inc>0`，`unfenced≈0`。  
墙的「输」是 **makespan**：那 3 hop 把本来可重叠的小 tx 串起来；4 个短链洞让 **176 笔的 pick** 都走 SF 调度；77 笔 validate miss 走 commute；每块再付 ~50 µs 尾。OCC 用 2–25 次瘦 abort 换重叠。本块 abort 便宜、预付贵 → **局部计数赢、全局时钟输**。

这与「Win_3 学错了」不同：F7 按 leftover/reexec **正确**升到 Win_3（Instant-off 稳定）。错的是 **奖励看不到预付墙**（`prepaid_ns=0`），所以没有人把 Win_3 和 OCC 墙对齐。

---

## 7. N=7 主证逐 iter（Instant-off）

| i | OCC ms | SF ms | learn | unf | reexec_ns | refuse | begin_n | main_inc | end_ns | commute |
|---|--------|-------|-------|-----|-----------|--------|---------|----------|--------|---------|
| 0 | 3.085 | 1.219 | Opt | 14 | 201285 | 2 | 4 | 67…171（14） | 59021 | 77 |
| 1 | 1.110 | 1.174 | Win_3 | 0 | 0 | 12 | 7 | [] | 58258 | 77 |
| 2 | 0.878 | 1.076 | Win_3 | 0 | 0 | 22 | 7 | [] | 52897 | 77 |
| 3 | 0.828 | 1.088 | Win_3 | 1 | 9013 | 0 | 7 | []（off=9） | 55888 | 77 |
| 4 | 0.822 | 1.137 | Win_3 | 0 | 26876 | 4 | 7 | [] | 54945 | 77 |
| 5 | 0.762 | 0.997 | Win_3 | 0 | 0 | 4 | 7 | [] | 59690 | 77 |
| 6 | 0.733 | 1.012 | Win_3 | 0 | 0 | 33 | 7 | [] | 57157 | 77 |

N=9 reuse 同构：Win_3、7 洞、unfenced 0–1、end 47–63 µs、commute 77。末 iter OCC 0.710 / SF 1.084。

---

## 8. Instant-tax 摘录（PROFILE N=7，worker 求和）

**不可加总成墙。** 只比「SF 是否系统性重于 OCC」。

暖机且策略仍是 Win_*、unfenced≤1 的 iter（避开 OCC[2] 离群与 SF[2] Win_1 列车）：

| iter | OCC h / v / s (ns) | SF learn | SF h / v / s (ns) | SF−OCC s |
|------|--------------------|----------|-------------------|----------|
| 1 | 415078 / 606440 / 165651 | Win_3 | 321911 / 622191 / 284489 | +119k |
| 3 | 419313 / 210849 / 116074 | Win_2 | 512647 / 1141398 / 260173 | +144k |
| 4 | 497639 / 706540 / 155029 | Win_2 | 316522 / 172906 / 202680 | +48k |
| 5 | 373752 / 559497 / 131020 | Win_2 | 361725 / 170397 / 185394 | +54k |
| 6 | 409975 / 517760 / 142183 | Win_2 | 486208 / 199268 / 244674 | +102k |

- `maybe_wait=0`（两边）。  
- handler 无稳定 SF>OCC。  
- sched Instant-tax **SF  consistently > OCC**（+48–144 µs worker 和）。  
- validate 无稳定方向。  
- `admit_seed_ns` 10–14 µs；`end_block` 49–67 µs（与 Instant-off 同量级）。

---

## 9. 底线

- **悖论:** 局部 Win_3（无 abort 列车）优于 PR20/PR24；整块因 **预付 7 洞 + 整块 SF pick + OptimisticRead validate/commute + ~50 µs 块末** 仍慢于 OCC。  
- **PRIMARY** 本盒 N=7/N=9 皆 false；SF reuse 地板 ~1.09 ms（与 land N=7 一致）。  
- **剩余 gap 不是 unfenced。** PR20 的 Resolve 账本已基本铲掉，墙 gap 还在。  
- **学习:** Instant-off 稳定 Win_3，但 `prepaid_ns` 看不到预付墙，F7 不会为 PRIMARY 降 hop。PROFILE Instant 会 **教错**。  
- **Soft=0** 已确认。  
- **不要做:** 全脊 FullChain、mid-execute 种边、热路径 T3 flush、把 Instant idle 写进 ĉ。  
- **下一刀（分析）:** 未闸 pick ≡ OCC；给预付打**墙**时钟再决定 Win_2 vs Win_3；削 commute/validate 壳。本 PR **不改** CC/policy/learn。
