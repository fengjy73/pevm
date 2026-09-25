# Soft=0：执行膨胀、理想 TPS，以及步级偏移

**日期：** 2026-09-25（北京时间）
**分支：** `cursor/soft0-execute-inflation-dig-e5fa`，基线 `cursor/soft0-busy-stall-dig-75d4`。产品调度、Avoid、Admit、Learn 未改。
**宿主：** 本机 4 个物理核，Intel Xeon（KVM），`lscpu` 为 1 线程/核，L3 320 MiB。`governor` 读不到。C>4 的曲线不在这台机器上跑。ict21 用 `scripts/soft0_percore_scan.sh`，脚本在物理核不够时跳过，不超订。
**二进制：** `target/release/examples/specfence_inflation_dig`。`cargo +stable`（rustc 1.98.1），release，`profile.release.lto=false`。
**原始数据：** `results/soft0-execute-inflation/scan/`。关旗墙是 `wall-c*.jsonl`。同钟代价和 DAG 来自探针开的 `profile-c1.jsonl`（K=10）。步级 trace 是 `step-trace.jsonl`。汇总是 `C1.json`、`C2.json`、`C4.json`、`curves.json`。

`ge_1_5=false`。下面每一格 R 的 95% CI 都整个低于 1，没有一格整体高于 1.5。

## 结论

在这台 4 核上，关旗墙的主输出是 TPS，不是「SpecFence 更接近理想」。块 15274915、C=4：`TPS_ideal_tx` **948699**，`TPS_ideal_step` **804338**，`TPS_OCC` **254484**，`TPS_SF` **70865**。OCC 约为 tx 级理想的 0.268，SF 约为 0.075。SF 比 OCC 慢，R 的中位是 **0.278**，CI **[0.241, 0.289]**。

同钟 Σwork（OCC workers=1 的 `ExecPhase.total_ns`）大块是 **4.977 ms**。历史笔记里的 Σwork=3.02 ms、`L_crit`=1.19 ms 是另一只时钟，只记在 `historical_l_crit_ms_not_this_clock`，不拿来减这只墙。这只时钟上，含 WAR 的 tx 级 `L_crit` 是 **1.292 ms**；去掉 WAR 的 tx 级 `L_crit` 是 **0.684 ms**。步级无限核关键路径 `L_step`（RAW+WAW，去掉 beneficiary 和 lazy）是 **0.688 ms**。把它拉长到约 1.29 ms 的，是把 beneficiary / lazy 写算进依赖，不是 `abd6bb397881` 那 76 跳。

C=4 大块 SF 相对 `Ideal_tx` 的缺口里，探针分解把 **0.077** 记成 inflation、**0.923** 记成 schedule loss。膨胀的相位排序（按 `Ideal_C(par)` 下降）是 post、interp、pre。schedule loss 旁边有一轮重执行线程中位 **41.333 ms**。同一次扫描的第 29 轮（round 28）SF 活锁，约 400% CPU、780 s 未返回，已杀掉，不进中位。产品调度没有改。

## 协议

**计时边界。** tx 列表和区块环境在 `Instant` 之外建好。三种引擎：

- `seq`：`execute_revm_sequential`（CacheDB + `transact` + commit）。
- `occ`：`execute_revm_parallel` / `ConcurrencyMode::Occ`。
- `sf`：`execute_revm_parallel` / `ConcurrencyMode::SpecFence` / `run_sf_block`。不走 OCC 的 `next_occ_task` 环，也不走 `Pevm::execute` 的 `force_sequential` / `n_tx < workers` / `gas_used < 4_000_000` 回退。

扫描直接调上面三个入口。JSON 里 `product_gate_fallback` 在 C=1/2/4 的两块都是 false（1226 笔、gas 29928443；176 笔、gas 4033966）。`seq≡par` 那一节才走产品 `Pevm::execute`。

**没有不计时热身。** 每一轮 timed 都是新的 `Pevm`。中位是统计中位（偶数个样本取中间两个的平均）。bootstrap 10000 次、种子 0、分位 2.5/97.5。`S` 和 `R` 按轮次配对。`S_overall` / `R_overall` 是各块中位墙之和的比，几何平均只作旁注。

**工人。** `SPECFENCE_PIN_CPUS` 在线程启动时 `sched_setaffinity`。脚本再用 `taskset` 把进程钉在同样的物理核上。C=1 钉 cpu 0，C=2 钉 0,1，C=4 钉 0,1,2,3。SEQ 把调用线程钉在这组的第一颗核上。

**两只时钟。**

- 关旗墙是 TPS 的来源。`SPECFENCE_INFLATION` 等探针关着。
- `Ideal_tx` 的每笔代价是探针开、OCC workers=1 的成功 `ExecPhase.total_ns` 的跨轮中位。SEQ 的 `transact+commit` 只报 basis A。
- 分解用关旗墙 + 探针开的 F 和每笔代价。探针把每笔代价抬高时，inflation 偏高、schedule loss 偏低。表里标了来源。

**DAG。** tx 级边来自 OCC workers=1 最终成功 incarnation 的读写哈希。WAW 是连续写者，RAW 是最近的更早写者，WAR 是最近的更晚写者。去掉 beneficiary 哈希和 lazy 写。`Ideal_tx` 默认含 WAR。另报只含 RAW+WAW 的 `L_crit`。

**步级。** `SPECFENCE_STEP_TRACE=1` 才让 OCC 走 `inspect_run`，而且 inspector 只记 PC / opcode 下标就返回。关旗不进这条路径。trace 墙不是产品墙。分数是 trace 里 `ExecPhase` 时长的比例，K=3 轮取中位，再乘到上面那只同钟每笔代价上。RAW/WAW：`start_i + r_i ≥ start_j + w_j`。WAR 主方案不排序。保守方案把后写者收成 `start_w ≥ start_r + r − w`。多条键取最紧的 lag。`w` 是最后一次写，WAW 的后继用第一次写。

## TPS 曲线（本机）

TPS = `n_tx / 中位墙秒`。`TPS_ideal_tx` = `n_tx / Ideal_tx`。`TPS_ideal_step` = `n_tx / Ideal_step`，主方案是去掉 beneficiary 和 lazy 的 RAW+WAW。数字来自 `curves.json`。

### 块 15274915（n=1226，gas=29928443）

| C | TPS_SEQ | TPS_OCC | TPS_SF | TPS_ideal_tx | TPS_ideal_step | Ideal_tx ms | Ideal_step ms | L_step ms | LB ms |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 352298 | 201218 | 139824 | 246311 | 246311 | 4.977 | 4.977 | 0.688 | 4.977 |
| 2 | 347008 | 210271 | 111191 | 492453 | 432093 | 2.490 | 2.837 | 0.688 | 2.489 |
| 4 | 343061 | 254484 | 70865 | 948699 | 804338 | 1.292 | 1.524 | 0.688 | 1.292 |

C=4 的关旗墙：大块 SF 只有 28 轮（round 28 活锁，见下）。SEQ 中位 3.574 ms，CI [3.528, 3.631]。OCC 4.818，CI [4.724, 4.877]。SF 17.303，CI [16.603, 20.004]，最小 13.548，最大 40.717。

C=1 关旗墙 K=30：SEQ 3.480 [3.470, 3.515]，OCC 6.093 [6.062, 6.381]，SF 8.768 [8.665, 9.034]。
C=2 K=30：SEQ 3.533 [3.505, 3.603]，OCC 5.831 [5.793, 5.894]，SF 11.026 [10.969, 11.311]。

| C | S_occ | S_occ CI | S_sf | S_sf CI | R | R CI |
| ---: | ---: | --- | ---: | --- | ---: | --- |
| 1 | 0.571 | [0.545, 0.578] | 0.397 | [0.385, 0.403] | 0.695 | [0.674, 0.727] |
| 2 | 0.606 | [0.599, 0.617] | 0.320 | [0.313, 0.327] | 0.529 | [0.515, 0.535] |
| 4 | 0.742 | [0.728, 0.761] | 0.207 | [0.178, 0.216] | 0.278 | [0.241, 0.289] |

`S_overall`（两块中位墙之和）：C=1 为 0.553 / 0.384，C=2 为 0.577 / 0.302，C=4 为 0.675 / 0.199（OCC / SF）。两块的 S 都小于 1。

### 块 3356896（n=176，gas=4033966）

| C | TPS_SEQ | TPS_OCC | TPS_SF | TPS_ideal_tx | TPS_ideal_step | Ideal_tx ms | Ideal_step ms | L_step ms | LB ms |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 630484 | 250693 | 173250 | 365359 | 365359 | 0.482 | 0.482 | 0.019 | 0.482 |
| 2 | 577339 | 214533 | 105163 | 730712 | 708973 | 0.241 | 0.248 | 0.019 | 0.241 |
| 4 | 454108 | 166801 | 67483 | 1455960 | 1343125 | 0.121 | 0.131 | 0.019 | 0.120 |

C=4 K=30：SEQ 0.388 [0.361, 0.409]，OCC 1.055 [0.954, 1.134]，SF 2.608 [2.496, 2.694]。R=0.405，CI [0.358, 0.447]。

OCC 对 `TPS_ideal_tx` 的接近度，大块从 C=1 的 0.817 降到 C=4 的 0.268。SF 从 0.568 降到 0.075。核变多时理想 TPS 上升，实测墙没有跟着升到同一比例。SF 从 C=1 到 C=4 更慢。

## A. Σwork 和 Ideal 怎么计时

同钟 Σwork 是 OCC workers=1、探针开、K=10 的每笔 `ExecPhase.total_ns` 中位之和。大块 **4.97745 ms**，薄块 **0.48172 ms**。basis A（SEQ `transact+commit` 中位和，C=1 profile）大块 **3.354 ms**，薄块 **0.262 ms**。关旗 SEQ 墙（C=1）大块 3.480 ms、薄块 0.279 ms，靠近 basis A，不靠近同钟 Σwork。同钟比 SEQ 墙贵，是因为并行 VM 在 1 个工人上的 `vm.execute` 包住了预热、解释和写提交，不是 CacheDB `transact`。

`Ideal_tx(C)` 是这条代价向量在 tx DAG 上的非延迟、关键路径优先列表调度。C=1 的 makespan 等于 Σwork。`LB = max(L_crit, Σwork/C)`。大块 C=4 时 `L_crit`（含 WAR）1.292 ms 大于 Σwork/4 = 1.244 ms，所以 `Ideal_tx` = `LB` = 1.292 ms。去掉 WAR 的 tx 级 `L_crit` 是 0.684 ms，C=4 的 RAW+WAW makespan 是 1.245 ms。

## B. 每笔膨胀：OCC 对 SF

比值是探针开的每笔 `total_ns` 除以 OCC workers=1 的同笔。C=1 的 OCC 比值按构造是 1。

大块 SF / OCC@1，C=1 profile：p50 **1.304**，p90 **1.389**，max **5.014**。多出来的线程时间最多的合约是 `0x6262998ced04146fa42253a5c0af90ca02dfd2a3`，**0.603 ms**。其次 `0x7758e507850da48cd47df1fb5f875c23e3340c50` 0.081 ms，`0xdac17f958d2ee523a2206206994597c13d831ec7` 0.031 ms。

同一合约在 C=4 的 OCC 额外线程是 **2.949 ms**，SF 是 **2.452 ms**。C=4 SF 的每笔比值 p50 2.249，p90 2.919，max 63.45。OCC 的 p50 2.352，p90 3.084，max 18.20。max 是单笔重执行，不是整块墙。

## C. 相位

`mv_lookup` / `mv_scan` / `storage` 在这套 profile 里是 0：`SPECFENCE_INFLATION_READS=1` 和 `SPECFENCE_INTERP_SPLIT=1` 没有开。下面只排 pre / interp / post / record。排序键是把该相位的并行代价收成串行代价后，`Ideal_C(par)` 下降多少毫秒。`thread/C` 是线程差额除以 C。

**C=4，块 15274915，OCC（相对 OCC@1）：**

| 相位 | Ideal 下降 ms | 线程差额 ms | 线程/C ms |
| --- | ---: | ---: | ---: |
| post | 0.670 | 2.680 | 0.670 |
| record | 0.407 | 1.632 | 0.408 |
| interp | 0.290 | 1.169 | 0.292 |

**C=4，块 15274915，SF：**

| 相位 | Ideal 下降 ms | 线程差额 ms | 线程/C ms |
| --- | ---: | ---: | ---: |
| post | 0.662 | 2.652 | 0.663 |
| interp | 0.314 | 1.254 | 0.314 |
| pre | 0.141 | 0.567 | 0.142 |

C=1 的 SF 没有重执行（reexec 线程 0）。相位 Ideal 下降就是线程差额：post 0.428，interp 0.300，pre 0.220，record 0.039 ms。C=1 OCC 的 inflation 是 0。

薄块 C=4 SF 的 Ideal 下降头名换成 interp **1.367 ms**（线程 1.562），然后 post 0.148、record 0.040。对应合约额外线程头名是 `0x209c4784ab1e8183cf58ca33cb740efbf3fc18ef`，**1.644 ms**。

## D. 超订和切换

8 worker 钉在 4 个核上的对照这次没有跑。1:1 钉核时，C=1 两块 OCC 和 SF 的 `nivcsw` 中位都是 0，`nvcsw` 中位也是 0。C=4 大块 OCC 的块内 `nvcsw` 中位 69.5、`nivcsw` 1.0；SF 是 28.5 和 5.5。这是整块成功与失败 attempt 的合计，不是每笔。非自愿切换不是 C=4 SF 墙 17 ms 的主体。`perf` 没开，`perf_ok` 比例是 0。jemalloc 只在 `global-alloc` feature 里，这次没开。mimalloc 不是工作区依赖，没有加。

## E. 分解

`wall = F + Ideal_tx + inflation + schedule_loss`。`inflation = Ideal_par − Ideal_tx`。`schedule_loss = 关旗墙 − F − Ideal_par`。F 是探针边界的 `f_pre + f_post`。Ideal 用探针每笔代价。

### 块 15274915

| C | 引擎 | 关旗墙 | F | Ideal_tx | Ideal_par | inflation | schedule | inflation 占缺口 | schedule 占缺口 | 重执行线程 |
| ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | OCC | 6.093 | 1.006 | 4.977 | 4.977 | 0 | 0.109 | 0 | 1 | 0 |
| 1 | SF | 8.768 | 1.628 | 4.977 | 5.896 | 0.919 | 1.244 | 0.425 | 0.575 | 0 |
| 2 | OCC | 5.831 | 1.128 | 2.490 | 3.445 | 0.955 | 1.258 | 0.432 | 0.568 | 1.152 |
| 2 | SF | 11.026 | 1.327 | 2.490 | 3.371 | 0.881 | 6.329 | 0.122 | 0.878 | 11.063 |
| 4 | OCC | 4.818 | 1.238 | 1.292 | 2.418 | 1.125 | 1.161 | 0.492 | 0.508 | 1.634 |
| 4 | SF | 17.303 | 1.672 | 1.292 | 2.404 | 1.111 | 13.228 | 0.077 | 0.923 | 41.333 |

C=4 SF 的探针墙中位是 19.309 ms，关旗墙是 17.303 ms。上表的墙是关旗的。缺口按关旗墙减 `Ideal_tx` 再减探针 F。

C=4 按 Ideal 下降排的前三名，OCC 是 post、record、interp，SF 是 post、interp、pre。这三项的 Ideal 下降（SF 0.662+0.314+0.141 = 1.117 ms）和上表 inflation 1.111 ms 同量级。它们解释不了 13.228 ms 的 schedule loss。

### 块 3356896，C=4

| 引擎 | 关旗墙 | F | Ideal_tx | Ideal_par | inflation | schedule | inflation 占缺口 | 重执行线程 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| OCC | 1.055 | 0.381 | 0.121 | 0.316 | 0.195 | 0.358 | 0.353 | 0.206 |
| SF | 2.608 | 0.482 | 0.121 | 1.722 | 1.601 | 0.404 | 0.798 | 2.014 |

薄块 C=4 的 SF 缺口以 inflation 为主，头名相位是 interp。大块相反，以 schedule loss 和重执行线程为主。

## F. 硬件

4 核，无 SMT。钉核之后 C=1/2/4 都是 1 个工人对 1 个物理核。L1d 192 KiB、L2 8 MiB、L3 320 MiB。没有 perf 计数，不能把 cache miss 写成膨胀来源。

## G. seq 与 par

产品 `Pevm::execute`，关旗，4 工人钉在 0–3，每轮新 `Pevm` 后先并行再串行，N=10。

- 3356896：10/10 `seq=par ok`，`diverge=0`。
- 15274915：9/10 `seq=par ok`，iter 1 `seq!=par`。第一笔分叉 tx 91，seq gas 4801468、par gas 4874668，seq logs 6、par logs 0。产品没有改。这次扫描的 TPS 不走 `Pevm::execute`，不把这一次分叉算进墙。

## 活锁

`results/soft0-execute-inflation/scan/hang-c4.txt`：块 15274915、引擎 sf、workers=4、round=28、关旗、新 `Pevm`，约 400% CPU，780 s 没有返回，进程被杀掉。`wall-c4.jsonl` 保留 round 0–27。中位用这 28 轮。薄块 30 轮都返回了。

## Step-level offsets & Ideal_step

等待要落到「等到哪一步」，而不是只等到哪一笔结束。trace 是 OCC、workers=1、`SPECFENCE_STEP_TRACE=1`、K=3、钉 cpu 0。大块三轮墙 12.650 / 10.615 / 9.860 ms，薄块约 1.0–1.1 ms。这些墙含 inspector，不进 TPS。

访问记在 SLOAD/SSTORE、CALL/CALLCODE 的余额、CREATE、SELFDESTRUCT，以及 `basic` / `storage` / `get_code_hash` 的宿主读。解释器结束后的写集里，sender 和 beneficiary 强制再记一笔（gas / nonce / 奖励），其它位置只在 opcode 没写过时补记。补记的 pc、opcode、code hash 置空，避免把结算记成最后一条 opcode。SSTORE 的时间在 `step_end`（指令刚结束）。带 value 的 CALL 在 opcode 入口记，不把被调用方的执行算进这次转账。

`w/dur`、`r/dur` 是 trace 时长的分数，三轮取中位。表里的纳秒是这个分数乘 OCC@1 的同钟代价。边在去掉 beneficiary 和 lazy 之后计数。

### 1. w/dur 和 r/dur

**块 15274915。** RAW 1221，WAW 165，WAR 159。把 beneficiary 和 lazy 加回去之后 WAW 变成 2436、WAR 1207、RAW 1223。lazy 写是大头。

| 类 | 端 | n | mean | stdev | p50 | p90 |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| RAW | w/dur | 1221 | 0.702 | 0.076 | 0.676 | 0.811 |
| RAW | r/dur | 1221 | 0.451 | 0.094 | 0.466 | 0.524 |
| WAW | w/dur | 165 | 0.778 | 0.169 | 0.823 | 0.949 |
| WAW | r/dur（第一次写） | 165 | 0.772 | 0.181 | 0.824 | 0.953 |
| WAR | w/dur | 159 | 0.828 | 0.114 | 0.830 | 0.960 |
| WAR | r/dur | 159 | 0.348 | 0.185 | 0.316 | 0.641 |

**脊 `abd6bb3978815b97`。** 76 条 WAW、76 条 RAW、76 条 WAR，对应 77 个写者。round 0 上这个哈希的 154 次访问全是 kind=basic、opcode=0、pc=0：探针看到的是宿主账户读写，不是这个哈希上的 SLOAD/SSTORE/CALL。最后一次写的 w/dur 均值 **0.806**（stdev 0.047，p50 0.814）。RAW 的第一次读均值 **0.308**（stdev 0.032，p50 0.316）。WAW 后继的第一次写均值也是 **0.806**。所以脊上的 WAW lag 接近 0：两边都落在交易后段的宿主结算，而不是一笔中途写、下一笔开头读。

**其它热位置（大块，去掉 beneficiary/lazy 的边）：**

| 位置 | 类 | n | w/dur mean | r/dur mean |
| --- | --- | ---: | ---: | ---: |
| `bef034365ca24581` | WAW | 6 | 0.681 | 0.685 |
| `bef034365ca24581` | RAW | 8 | 0.639 | 0.432 |
| `bef034365ca24581` | WAR | 13 | 0.766 | 0.456 |
| `d836a55a84878178` | WAW | 3 | 0.953 | 0.955 |
| `d836a55a84878178` | RAW | 4 | 0.953 | 0.182 |
| `930831d7501a43bf` | WAW | 1 | 0.939 | 0.957 |
| `930831d7501a43bf` | RAW | 2 | 0.939 | 0.309 |

按边数排在脊前面的是 `7ec8be01af547316`（996 条，全是 RAW）。它不在点名的热前缀里。完整分布在 `C1.json` 的 `step.top_locations_by_edges`。

**块 3356896。** 没有 `abd6bb397881`。边数最多的是 `dff71d59d972d654`（48 条，含 WAW/RAW/WAR）。RAW 110 条，w/dur 均值 0.656、r/dur 0.371。WAW 23 条，w/dur 0.561、r/dur 0.554。

### 2. Ideal_step(C)、Ideal_tx(C)、LB、L_step

列表调度：工人忙到该笔结束；后继的最早开始是 `start_j + (w_j − r_i)`，对所有前驱取最大。无限核最早完成时间是 `L_step`。C=1 时工人被整笔占住，makespan 等于 Σwork，所以 `Ideal_step(1) = Ideal_tx(1)`。

列表调度不是最优调度。步级约束比「等整笔结束」松，但优先级用的是步级最早完成，排出来的 makespan 可以长过 tx 级列表调度。下表把两个都写上。`L_step` 才是无限核下沿。

**块 15274915，主方案 RAW+WAW，去掉 beneficiary 和 lazy。** `L_step` = **0.687698 ms**。含 WAR 的 tx 级 `L_crit` = **1.292296 ms**。只含 RAW+WAW 的 tx 级 `L_crit` = **0.683821 ms**。历史 1.19 ms 不是这只钟。

| C | Ideal_tx ms | Ideal_step ms | 保守 WAR 的 Ideal_step ms | LB ms | L_step ms |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 4.977 | 4.977 | 4.977 | 4.977 | 0.688 |
| 2 | 2.490 | 2.837 | 2.777 | 2.489 | 0.688 |
| 4 | 1.292 | 1.524 | 1.514 | 1.292 | 0.688 |

把 beneficiary 和 lazy 加回去之后，无限核 `L_step` 变成 **1.295 ms**（`all_raw_waw`），C=4 的列表 makespan 变成 **2.089 ms**。保守 WAR 没有把 `L_step` 再拉长（仍是 0.688 ms），C=4 列表 makespan 是 1.514 ms。两条 WAR 方案的调度都没有环。

**构成 `L_step` 的边**（大块，终于 tx 102）。lag 是同钟纳秒。opcode 0 且 pc 0 是宿主结算，不是一条 EVM 指令。code hash 写前 16 个十六进制字符，全长在 `C1.json` 的 `step.ideal_step.raw_waw.path`。

| 类 | tx | 位置 | lag ns | 前驱分数 | 后继分数 | 前驱 pc / op / 下标 / selector | 后继 pc / op / 下标 / selector / code |
| --- | --- | --- | ---: | ---: | ---: | --- | --- |
| RAW | 5→49 | `111015025e6393bd` | 168124 | 0.969 | 0.113 | 宿主 / `c04b8d59` | pc 2371 / SLOAD / 1886 / `5ae401dc` / `aacbe6bdfb5697db` |
| WAW | 49→59 | `b6f13759584cd118` | 109783 | 0.921 | 0.877 | pc 2331 / SSTORE / 17398 / `5ae401dc` / `d0a06b12ac47863b` | pc 2675 / SSTORE / 4958 / `18cbafe5` / 同 code |
| RAW | 59→91 | `bef034365ca24581` | 13897 | 0.947 | 0.451 | 宿主 / `18cbafe5` | pc 4361 / EXTCODESIZE / 5062 / `5ae401dc` / `6ec798e80f3a19de` |
| RAW | 91→92 | `bef034365ca24581` | 65526 | 0.961 | 0.601 | 宿主 / `5ae401dc` | pc 8468 / CALL / 3818 / `18cbafe5` / `5b83bdbcc56b2e63` |
| RAW | 92→94 | `bef034365ca24581` | 43553 | 0.949 | 0.199 | 宿主 / `18cbafe5` | pc 9114 / EXTCODESIZE / 1831 / `7ff36ab5` / `d6828dea5ec4e24c` |
| RAW | 94→98 | `930831d7501a43bf` | 77724 | 0.939 | 0.090 | 宿主 / `7ff36ab5` | pc 3473 / SLOAD / 656 / `7ff36ab5` / `4acfec2f4d266e75` |
| RAW | 98→101 | `bef034365ca24581` | −30689 | 0.213 | 0.298 | pc 9129 / CALL / 1843 / `7ff36ab5` / `d6828dea5ec4e24c` | pc 15413 / CALL / 5183 / `5ae401dc` / `54f2b4c90d293926` |
| RAW | 101→102 | `111015025e6393bd` | 143480 | 0.969 | 0.163 | 宿主 / `5ae401dc` | pc 2371 / SLOAD / 1760 / `5ae401dc` / `aacbe6bdfb5697db` |

脊不在这条路径上。负 lag 的那一跳（98→101）是 CALL 写在前驱的 0.213 处，后继的 CALL 读在 0.298 处。

**块 3356896。** `L_step` = **0.018645 ms**。tx 级 `L_crit` = **0.073964 ms**（含 WAR 与不含 WAR 相同）。路径两条 RAW：14→16 位置 `1ea035099ca379c2`，lag 4505 ns，前驱宿主、selector `a9059cbb`，后继 pc 2553 / SLOAD / 下标 201 / code `74723e26c5dc07ec`；16→17 位置 `348f6446dd3e2f97`，lag 6486 ns，后继是宿主读（pc 0）。C=4 的 `Ideal_step` 0.131 ms，`Ideal_tx` 0.121 ms，LB 0.120 ms。

### 3. TPS_ideal_step 在扫描 JSON 里

`scripts/specfence_inflation_report.py --step-trace` 把下列字段写进每个 `C*.json` 的块，并写进 `curves.json`：`tps_ideal_tx`、`tps_ideal_step`、`ideal_step_ms`、`l_step_ms`、`proximity_occ_step`、`proximity_sf_step`，以及 `step` 里的分布、脊、热位置、分组和四套调度（`raw_waw`、`raw_waw_war`、`all_raw_waw`、`all_raw_waw_war`）。`scripts/soft0_percore_scan.sh` 先在 1 个物理核上收集 `step-trace.jsonl`，再在每个 C 的 JSON 里引用它。模拟不需要重跑 EVM。

C=4 大块：`TPS_ideal_step` 804338，`TPS_ideal_tx` 948699，`TPS_OCC` 254484，`TPS_SF` 70865。步级列表调度的 TPS 低于 tx 级列表调度，因为 makespan 是 1.524 ms 而不是 1.292 ms。无限核 `L_step` 更短，没有被这个 C=4 包装出来。

### 4. 偏移能不能做成跨块先验

分组键是（code hash，函数 selector，键类，端点）。键类在热前缀上用位置名，否则用 basic / storage / code / lazy。只保留组内至少 2 个样本的前 40 组。`stdev` 是组内各笔中位分数的总体标准差。`round_stdev_mean` 是同一笔、同一位置在 3 轮 trace 之间的标准差的平均。

**组内紧、轮次也紧的例子（大块）：** 脊的 WAW 写端，n=76，均值 0.806，组内 stdev 0.047，轮次 stdev 均值 0.033。selector `c04b8d59` 的 storage RAW 写端，n=25，均值 0.973，stdev 0.002，轮次 stdev 均值 0.012。

**组内散的例子：** code `aacbe6bdfb5697db`、selector `5ae401dc`、storage、RAW 读端，n=15，均值 0.325，stdev 0.185。一个常数先验盖不住。

**两块都出现的组**（`offset_groups_across_blocks`）：

| code hash | selector | 键类 | 端 | n | 大块均值 | 薄块均值 | 绝对差 | 大块 stdev | 薄块 stdev |
| --- | --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 空（宿主） | `00000000` | basic | RAW 写 | 1044 / 89 | 0.679 | 0.681 | 0.002 | 0.016 | 0.052 |
| 空（宿主） | `00000000` | basic | RAW 读 | 1046 / 88 | 0.466 | 0.345 | 0.122 | 0.052 | 0.073 |
| 空 | `a9059cbb` | storage | WAW 写 | 10 / 3 | 0.864 | 0.848 | 0.016 | 0.019 | 0.018 |

宿主 basic 的最后写，两块均值差 0.002，块内 stdev 也小，跨块先验说得通。同一批宿主读差 0.122，说不通。`a9059cbb` 的 storage 写差 0.016，但薄块只有 3 个样本。脊的 0.81 只在 15274915 里，3356896 的热位置是 `dff71d59d972d654`，这个先验不能跨块搬。

## 这次没有测的

- C>4。本机 4 个物理核，脚本会跳过。
- 8-on-4 超订。
- `SPECFENCE_INFLATION_READS`、`SPECFENCE_INTERP_SPLIT`、`SPECFENCE_INFLATION_PERF`。所以 mv 查找、解释器内部分裂和 cache miss 没有数。
- 同一 `Pevm` 复用的 oracle 行。C=4 的 meta 写了 `oracle_k=10`，文件里只有 timed 行；活锁发生在 oracle 之前。
- profile-c4 的原始 jsonl 约 268MB（每行带完整 attempt）。提交的是算好的 `C4.json`。同钟代价用的是 `profile-c1.jsonl`。
