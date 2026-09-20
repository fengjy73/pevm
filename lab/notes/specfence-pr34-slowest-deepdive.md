# PR #34 最慢块多维开销深挖 + 问题定位

**基线:** `cursor/specfence-prepaid-losers-b5de` @ `0144211ae115c87eb2e80828e17e0750d3e2cf6b`  
**扫块:** [`specfence-pr34-allblocks-sweep.md`](specfence-pr34-allblocks-sweep.md)（99/99，Soft=0，reuse N=3）  
**深挖:** Instant-off N=7 interleaved PRIMARY；`SPECFENCE_PROFILE=1` N=7 只作 **Instant-tax**（不可加总进墙，且会改学习）  
**用语:** OptimisticRead / OrderedAdmit  
**规则:** 不发明 ns；未测标「未测」；worker 求和 Instant 桶标 **Instant-tax**  
**本盒:** 4 物理核 · harness `cores=8`  
**Soft=0** 全行（主证 / PROFILE `soft_wait_arms=0`）  
**原始:** `lab/results/pr34-k8-deepdive-instant-off.json` · `lab/results/pr34-k8-deepdive-profile.json`（gitignore）  
**摘要 JSON:** [`specfence-pr34-slowest-deepdive-summary.json`](specfence-pr34-slowest-deepdive-summary.json)

K=8 稳定最慢（绝对 SF wall ∪ SF/OCC 比；排除 2179522 / 19434587 单 iter 病理）：

`14396881, 15274915, 13217637, 19807137, 17666333, 15538827, 14334629, 15199017`

---

## 0. 总定位（先读这节）

**一句话:** 全集最慢不是 3356896 那种「薄 WAW、unfenced≈0、仍输 0.2 ms」。最慢尾是 **n=700–1300 肥块**：OCC 把 **basic_lazy 数百～千写者** 当廉价重叠更新（墙 4–8 ms）；SpecFence 把同一 ℓ 学成 Win_1 / Full / Defer，种 **6–95 个 begin 洞**，`pick_occ_n≈0`（整块离开 OCC pick），再付 **0.3–3 ms 的 `end_block`**。局部 unfenced 常常只有个位数——**局部 CC 不惨、整块墙 4–27×**。

| 新/旧类 | 代表 | 结构 | 墙机制 |
|---------|------|------|--------|
| **S-lazy（新）** | 14396881, 13217637, 15199017, 17666333, 14334629, 15538827 | DAG 短或中等；D1 头名是 **basic_lazy 连续写者**（459–1197） | 闸让 800–1300 笔走 wave/refuse；块末 D1/learn 0.3–3 ms；OCC abort 个位～几十 |
| **S-mixed** | 15274915 | L=77 真 Basic 脊 **+** lazy 997 | 冷 begin_n=**95**；reuse 仍 ~80 ms vs OCC 5.3 |
| **Spine-U（附录 WAW）** | 19807137 | storage 571 写者，bound@8≈1.25 | Win_1/Full **盖不住**；unf 79–575；OCC 自己也 14–19 ms（冷有 2.3 s 病理） |
| 旧 **U/O/D/L4** | 本 K8 last-iter 几乎不贴 | — | 预付刀吃掉了 52 集里的 U/L4；**没吃掉肥 lazy 壳** |
| 旧 **3356896 薄输** | 不在 K8 | Win_2 cover unf=0，ratio 1.4 | 仍输，但不是全集问题 |

**必要 vs 不必要（相对 OCC 墙）**

- **必要（结构真相）:** 15274915 的 77-writer Basic、19807137 的 571-writer storage —— 必须有人付串行或 abort。  
- **不必要 / 可铲:** 把 **basic_lazy 长链** 当 OrderedAdmit 对象；有闸后 **整块** 离开 `next_occ_task`；肥块 `end_block` 比 3356896 的 ~50 µs 高一个数量级；F7 在 lazy ℓ 上选 Win_1/Defer。  
- **已排除作主账本:** Soft；`wait_for_dependency` 多数 0（洞走 refuse/skip，不是 park Instant）；K8 上 `commute_skip=0`（与 3356896 的 77 不同）。  
- **Instant-tax（PROFILE）:** SF `profile_scheduler_ns` 系统性大于 OCC（肥块 +数 ms～+150 ms worker 和）。**不能**加进墙，只证明 pick 更重。

**PC:** Instant-off `occ_pick_while_gated` 几乎为 0——不是「未闸走了 OCC pick」，而是 **几乎没有未闸 pick**（`pick_occ_n=0`）。`ready_width_mean` 0 或 1（袋空，税在分支）。`idle_core_ns` 产品路径 0。

---

## 1. Instant-off 主证墙（N=7）

| block | n | OCC med | SF cold | SF reuse | × | last arm | unf last | begin_n | pick_occ 暖 | end_block 暖 |
|------:|--:|--------:|--------:|---------:|--:|----------|--------:|--------:|-------------|--------------|
| 14396881 | 1346 | 4.403 | 107.4 | **110.9** | 25.2 | Defer | 4 | 6–9 | 0 | 0.59–1.25 ms |
| 15274915 | 1226 | 5.313 | 238.1 | **79.8** | 15.0 | Opt | 7 | **95** | 0 | 0.34–1.29 ms |
| 13217637 | 1100 | 4.731 | 68.8 | **64.3** | 13.6 | Win_1 | 7 | 26–29 | 0（末 1） | 0.28–1.36 ms |
| 19807137 | 712 | 17.590† | 50.2 | **65.5** | 3.7 | Win_1 | 236 | 38 | 0–3 | 0.50–1.73 ms |
| 17666333 | 961 | 7.374 | 33.8 | **35.6** | 4.8 | Win_1 | 13 | 36 | 0 | 0.36–1.98 ms |
| 15538827 | 823 | 5.675 | 29.5 | **29.4** | 5.2 | Win_1 | 21 | 79–85 | 0 | 0.31–3.07 ms |
| 14334629 | 819 | 6.246 | 27.8 | **27.7** | 4.4 | Full | 14 | 63 | 0 | 0.35–3.06 ms |
| 15199017 | 866 | 4.276 | 20.5 | **21.9** | 5.1 | Win_1 | 3 | 44 | 0 | 0.31–0.40 ms |

† 19807137 OCC[0]=**2278 ms** 单次病理；median 用其余 ~14–19 ms。方向不变。

扫块 N=3 与深挖 N=7 同量级（14396881 ~106 vs 111；15274915 reuse 扫块 152 含噪声，N=7 reuse **79.8** 更稳）。PRIMARY 全 false。

`prepaid_ns` / `refuse_ns` / `gate_stall_ns` 在本 tip Instant-off **非 0**（与 PR25 尸检「产品路径全 0」不同）。三者常同值。有时接近墙（13217637 单 iter refuse_ns 63.8 ms / 墙 68 ms），有时远超单时钟常识（15274915 冷 201 ms / 墙 238 ms）。**当墙时钟嫌疑，不当加法分解；不与 Instant-tax 再加。** `yield_ns` 注释为 Instant-tax。`reexec_ns` 是执行计时，19807137 暖机 138–197 ms——worker 向，**不加进墙**。

---

## 2. 逐块

### 2.1 14396881 — 近独立肥块，S-lazy 极值（25×）

**形态（历史 + 本跑 structure）:** `near_independent_meta_gap`，**不在 52 集**。L=5 W=1337 RAW=0 WAW=13。D1 头名 **basic_lazy 1197 写者**（loc `11880556412163541166`，tx 47,48,49…连续）+ lazy 60 + Basic 5。

**臂:** 冷/前段 Win_1 → i4 Opt → i5–6 **Defer/1196** + Win_1/60。`w_need` last=2。cover 不稳。

**CC:** unf **1–4**；OCC abort **3–6**；`commute_skip=0`；`wait_for_dependency=0`；`double_pay=0`。

**开销:** begin 仅 **6–9** 洞，但 `pick_occ_n=0`（i0 为 1）。墙 **~110 ms** 不能用 6 个 Detect hop 解释——独立集在付 SF pick + 块末。`end_block` **0.59–1.25 ms**（单时钟，约占 gap 的上限 ~1/25，单独不够）。`occ_kernel_validates=0`：validate 不是 OCC bool 核。

**归类:** **S-lazy / 附录 meta-gap**。OCC 对 1197 lazy 几乎无 abort；SF 把它当成一条要学的 ℓ。  
**必要:** 13 条真 WAW。**不必要:** 对 lazy 千写者 Defer/Win_1 + 整块 wave pick。

### 2.2 15274915 — S-mixed，95 洞预付（15×）

**形态:** 52 集 `mixed_RAW_WAW`。L=77 W=1121 RAW=35 WAW=120。D1：lazy **997** + **Basic 77**（与 L=77 对齐）+ storage 短。

**臂:** Opt 钉 begin_n=**95**；中段 Full / Win_1；末 Opt。冷墙 **238 ms**（refuse/prepaid 记 201 ms，同量级嫌疑）；reuse 77–96 ms。

**CC:** unf **3–7**（局部不惨）；OCC abort 64–104；`wait_for_dependency` 0–3；dp=0。

**开销:** 95 个 begin 洞 → `has_any_gated` 贯穿；`pick_occ=0`。`end_block` 0.34–1.29 ms。PROFILE Instant-tax：SF sched − OCC sched **+7–37 ms**（worker 和）。

**归类:** **S** + 过宽 Detect。77-writer Basic 是真脊（必要）；997 lazy 与 95 洞是税。  
**对照 PR34 land:** 「15274915 21→18× 仍肥」——本盒 N=7 **15×**，刀没动地板。

### 2.3 13217637 — 短 DAG（L=6）仍 14×

**形态:** 52 集 mixed。**L=6** W=1060 RAW=21 WAW=44。D1：lazy **934** 连续 + 短 Basic/storage。`max_writers_on_loc` 目录=5，D1 lazy 却 934——lazy 链被 OCC 有效图丢掉，SF D1 仍看见。

**臂:** 冷～中 Full，末 Win_1（lazy 933 + Full/5 + Win_2/2）。begin **26–29**。

**CC:** unf 4–9；OCC abort 4–20。

**开销:** SF 墙钉在 **62–69 ms**。`pick_occ` 暖机 0。L=6 ⇒ 并行上界高，OCC ~4.7 ms 合理；SF 14× 是壳，不是最长路。

**归类:** **S-lazy**。预付刀的 Win_w 对「无长脊」块帮不上。

### 2.4 19807137 — 附录 storage 脊，Spine-U（~4×，unf 仍百级）

**形态:** `WAW_spine`，**不在 52 集**。L=571 W=106 RAW=9 WAW=628。D1：**storage 571** 连续写者。

**臂:** 冷 Opt unf=**575** → Full / Win_1。begin **38** 钉死。`edge_ordered_admit` 暖机 **1261–1743**。

**CC:** unf **79–575**；OCC abort **746–1256**（脊上 OCC 自己也贵）。`reexec_ns` Instant-off 105–197 ms（**Instant/worker，不加墙**）。`optimistic_read_occ_fast` 暖机 ~845–965。

**开销:** SF reuse 56–87 vs OCC med 17.6。比 S-lazy **倍数小、绝对墙仍大**。OCC[0] 2.3 s 不作主证。PROFILE：SF sched Instant-tax 系统性 +120–157 ms worker 和。

**归类:** **Spine-U**（不是 PR34 的 Win_2+unf≥8；是 **storage 全脊盖不住**）。Full 持久化长脊仍禁 —— 本块证明禁令正确，但也没有第三种便宜动词。

### 2.5 17666333 — 双 lazy 脊 + Win_1（4.8×）

**形态:** mixed，L=32 W=897。D1：lazy **450** + lazy **374** + Basic 31 + storage 18。

**臂:** Opt → Full → Win_1（`bef034…:Win_1/30`）。begin **36**。

**CC:** unf 7–33；`wait_for_dependency` 暖机 **4–20**（K8 里 park 计数最多的一块）；OCC abort 45–80。

**开销:** SF 33–47 vs OCC 7.4。`end_block` 常 **1.7–2.0 ms**。`pick_occ=0`。PROFILE sched Instant-tax SF≫OCC。

**归类:** **S-lazy** + 轻量 WAW。unf 未清零，但墙主因仍是 36 洞 + 壳，不是 575 级列车。

### 2.6 15538827 — 85 洞 + 块末可到 3 ms（5.2×）

**形态:** mixed，L=35。D1：lazy **533** + storage 35。

**臂:** Opt → Full → Win_1。begin **79–85**。

**CC:** unf 10–33；OCC abort 50–74；wfd 0–6。

**开销:** SF 钉 **27–33 ms** vs OCC 5.7。`end_block` **0.31 或 2.4–3.1 ms**（双峰；3 ms 已是 OCC 墙的一半）。`pick_occ=0`。

**归类:** **S** / S-lazy。85 洞接近 15274915 的 95，墙却只有其 1/3——n 与 gas 更小，机制同类。

### 2.7 14334629 — 63 洞 Full（4.4×）

**形态:** mixed，L=28。D1：lazy **486** + Basic 27。

**臂:** Opt → **Full**（`bef034…:Full/27`）。begin **63**。

**CC:** unf 11–17；wfd 0–13；OCC abort 29–54。

**开销:** SF 25–30 vs OCC 6.2。`end_block` 同样 0.35 / ~2.6–3.1 ms 双峰。`pick_occ=0`。

**归类:** **S-lazy**。Full 标在 27-writer Basic，不是 486 lazy——短 Full 合法，整块仍因有闸走 SF pick。

### 2.8 15199017 — 最「干净」的 S-lazy（5.1×）

**形态:** mixed，**L=7** W=831。D1：lazy **459** + 169 + 短链。

**臂:** **全程 Win_1**（`5a55…:Win_1/458`）。begin **44** 钉。unf **3–6**，cover=1。这是「学对了轻窗」仍 5×。

**CC:** OCC abort 仅 4–13。dp=0。wfd≈0。

**开销:** SF **20.5–22.6** vs OCC **4.15–4.48**（两边都稳）。`end_block` **0.31–0.40 ms**（K8 里最稳的块末，仍 ≫ 3356896 的 50 µs）。`pick_occ=0`，`ready_width=0`。

**归类:** **S-lazy，局部赢、全局输**（PR25 3356896 悖论的肥块版）：Win_1 盖住 458 lazy 头，unf≈0，墙仍 5×。

---

## 3. 开销桶（K8 合并，必要 vs 不必要）

对照：各块 gap = SF reuse − OCC med（本盒）。**不把 Instant-tax 加总成 gap。**

| 桶 | 测到 | 归类 | 说明 |
|----|------|------|------|
| **basic_lazy 长链当 ℓ** | D1 头名 459–1197 writers；structure 与 OCC 有效 DAG（L=5–7）脱节 | **不必要（相对 OCC）** | OCC lazy 更新不付这条脊；SF 学 Win_1/Defer/Full |
| **整块离开 OCC pick** | 暖机 `pick_occ_n=0`（偶发 1–3） | **不必要壳** | 6 个洞（14396881）已够让 1346 笔走 wave |
| **begin 洞宽度** | 6 / 95 / 26 / 38 / 36 / 85 / 63 / 44 | 真脊必要 + lazy 过预付 | 15274915 的 95、15538827 的 85 是过预付嫌疑 |
| **unfenced 列车** | S-lazy 3–33；19807137 **79–575** | 仅 spine 块仍是主 CC 账 | 其余块 **解释不了** 15–25× |
| **end_block** | **0.28–3.1 ms** 单时钟 | SF 独有；肥块 ≫ 50 µs | 可与墙比；单独不够 25×，够解释薄 gap 的一部分 |
| **commute** | K8 **全 0** | 与 3356896 的 77 不同 | 肥块税不在 commute collect |
| **validate 核** | `occ_kernel_validates≈0` | 走 optimistic_fast | 未测墙 ns |
| **refuse/prepaid/gate_stall ns** | Instant-off 非 0，常互等 | **半可信墙时钟**；勿加总 | 有时≈墙，有时像叠加 |
| **reexec_ns** | S-lazy 0.3–35 ms；spine 100–197 ms | 执行计时 / 或 worker 向 | **不加进墙** |
| **PROFILE sched Instant-tax** | SF≫OCC（见 §4） | Instant-tax | 与 pick 换函数同向 |
| **maybe_wait / Soft / idle 产品** | 0 | 已关 | — |
| **ready-bag** | 0 或 1 | 袋空 | 勿读成宽度 800 |

---

## 4. Instant-tax 摘录（PROFILE N=7）

**不可加总成墙。PROFILE 会改策略**（13217637 Instant-off 走 Full/Win_1，PROFILE 走出 Win_2；14334629 PROFILE 末 Win_2）。只比「SF sched 是否系统性重」。

暖机且避开 19807137 OCC[0] 2.3 s：

| block | SF−OCC sched Instant-tax（µs，worker 和） | 方向 |
|-------|------------------------------------------|------|
| 14396881 | +0.2–0.9 ms（i5 离群 +9.9） | 弱于绝对墙 |
| 15274915 | **+7–38 ms** | 重 |
| 13217637 | 噪声（−1.4～+30） | 不稳 |
| 19807137 | **+121–158 ms** | 极重 |
| 17666333 | **+19–57 ms** | 重 |
| 15538827 | **+6–30 ms** | 重 |
| 14334629 | **+13–31 ms** | 重 |
| 15199017 | 多 iter 近 0，两 iter +10–12 | 中 |

`maybe_wait_ns=0`。handler 无稳定 SF>OCC（spine 块除外）。validate 无稳定方向。  
**结论:** Instant-tax 支持「肥块 SF pick 更贵」，**不能**写成「墙 gap = sched 的 x ms」。

---

## 5. 学习可信度

Instant-off 轨迹可信。F7 在 lazy 千写者上选 Win_1/Defer/Full —— 按 leftover/unf **局部合理**（unf 降到个位），按 PRIMARY **全局错**：奖励仍看不见「整块 pick + 块末 ms」。

`prepaid_ns` 本 tip 已非 0，但与墙不对齐（可大于或远小于 gap）→ **仍不能**当 ĉ 的墙时。PROFILE idle 进入学习会换臂（重演 PR25：仪表教错）。

`occ_aborts`：OCC 侧 3–1256，SF 侧口径不同；**禁止**主导 F6。

---

## 6. 问题定位摘要

1. **全集最慢 = 肥 lazy / 肥混合，不是 52 集薄 WAW。** 跑 `ALL_BLOCKS=all` 才看见 14396881（附录 meta-gap）以 25× 居首。  
2. **PR34 预付刀的胜利面（U/D/L4↓）与最慢尾正交。** K8 last-iter 几乎没有 U/D/L4；S 与 S-lazy 仍在。  
3. **共同机制:** `basic_lazy` 长链 → 种洞 → `pick_occ_n=0` → n 笔 SF 调度 + 肥块 `end_block`。OCC 用 lazy 更新 + 少量 abort 结束。  
4. **19807137 是另一类:** 真 storage 脊，unf 仍百级；52 集正确排除，但作为控制组说明「禁 Full 长脊」之后没有便宜替代。  
5. **3356896 仍薄输（扫块 1.40×，Win_2 unf=0），不是下一刀最大杠杆。**  
6. Soft=0 保持。

---

## 7. 下一刀假设（只分析，不落地）

| 序 | 假设 | 预期方向 | 风险 | 依据 |
|----|------|----------|------|------|
| 1 | **basic_lazy 长链永不种 OrderedAdmit / 不进 D1 热 ℓ**（交给 OCC lazy） | 砍 S-lazy 4–25× 的主因 | 真 Basic 被误标 lazy → 漏闸 abort | 14396881 / 13217637 / 15199017 D1 头名 lazy 459–1197，OCC abort 个位 |
| 2 | **有闸时未闸 tx 仍走 `next_occ_task`** | 砍整块 pick 壳 | 漏 refuse；ERC-20 mid-plant 史 | K8 `pick_occ=0`，6 洞已污染 1346 笔 |
| 3 | **肥块 end_block 再削**（lazy 已见则跳 MV merge / 限 D1 top） | 砍 0.3–3 ms 墙 | 学丢短 storage | 单时钟已测；15199017 也有 0.35 ms 地板 |
| 4 | **begin 洞 cap**：n≥512 时 `begin_blocked` 上限（例如 ≪ 95） | 砍 15274915 / 15538827 过预付 | 真 77-writer Basic 欠覆盖 → unf 回升 | 95 洞 + unf 仍 3–7 = 过预付 |
| 5 | **给 lazy 预付打墙时钟**（不要 Instant idle） | 让 F7 看见 Win_1(458) 输 OCC | 写进 ĉ 会教错 | 15199017 全程 Win_1 仍 5×；`prepaid_ns` 与墙不对齐 |
| 6 | **19807137：不要 Full 571**；若做，只允许极短 storage 窗 + 其余 Opt | 控 unf 列车，墙可能仍 ≥OCC | 脊 abort 回潮 | L=571 bound@8≈1.25，SF 很难赢 OCC |
| 7 | **不要** 恢复长脊 Full / mid-execute 种边 / 热路径 T3 flush / PROFILE idle→ĉ | — | 已翻车 | land + 本 PROFILE 换臂 |
| 8 | **不要** 用 `ready_width` 或 `occ_aborts` 做奖励 | — | F6 | 袋深 0–1 |

---

## 8. Soft=0 / 安全

主证与 PROFILE：`soft_wait_arms=0`。无 CC/policy/learn 改动。深挖二进制 `specfence_block_deepdive` 只读遥测。
