# PR #40 TPS 输家：理论最优排列 vs 不必要开销

**基线:** PR #40 `cursor/specfence-midband-spine-tps-041c` @ `a7562676301eb173ef7a3692f89cd3b3e2cdbb75`  
**本分支:** `cursor/specfence-tps-losers-optimal-overhead-ff75`  
**性质:** 分析；不改 SpecFence CC / policy / learn  
**Soft=0 · Instant-off 主墙 · Instant-tax（`yield_ns` / `worker_busy_ns` / idle）不计入墙、不发明 ns**  
**索引:** [`specfence-pr40-tps-losers-analysis-index.md`](specfence-pr40-tps-losers-analysis-index.md)  
**摘要:** [`specfence-pr40-tps-losers-optimal-vs-overhead-summary.json`](specfence-pr40-tps-losers-optimal-vs-overhead-summary.json)  
**逐块附录:** [`specfence-pr40-tps-losers-optimal-vs-overhead-appendix.md`](specfence-pr40-tps-losers-optimal-vs-overhead-appendix.md)

用户问：这些块还有什么问题；若还没接近理论最优排列 — 为什么；若已接近 — 为什么还有大量不必要开销。

---

## 0. 判决（先给答案）

K=11 Instant-off Soft=0 后，**没有**「整簇已经贴上理论最优、只剩一点壳」这回事。输家裂成两类，对应两种完全不同的下一刀：

| 簇 | 块 | 相对等权 list-schedule | 墙还输 OCC 的主因 |
|----|----|------------------------|-------------------|
| **A · 近独立 / 薄脊** | 14396881, 13217637, 19638737, 3356896 | **NEAR**（有效 DAG 不必深有序；独立工作已大量 ungated） | SF 调度/validate/壳 ≫ OCC 的廉价 abort；Detect 仍留 4–8 个 wait-set 洞 |
| **B · 真脊盖不住** | 19807137, 16146267, 8889776, 19716145, 19860366, 19469101 | **FAR**（关键路径 L=33–571，wait-set 软顶=8 + sticky Opt / 空 Full，cover_window=0） | 排列没走到 L 波；未盖的脊付 OCC 式 abort 列车，盖了的 8 个洞再付 OrderedAdmit 预付 |
| **C · 错对象 Full** | 15274915 | **FAR** | `Full/996` 打在 basic_lazy 千写者上（lazy 不是 OrderedAdmit 对象），同时 77 笔 Basic 脊只种 1 个洞 |

**一句话:** PR #40 把 lazy 4–27× 尾和中档过预付压下来了，但 **没有** 把中档真脊排到 `max(L, ⌈n/8⌉)` 波。23/98 冻在「NEAR 块的壳 + FAR 块的欠盖」上：前者再调臂也翻不了 TPS 计数，后者在 `cover_proven_cheaper` 几乎永不成立时会永远 sticky Opt。

对「理论最优」的诚实分层（与 PR19 3356896 同一尺子）：

1. **结构层** — 有效冲突 DAG（beneficiary + `basic_lazy` 排除）的 list-schedule：临界路径全序、每波 ≤1 条脊节点、反链填满 8 槽。  
2. **墙时层** — 单位成本界 `t_work / bound@8`。本箱除 19807137 外，OCC 已是 6–28× 该界；「打到 8×」对本箱多数块 **不是** 可操作目标。

NEAR 只承诺（1）。（2）远大于 1 且两边都远，是并行元开销，不是排错波。

---

## 1. 方法

- **Instant-off 主墙:** `specfence_3356896_compare` 交错 OCC/SF，N=5，SF 复用，`SPECFENCE_COMPARE_CORES=8`。主墙 = SF reuse median（iter 1..4）vs OCC median。Soft=0 全 iter `soft_wait_arms=0`。  
- **DAG:** `analyze_dag` 最终 RW，RAW+WAW；有效图排除 beneficiary / `basic_lazy`。`L=longest_chain`，`W=max_wave_width`，`bound@8 = min(8, n/L, W)`，等权波数 `max(L, ⌈n/8⌉)`。  
- **串行帽:** upper-bound sequential。仅 19807137 serial 2412 ms ≫ OCC@1 16.8 ms，标病态，`t_work` 改 OCC@1。  
- **NEAR/FAR:** 看 wait-set 是否 ⊆ 必要后继、独立集是否过闸、臂是否对准真对象、cover_window 是否吸收 L。墙 ≫ 单位界 **单独** 不构成 FAR。  
- **不计入墙:** `yield_ns` / `worker_busy_ns` / `idle_core_ns`（Instant-tax；本轮 Instant-off 下 idle/busy 为 0）。`reexec_ns`/`prepaid_ns` 只作信号，不加总进墙。  
- **宿主:** `nproc=4`，请求 8 核（与 PR40 扫块一致）。比值在交错下仍可比；绝对 ms 有超订。  
- **原始:** `lab/results/pr40-k11-optimal-overhead/`（gitignore）。

K=11：最差 TPS（14396881, 15274915, 13217637）+ 中档真脊（16146267, 8889776, 19638737, 19716145, 19860366）+ 欠盖脊 19807137 + Opt 残留 19469101 + 薄块 3356896。

---

## 2. Instant-off 总表（Soft=0, N=5, @8）

| block | n | L | W | bound@8 | 波 | OCC med | SF reuse | SF/OCC | 判决 | 臂轨迹 | wait | cover | ungated末 | end µs | unf | 形态 |
|------:|--:|--:|--:|--------:|--:|--------:|---------:|-------:|------|--------|-----:|------:|----------:|-------:|----:|------|
| 14396881 | 1346 | 5 | 1337 | 8.00 | 169 | 4.536 | 15.154 | **3.34** | **NEAR** | Opt→Full×4 | 4 | 0 | 768 | 244 | 4 | 近独立 + lazy 1197 |
| 15274915 | 1226 | 77 | 1121 | 8.00 | 154 | 5.349 | 15.647 | **2.93** | **FAR** | Full×5 | 1 | 0 | 113 | 429 | 59 | lazy Full + Basic 77 |
| 13217637 | 1100 | 6 | 1060 | 8.00 | 138 | 5.615 | 14.841 | **2.64** | **NEAR** | Opt×5 | 8 | 0 | 1102 | 336 | 12 | 近独立 + lazy 934 |
| 16146267 | 473 | 50 | 375 | 8.00 | 60 | 4.358 | 11.781 | **2.70** | **FAR** | Opt×5 | 8 | 0 | 102 | 211 | 59 | storage 50 |
| 19807137 | 712 | 571 | 106 | **1.25** | 571 | 18.015 | 44.977 | **2.50** | **FAR** | Opt×5 | 8 | 0 | 395 | 529 | 583 | storage 571 |
| 8889776 | 330 | 56 | 128 | **5.89** | 56 | 2.965 | 5.990 | **2.02** | **FAR** | Full→Win_1→Opt→Defer→Win_1 | 8 | 0 | 375 | 135 | 88 | storage 56 |
| 19638737 | 381 | 20 | 350 | 8.00 | 48 | 5.046 | 9.808 | **1.94** | **NEAR** | Opt×5 | 6 | 0 | 326 | 205 | 17 | 短 Basic 19 |
| 19716145 | 341 | 46 | 226 | **7.41** | 46 | 10.887 | 19.879 | **1.83** | **FAR** | Full×5 | 8 | 0 | 521 | 221 | 44 | Basic 45 / st 25 |
| 19860366 | 430 | 33 | 287 | 8.00 | 54 | 9.753 | 18.054 | **1.85** | **FAR** | Full→Win_1×4 | 8 | 0 | 147 | 266 | 75 | Basic 31 |
| 19469101 | 469 | 36 | 279 | 8.00 | 59 | 9.973 | 13.984 | **1.40** | **FAR** | Opt×3→Full×2 | 8 | 0 | 143 | 227 | 64 | storage 36 |
| 3356896 | 176 | 17 | 154 | 8.00 | 22 | 1.062 | 1.334 | **1.26** | **NEAR** | Win_1→Win_2→Opt→Win_8→Defer | 3 | 8 | 57 | 70 | 15 | Basic 17 |

OCC 相对单位界：19807137 **1.3×**（OCC 已贴结构界）；其余 **6–28×**。SF 相对单位界：19807137 3.3×，其余 10–46×。除欠盖长脊外，「8× 加速」不是墙时目标。

PR40 扫块 TPS 比与本 Instant-off 墙比同序：14396881 最差，3356896 最轻。19716145 Instant-off 1.83× 重于扫块 1.51×（reuse 末 iter Full 变重）— 以本交错 N=5 为准。

---

## 3. 总论：PC / CC / 学习还错在哪

### 3.1 并行计算（PC）

**目标:** 依赖走 L，反链吃满 8 核；独立工作零 SF 闸税。

**做了什么:** 大块近独立上 `ungated_occ` 已到数百～1100（13217637 ≈ n）。S1「未闸走 OCC pick」在 A 簇成立。Soft=0。

**没做好:**

- **NEAR 不等于吃到 bound@8 墙。** 14396881 ungated≈768、wait-set 仅 4，结构可填 169 波，墙仍 3.34× OCC。税在 SF execute/validate 壳，不在「没发独立工作」。  
- **FAR 簇 ungated 被闸打瘦。** 15274915 ungated 113（n=1226）；16146267 51–251；19469101 143–188。少量 OrderedAdmit 洞把 pick 推进 gate 分支。  
- **`ready_width` 采样多为 0/1** — 不是 wave width，不能当「袋深」证明。精确 22/46/56 波对齐 **未测**。

### 3.2 并发控制（CC）

**目标:** 真冲突 Detect→有序或廉价 Resolve；lazy / 可交换不当锁链。

**做了什么:** lazy 默认不是 OrderedAdmit 对象（有效 DAG 排除 `basic_lazy`）；欠盖脊禁空 Win_1；wait-set 软顶=8；Done-on-success；Soft=0。

**没做好:**

- **软顶 8 对 L=33–571 是结构性欠盖。** 19716145 L=46、8889776 L=56、19807137 L=571：wait-set=8 且 `cover_window=0`。M1 `yield_to_occ_abort` 在「软顶仍输」时撤有序 — 这避免了 PR36 的 50–108 洞，但 **也放弃了走到 L 波**。  
- **双付:** 脊上仍 `unfenced` 几十～五百 + OCC 级 abort，同时 8 个洞付 `refuse`/`edge_ordered_admit`。Detect 预付与 Resolve 列车并存。  
- **15274915 `Full/996`:** 热位置是 997 写者 basic_lazy。标签 Full 却 `cover_window=0`、wait-set=1。错对象。  
- **19716145 Full/6+Full/5:** Full 打在短链，不是 45 写者 Basic。有序对象选错。  
- commute 仅 3356896 稳定 77；肥块 0 — 肥块路径不是 commute 税。

### 3.3 学习

**目标:** ĉ 收敛到「整脊墙最小」；冷探热用。

**做了什么:** 薄块仍探索 Win_2/Win_8；中档 sticky Opt 当 `cover_proven_cheaper` 失败。

**没做好:**

- **`cover_proven_cheaper` 在中档真脊上本箱几乎永不亮。** 需要 last_cover_ok ∧ 有序臂 ∧ n≥2 ∧ ĉ+δ < abort。欠盖时 cover 从未成功 → 永远不能证明更便宜 → 永远不盖。学习被自己的撤单谓词锁死。  
- **8889776 臂在 Full/Win_1/Opt/Defer 间摆** — 没学到「对此 storage-56 要么盖满 L、要么彻底 Opt」。  
- **3356896 末 iter Defer / Win_8** — 薄块探索仍抖；结构 NEAR，学习不稳。  
- 奖励仍更像「灭 unfenced / 降 reexec_ns」，不是 OCC 墙。15274915 选 Full 灭不了 lazy，只瘦 ungated。

### 3.4 因果链

```
中档真脊 L=33–571
  → wait-set 软顶 8 且 cover 未证明更便宜
    → yield_to_occ_abort → sticky Opt / 空 Full（cover_window=0）
      → 脊上 leftover abort ≈ OCC（unfenced 高）
        → 另付 8 洞 OrderedAdmit + SF validate 壳
          → 墙 1.4–2.7× OCC，且远离 L 波

近独立 / 薄有效 DAG
  → 排列已 NEAR（ungated≈n 或 A1 对准短 WAW）
    → 仍走 SF 调度/validate（end_block 只占 0.05–0.3 ms）
      → 墙 1.3–3.3× OCC（单位界两边都远）
```

---

## 4. 簇 A — NEAR：为何还有不必要开销

### 4.1 `14396881` — 近独立对照（最差 TPS）

有效 DAG **L=5 / W=1337 / 波 169 / bound@8=8**。独立 98.9%。lazy 1197 写者在有效图外（正确：不当锁链）。OCC abort 个位。

**为何 NEAR:** 有效冲突几乎没有；list-schedule 瓶颈是 ⌈n/8⌉ 不是 L。ungated 768–828，wait-set 稳定 4，`pick_gate` 6–7。不是 probe-star 锁死 1346 笔。

**为何仍 3.34× OCC（+10.6 ms）:**

| 桶 | 量级 | 归类 |
|----|------|------|
| end_block | 215–300 µs | 必要尾的一小片；**解释不了** 10 ms |
| OCC abort | 双方 3–6 | 非残差 |
| wait-set=4 | 4 个 begin 洞 | 小过闸；相对 1346 独立不应主导 |
| Full/59（iter1..） | 臂标签 | **不必要：** 有效图无 59 写者真脊；疑打 lazy 次链。cover=0 |
| SF vs OCC 壳 | 墙差 10.6 ms | **主残差（不必要）** — A0-majority 仍重于 OCC steal/validate |
| vs 单位界 0.33 ms | OCC 13.7× / SF 45.9× | 双方 meta；本块不可追 serial/8 |

不是 FAR 排错波。是 T1：Opt 路径税。PR40 `skip_ungated_path_tax` 没把大块近独立的 validate 壳打到 OCC。

### 4.2 `13217637` — 近独立 + 过预付 8 洞

L=6、独立 95%、lazy 934。ungated **≈n（1102–1107）** — PC 宽度是本箱最干净的。臂全程 Opt。

**NEAR** 排列。残差 2.64×（+9.2 ms）同 T1 壳。额外：**wait-set=8 顶格** 打在近独立块上 — 过预付，不是盖 L=6 所必需。end_block 0.32–0.49 ms 仍远小于墙差。

### 4.3 `19638737` — 短 Basic + sticky Opt

L=20、max writers=19、波 48（n/P）。臂 Opt，wait-set=6，ungated≈n，unfenced 17–21 ≈ OCC abort。

**NEAR-Opt:** 19 写者若预付 ≱ abort，Opt 是对的。不是「该盖 20 波却没盖」的典型 FAR — 即便盖满 19，等权仍受 ⌈381/8⌉=48 约束。残差 1.94× 是 6 洞 + 壳。中档里 **最接近「Opt 正确、壳过贵」**。

### 4.4 `3356896` — 薄块，沿用 PR19 NEAR

L=17、波 22、bound@8=8。begin_blocked 末 `[16,19,20]` ⊆ storage 短链；commute/ignore 77/77；opt_maj=true；cover_window 热到 8。主链 inc>0 仍在 writers 上，不是独立集税。

**NEAR。** SF 1.334 / OCC 1.062 = 1.26×（+0.27 ms）。双方相对 0.037 ms 单位界 28–36×。残差：A1 预付 vs 廉价 abort + 薄块调度壳。学习末 iter Defer/Win_8 — 探索抖，非拓扑 FAR。

---

## 5. 簇 B — FAR：为何没接近最优排列

### 5.1 `19807137` — 欠盖冲突脊（墙冠军量级）

有效 **L=571 = max writers（storage）**，bound@8=**1.25×**，波=571。等权最优几乎是 **沿 storage 脊串行**，反链 106 只能填旁边。

OCC Instant-off 18.0 ms vs `t_work` 16.8 ms vs 理想 13.5 ms → OCC **1.3× 界，结构上已 NEAR**。SF 45.0 ms = 2.50× OCC。

**为何 SF FAR:** 最优是把 571 写者变成一条有序链（或等价地让 abort 沿着链重叠成近串行）。SF 选全程 Opt、`cover_window=0`、wait-set=8。unfenced **564–594**，SF abort **815–1108** ≈ OCC — Resolve 列车还在，Detect 只种了 8 个无关痛痒的洞。M3 禁止空 Win_1 / Full 硬钉是对的（盖不住 571），但 **Opt 路径没有变成 OCC 快路径**，所以既不盖、也不便宜。

### 5.2 `16146267` — 中档 storage-50，比 PR39 更差

L=50、波 60、storage 50 写者 = 临界路径。等权最优 ≈ 60 波（L 与 n/P 几乎打平）。

全程 Opt，wait-set=8 顶格，cover=0，ungated 最低 51。unfenced 47–68，abort 与 OCC 同量级。

**FAR:** 该盖（或证明不盖更便宜后让 Opt **等于** OCC）。现在两头不靠：8 洞盖不住 50；Opt leftover 继续 abort。这是 M1 sticky Opt 的失败代表 — PR40 扫块比 PR39 更差，Instant-off 仍 2.70×。

### 5.3 `8889776` — 真脊 + 学习摆动

L=56=storage 写者，bound@8=5.89（**L 界，不是 n/P**）。最优必须串起 56 写者。

臂 `Full → Win_1 → Opt → Defer → Win_1`，cover 始终 0，wait-set=8，unfenced 85–99。Win_1 吸收不了 56；Full 也没把 cover_window 拉到 56。

**FAR — 学习没收敛到脊策略。** 这不是「再削壳」能到最优的块。

### 5.4 `19716145` — 中档最大 Δ，仍 FAR

L=46、bound@8=7.41、波 46。几乎无 lazy。热 Basic 45 + storage 25。**最优就是 46 波脊调度。**

全程 Full，但 `selected_arms` 是 Full/6 与 Full/5 — 短链，不是 45 写者。wait-set=8，cover=0，unfenced 44–84，reexec 241–321，末 iter `pick_gate=193`。

PR40 扫块 TPS 0.440→0.660 来自撤 108 洞，不是走到 46 波。Instant-off 仍 1.83×。**FAR：有序对象选错 + 软顶欠盖。**

### 5.5 `19860366` — Win_1 盖不住 L=33

L=33、波 54。Full→Win_1×4，covering_n 到 1，但 cover_window=0、wait-set=8。Win_1 定义上盖不住 31 写者 Basic。unfenced 66–78。**FAR。** 扫块 +0.18 同样是撤过预付，不是最优排列。

### 5.6 `19469101` — 比值最好的 FAR

L=36、storage 36=脊。Opt×3 后 Full×2；**ordered_admit 队列 44** 且 ungated 仅 143–188 — Detect 很忙，cover 仍 0。比值 1.40× 是本箱 FAR 里最轻的，因为 OCC 自己 abort 63–102、墙已贵。**仍 FAR：** 44 个 OA 队列 ≠ 36 波 list-schedule。

---

## 6. 簇 C — `15274915` 错对象

有效 L=77（Basic 77 写者），lazy 另有 997。最优：串 Basic-77，lazy 重叠、零闸。

实测：全程 Full，`Full/996` + `Full/51`，wait-set=**1**（tx 105），ungated 113–178，unfenced 18–68。

**FAR 的两种错叠在一起：**

1. lazy 千写者被标 Full（违反「lazy 永不 OrderedAdmit 对象」的精神 — 标签在、cover 不在）。  
2. 真 Basic-77 只种 1 洞，L=77 未盖。

ungated 从「应有 ~1200」掉到 ~120，是 PC 宽度崩，不是壳噪声。这是 70 输家里 **最不该用「再削 end_block」解释** 的块。

---

## 7. 不必要开销桶（只对 NEAR 块拆；FAR 先别用这张表洗排列）

相对 Instant-off 墙差（SF reuse − OCC med）：

| 桶 | 14396881 +10.6 ms | 13217637 +9.2 ms | 19638737 +4.8 ms | 3356896 +0.27 ms |
|----|-------------------|------------------|------------------|------------------|
| end_block | 0.22–0.30 ms | 0.32–0.49 ms | 0.18–1.15 ms（仅冷） | 0.05–0.07 ms |
| admit_seed | 0（测得） | 0 | 0 | 0 |
| Soft / idle | 0 | 0 | 0 | 0 |
| wait-set 过闸 | 4 洞 | **8 洞（过预付）** | 6 洞 | 2–4 ⊆ 短链 |
| Detect 预付 vs OCC abort | abort 双方个位 | 同 | 同量级 | A1 预付略贵 |
| 调度/validate 壳 | **主残差** | **主残差** | **主残差** | 主残差（小） |
| Instant-tax yield/busy | 不计入墙 | 不计入 | 不计入 | 不计入 |
| per-wave 填槽 | **未测** | 未测 | 未测 | 未测 |

FAR 块的墙差 **首先是排列**（欠盖 + 错对象），壳是第二项。19807137 的 27 ms 差不能用 0.5 ms end_block 解释，也不能用「再 sticky Opt」解释 — OCC 已经贴着 1.25× 界。

---

## 8. 对语料 23/98 的含义

PR40：SF TPS≥OCC **23/98**（PR39 28/98）。本 K=11 全是那 70 个输家的代表，Instant-off **无一块** `sf_le_occ`。

1. **计数被两类锁死，不是被「还没学到 Win_2」锁死。**  
   - ~大块近独立（14396881 / 13217637 / 15274915 一类）：即使排列 NEAR，壳 2.6–3.3×。这些块决定 **wall max**，也拖低中位数。不把 A0-majority 做成 OCC 等价 steal，23 变不成 40。  
   - ~中档真脊（19716145 / 19860366 / 16146267 / 8889776）：PR40 的 Δ 来自撤过预付，**距 L 波仍远**。`cover_proven_cheaper` 锁死 → sticky Opt → 永远 1.8–2.7×。  

2. **再调 mid-band 谓词（软顶、禁空 Win_1、path-tax skip）边际已小。** 19716145 已从 0.44 到 0.66，Instant-off 仍 1.83×；16146267 相对 PR39 还退了。同一把刀削不到 23→过半。  

3. **19807137 类不该再被当成「可盖脊」。** OCC 已 1.3× 结构界。目标是 Opt 路径 = OCC 路径，不是 cover_window→571。  

4. **15274915 是正确性/对象债，不是吞吐调参。** Full/996 在语料里会继续制造「大 n + 低 ungated」输家。  

5. **不要把 SF≤OCC 计数当北极星**（PR36 已写）。本箱 NEAR 块相对单位界 OCC 也是 8–28×；产品若要「小块不慢于串行」，是降核/低 meta，超出排列。

6. **若只做一件事让 23 松动：** 分叉，不要统一。  
   - A：OCC 等价 ungated 路径（validate/steal），不动 OrderedAdmit 对象。  
   - B：要么给 L∈[20,64] 的真 storage/Basic **一条能证明 cover 的路**（打破 `cover_proven_cheaper` 死锁），要么承认不盖并把 Opt 墙打到 OCC。现在卡在中间。  
   - C：禁止 Full 标签落在 lazy 千写者上（策略已写，15274915 的臂轨迹说明执行口还在漏）。

---

## 9. 还剩什么问题（按杠杆，不是按块号）

1. **P0 — FAR 真脊：软顶 8 + 撤单死锁。** 16146267 / 8889776 / 19716145 / 19860366 / 19469101。接受：要么 cover_window 吸收到 L（并证明墙 < OCC），要么 ungated Opt 墙 = OCC。现在两头不靠。  
2. **P0 — NEAR 近独立壳 2.6–3.3×。** 14396881 / 13217637。end_block 不是主因。A0-majority 必须 OCC 等价。  
3. **P0 — 15274915 错对象 Full/996。** lazy 不当 OA；真 Basic-77 未盖。  
4. **P1 — 19807137 Opt ≠ OCC。** 结构界已到；差的是欠盖脊上的 SF 壳 + 8 洞。  
5. **P1 — 8889776 学习振荡。** 形态可迁移失败。  
6. **P2 — 3356896 探索抖 + 缺 per-wave dump。** 不阻塞设计；阻塞 NEAR→PASS 证明。  
7. **P2 — 宿主 4 核跑 8 线程。** 比值可用；绝对 ms 勿与 8 物理核对打。

---

## 10. 底线

- **还没接近理论最优排列的块（多数中档真脊 + 15274915 + 19807137）:** 因为 wait-set 软顶和 `yield_to_occ_abort` 把 Detect 停在 8 个洞 / sticky Opt，cover 无法证明自己，L 波从未成为调度对象；再叠加错 Full 对象。  
- **已经接近的块（近独立与薄 3356896）:** 有效 DAG 的 list-schedule 骨架在；剩余是 **不必要的 SF 调度/validate 壳**（外加 4–8 个非必要洞），以及双方都远高于单位成本界的并行固定税。  
- **23/98** 是这两类的加权和。PR40 中档 Δ 是「少付过预付」，不是「排到最优」。下一刀必须按 A/B/C 分叉，不能再指望同一套 mid-band 谓词抬计数。

Soft=0 全 iter 确认。Instant-off 主证。Instant-tax 未加进墙。
