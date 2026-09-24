# Soft=0：`run_pevm_tx` 的差在 Detect-on-kept，不在操作码

**日期：** 2026-09-24（北京时间）
**探针：** `3421771`。只加时钟。`SPECFENCE_INTERP_SPLIT=1` 时，每次 `run_pevm_tx` 分成 opcode、VmDb、Detect-on-kept。不改 pick、验证、发布，也不缩短 VmDb 或 Detect。
**块：** 15274915 为主。3356896 只作对照。
**协议：** Soft=0 Instant-off，release，LTO off，请求 8 核，宿主 4 核，`taskset -c 0-3`。大块 N=5，薄块 N=3。复用中位是四次复用墙的 `sorted[2]`（两次复用时是较慢的那次）。冷启动（iter 0）不进中位。`SPECFENCE_COMPARE_CHECK=1`。

## 结论

**差在 Detect-on-kept，不在 opcode。** 大块四次复用里，opcode 的线程差是 −0.83 到 +0.35 ms，有三次 SpecFence 的 opcode 线程更短。Detect 每次都是正的，+1.18 到 +4.18 ms 线程。VmDb 的线程差正负都有，按每次 `run_pevm_tx` 算只多大约 0.3–0.6 µs。

因此设计里的产品切仍是 **CONDITIONAL**：Δ 落在 SpecFence 多付的 Detect（附带一小段 VmDb），不是 opcode。本 PR **不下刀**。就算把中位那一轮的 Detect 线程（1.75 ms）摊进 4 核，墙上界大约 0.44 ms，ratio 仍大约 0.80。贴齐 OCC 也只是 1。`ge_1_5=false`。

opcode 不是这道差。不停在「两边操作码地板」上，但也不要在这轮把 Detect 热路径改短。

## 时钟怎么分

`opcode + vmdb + detect + other = interp`（`phase_interp_ns`）。四次复用每一行都对得上。`other` 恒为 0：`run_pevm_tx` 里扣掉 VmDb 方法之后，剩下的就是 revm 解释器核（含不进 VmDb 的 journal 写准备）。写集提交仍在外层 `post`。`finish_execution` 仍是外层 `finish_*`，不并进这三个桶。

- **opcode：** `run_pevm_tx` 减去下面两段。
- **VmDb：** `basic` / `storage` / `code_by_hash` / `block_hash` 的方法时间，再减去 Detect。
- **Detect-on-kept：** 通过早期返回之后的 spine peek，以及 `wait || crit` 之后的 WaitOnce consult。OCC 在 `mode != SpecFence` 处返回，Detect 为 0。首次 Opt 上已经跳过的冷读不进这个桶。

比较例程自己打开 `SPECFENCE_INTERP_SPLIT`。库默认关，未设环境变量时不在每次读上打 `Instant`。

## 这轮墙，不替换锁定带

锁定带仍是 QuietExit 的 SF **7.065** / OCC **5.306**，以及无探针 FirstExecCut 的 **7.210** / **5.900**。差约 **1.3–1.9 ms**。同 EVM 诊断中位 8.020 / 5.823 也不替换它。

本探针每次 VmDb 方法打 `Instant`，Detect 段再打一次。大块复用中位：

| 项 | 值 |
| --- | ---: |
| SF / OCC | **7.687 / 5.795 ms** |
| 差 | **1.892 ms** |
| ratio（OCC/SF） | **0.754** |
| TPS SF / OCC | **159486 / 211546** |
| `ge_1_5` | **false** |
| `est` / `soft` / `occ_picks` | 0 / 0 / 0 |
| `spine_cores_max` / 链 | 1 / 77 |
| `seq≡par` | 是 |

7.687 高于锁定带上沿 7.2，也高于无探针 7.210。OCC 5.795 落在 5.3–5.9 里。这是探针轮的墙，不是新的 Soft=0 带。四次复用 SF 墙是 7.400 / 7.567 / 7.687 / 9.152。OCC 是 5.643 / 5.793 / 5.795 / 6.763。

## 大块复用

块长 1226。冷启动 SF 墙 34.667 ms，Detect 221 ms 线程，`yield_deadlock=232`。那是第一遍 WaitOnce，下面只看四次复用。`n` 是进入 `run_pevm_tx` 的次数（`split_n`）。`ok` 是 `vm.execute` 成功次数。

| iter | 墙 SF / OCC | SF n / ok | OCC n / ok | SF abort | OCC abort |
| --- | ---: | ---: | ---: | ---: | ---: |
| 1 | 9.152 / 5.795 | 1271 / 1242 | 1960 / 1316 | 14 | 90 |
| 2（SF 中位墙） | 7.687 / 5.793 | 1237 / 1232 | 1386 / 1301 | 6 | 75 |
| 3 | 7.567 / 5.643 | 1264 / 1235 | 1363 / 1312 | 9 | 86 |
| 4 | 7.400 / 6.763 | 1235 / 1232 | 1709 / 1352 | 4 | 126 |

OCC 成功次数仍然更多，abort 也更多。OCC 更早不是因为 SpecFence 多跑了一遍块。

线程时间（毫秒）。差是 SF − OCC。`/4` 只是「多出来的线程时间若摊在 4 个满核上」的上界，不是另一条墙。

| iter | 墙差 | opcode | VmDb | Detect | 合计 interp | interp/4 | finish |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | +3.357 | +0.35 | −0.69 | **+4.18** | +3.83 | +0.96 | +1.56 |
| 2 | +1.894 | −0.83 | +0.55 | **+1.75** | +1.47 | +0.37 | +0.64 |
| 3 | +1.924 | −0.36 | +0.45 | **+4.17** | +4.26 | +1.07 | +0.72 |
| 4 | +0.637 | −0.82 | −0.31 | **+1.18** | +0.05 | +0.01 | +0.60 |

每次 `run_pevm_tx`（微秒，SF − OCC）：

| iter | opcode | VmDb | Detect | interp |
| --- | ---: | ---: | ---: | ---: |
| 1 | +1.54 | +0.35 | **+3.29** | +5.18 |
| 2 | −0.23 | +0.61 | **+1.41** | +1.79 |
| 3 | −0.02 | +0.45 | **+3.30** | +3.73 |
| 4 | +0.43 | +0.37 | **+0.96** | +1.76 |

中位墙那一轮（iter 2）SpecFence 自己的一次调用是 opcode **3.41** / VmDb **1.96** / Detect **1.41** µs。OCC 是 opcode **3.64** / VmDb **1.35** / Detect **0**。操作码单次 SpecFence 更便宜。多出来的是 Detect，加上大约 0.6 µs 的 VmDb。

四次都成立的是：

- **Detect 是唯一每次都为正、而且到毫秒线程的桶。** 复用上 `yield_deadlock=0`，`yield_ok` 是 7 / 1 / 4 / 2。这段不是冷启动那种长自旋。`detect_n` 是 715 / 552 / 723 / 570，大约每两次调用有一段 kept Detect。首次 Opt 的旧探针 `cut_keep_n` 只有 9–53：那只数已经跳过门里仍保留的冷读。本桶还包含链上和后续 incarnation 的全 Detect。
- **opcode 解释不了正差。** 三次线程差为负。单次差没有稳定的正号。
- **VmDb 是小头。** 线程差 −0.69 到 +0.55 ms。单次稳定地多 0.35–0.61 µs，因为 OCC 的 `vmdb_n` 更多（3640–4244 对 3305–3538），总线程不一定更多。
- **finish 仍小。** 复用多 0.60–1.56 ms 线程，大约 0.15–0.39 ms 墙。外层桶，不是这把刀。

iter 2 把解释器差摊到 4 核大约 **0.37 ms** 墙，实测墙差 **1.89 ms**。Detect 是解释器差的主体，不是整段墙差。pre / post / 验证仍在解释器外面。iter 4 的解释器线程几乎打平（+0.05 ms），墙差仍有 0.64 ms，同一句话：解释器差不是墙差的全部。

## 和纸面天花板

纸面把解释器差的墙上界放在 **0.7–1.2 ms**，并写明即使全砍，ratio 朝 1 而不是 1.5。本探针的解释器线程差是 +0.05 到 +4.26 ms，`/4` 是 +0.01 到 +1.07 ms。高的那两次（iter 1、3）落在这条天花板里；中位墙那次只有大约 0.37 ms。同号，但不是每一轮都顶满 1.2 ms。

中位墙若只减去 Detect 的 1.75 ms 线程 / 4 ≈ 0.44 ms，SF 大约 7.25 ms，对 OCC 5.795，ratio 大约 **0.80**。仍低于 1.5。这是上界形状，不是已经落地的切。

## 薄块

3356896，N=3。复用中位取较慢的那次：SF **2.108** / OCC **1.207**，差 **0.901 ms**，ratio **0.573**。`est=0`，`soft=0`，`occ_picks=0`，`spine_cores_max=1`，链 17。`seq=par ok`。

这一行（较慢复用）`run_pevm_tx` 线程差是 **−0.04 ms**。单次差 opcode −0.09 / VmDb +0.31 / Detect +0.10 µs。墙差 0.90 ms 不在解释器里。另一次复用单次 opcode 多 1.45 µs，但线程只多 0.18 ms，而且那次 SF 墙更短（1.664）。薄块不改大块的 Detect 判语，也对不上大块那 1–3 µs 的 Detect 形状。

## 判语

| 问题 | 答案 |
| --- | --- |
| Δ 主要在哪 | **Detect-on-kept**。VmDb 单次只多不到 1 µs。opcode 不是正差 |
| 产品切 | **CONDITIONAL 仍在，本 PR 不切** |
| 若当时 opcode 主导 | 会停 Soft=0 元数据 / VmDb 刀。这次不是 |
| `ge_1_5` | **false**（0.754） |
| 锁定带 | 不替换。7.687 / 5.795 是探针墙 |
| 正确性 | 两块 `seq=par ok`。`occ_picks=0`，`spine_cores_max=1`，`est=0`，`soft=0` |

## 硬边界

无 Estimate 门，无 park-all，无 #47，无 QuietExit 再拧，无 FirstExecCut 式跳过，无加核。不宣称 ≥1.5。不把 7.687 / 5.795 或 8.020 / 5.823 写成锁定带。
