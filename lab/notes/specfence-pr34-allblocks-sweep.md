# PR #34 全量扫块（全部可加载 ethereum 快照）

**基线:** `cursor/specfence-prepaid-losers-b5de` @ `0144211ae115c87eb2e80828e17e0750d3e2cf6b`
**分析分支:** `cursor/specfence-allblocks-deepdive-bc5e`
**性质:** 分析 / 文档 / 结果；**不改** CC / policy / learn
**Soft=0 · OptimisticRead / OrderedAdmit · 不发明 ns**
**本盒:** 4 物理核；harness `cores=8`（与 PR34 land 同口径，本机超订）
**PRIMARY 墙:** Instant-off；reuse = 同一 SpecFence `Pevm` 的 iter1..2 median

## 0. 一句话

99/99 可加载快照（空块 19910734 除外 98 行）Soft=0 N=3 reuse：median SF/OCC wall **1.14×**，p90 **3.57×**。绝对墙与倍数榜都被 **n≥512 肥块（S）** 和附录 **near_independent_meta_gap** 占据；52 OCC-gap 集里的 3356896 只是 ~1.4× 的薄输，不是全集最慢。

## 1. 口径

| 项 | 值 |
|----|----|
| 二进制 | `specfence_all_blocks_sweep` release，LTO off |
| 集合 | `SPECFENCE_ALL_BLOCKS=all`（discover 全部 block 目录，**不是**默认 52 OCC-gap） |
| reuse | `SPECFENCE_ALL_REUSE=1` · `SPECFENCE_ALL_ITERS=3` |
| process-trace | 关（`PROCESS_TOP=0`） |
| Soft | **0**（98/98 行 `soft_wait_arms=0`） |
| 原始 JSON | `lab/results/pr34-allblocks-reuse-n3.json`（gitignore） |
| 本摘要 | `lab/notes/specfence-pr34-allblocks-sweep-summary.json` |

覆盖：candidates **99** · loaded **99** · skipped **0** · 空块 **19910734** · 可用 **98** · 其中 OCC-gap 52 集 **52** · 集外 **46**。

## 2. 分布

| 指标 | median | p90 | max | mean |
|------|-------:|----:|----:|-----:|
| OCC wall ms（3-iter median） | 3.845 | 8.550 | 16.900 | 4.157 |
| SF wall ms（含冷） | 4.084 | 21.241 | 106.041 | 9.696 |
| SF reuse ms | 4.933 | 22.769 | 188.197 | 13.269 |
| SF/OCC（all median） | 1.139 | 3.313 | 27.452 | 2.060 |
| SF reuse / OCC all | 1.320 | 3.896 | 30.881 | 2.523 |

SF≤OCC wall：**22/98**；SF reuse≤OCC reuse：**30/98**（reuse/reuse 排除 spike 后见 JSON）。

对照 PR34 land（当时只跑 52 集 N=3）：SF≤OCC wall 11/52、reuse 16/52。全集把附录 meta-gap / WAW_spine / 小块也算进来，赢的行数变多、median 比被小块拉低，**不能**据此说「全集比 52 集更健康」——最慢尾在变肥。

## 3. 噪声 / spike（N=3 仍不够）

| block | 现象 | 处理 |
|------:|------|------|
| **2179522** | SF all 4.16 但 reuse **188**；OCC all 14.4 / reuse 79.7。历史 Bind 风暴 + 近独立 meta-gap | **不进绝对墙主榜**；深挖若做只当噪声样本 |
| **19434587** | OCC reuse **2310**（单 iter 病理），SF 稳定 ~22 | OCC 侧噪声；SF/OCC 比不可用 reuse/reuse |
| 13217637 | OCC reuse/all=2.2（4.4→9.7），SF 稳定 68–74 | 绝对墙仍稳；倍数用 all-median |
| 4370000 | SF reuse/all=2.3（2.73→6.33） | 小块噪声 |

规则：主榜用 **SF all-median 墙** 与 **SF/OCC all-median 比**；reuse 墙并列，遇 `*_spike` 降权。

## 4. 最慢榜 A — 绝对 SF wall（all-median）

| block | n_tx | OCC wall | SF | ratio | arm | unf | dp | w_need | Soft | morph | class | spike |
|---:|---:|---:|---:|---:|---|---:|---:|---:|---:|---|---|---|
| 14396881 | 1346 | 3.863 | 106.041 | 27.45 | Opt→Win_1 | 2 | 0 | 1 | 0 | near_independent_meta_gap | O,S |  |
| 15274915 | 1226 | 4.924 | 79.337 | 16.11 | Opt→Full | 3 | 0 | 1 | 0 | mixed_RAW_WAW | S |  |
| 13217637 | 1100 | 4.436 | 68.202 | 15.37 | Opt→Full | 9 | 0 | 0 | 0 | mixed_RAW_WAW | S | occ_reuse_spike |
| 19807137 | 712 | 16.900 | 60.292 | 3.57 | Full→Full | 187 | 0 | 0 | 0 | WAW_spine | S |  |
| 17666333 | 961 | 8.525 | 38.014 | 4.46 | Win_1→Win_1 | 3 | 0 | 1 | 0 | mixed_RAW_WAW | O,S |  |
| 15538827 | 823 | 5.987 | 29.425 | 4.91 | Full→Full | 13 | 0 | 0 | 0 | mixed_RAW_WAW | S |  |
| 14334629 | 819 | 5.371 | 25.133 | 4.68 | Win_1→Win_1 | 8 | 0 | 0 | 0 | mixed_RAW_WAW | S |  |
| 19606599 | 367 | 13.094 | 22.944 | 1.75 | Opt→Full | 40 | 0 | 0 | 0 | mixed_RAW_WAW | - |  |
| 19716145 | 341 | 9.532 | 21.872 | 2.29 | Full→Full | 22 | 0 | 0 | 0 | mixed_RAW_WAW | - |  |
| 19434587 | 390 | 12.896 | 21.614 | 1.68 | Opt→Opt | 68 | 0 | 0 | 0 | mixed_RAW_WAW | - | occ_reuse_spike |
| 15199017 | 866 | 4.112 | 21.241 | 5.17 | Full→Win_1 | 5 | 0 | 0 | 0 | mixed_RAW_WAW | S |  |
| 14383540 | 722 | 5.593 | 21.088 | 3.77 | Full→Win_1 | 13 | 0 | 0 | 0 | mixed_RAW_WAW | - |  |
| 14545870 | 456 | 7.146 | 18.696 | 2.62 | Opt→Opt | 27 | 0 | 0 | 0 | mixed_RAW_WAW | - |  |
| 19860366 | 430 | 10.337 | 18.684 | 1.81 | Full→Full | 25 | 0 | 0 | 0 | mixed_RAW_WAW | - |  |
| 14683600 | 660 | 8.570 | 18.253 | 2.13 | Opt→Opt | 27 | 0 | 0 | 0 | mixed_RAW_WAW | - |  |

并列：SF **reuse** 墙（含 spike）

| block | n_tx | OCC wall | SF | ratio | arm | unf | dp | w_need | Soft | morph | class | spike |
|---:|---:|---:|---:|---:|---|---:|---:|---:|---:|---|---|---|
| 2179522 | 222 | 14.362 | 188.197 | 13.10 | Opt→Opt | 14 | 0 | 0 | 0 | near_independent_meta_gap | - | occ_reuse_spike,sf_reuse_spike |
| 15274915 | 1226 | 4.924 | 152.046 | 30.88 | Opt→Full | 3 | 0 | 1 | 0 | mixed_RAW_WAW | S |  |
| 14396881 | 1346 | 3.863 | 110.096 | 28.50 | Opt→Win_1 | 2 | 0 | 1 | 0 | near_independent_meta_gap | O,S |  |
| 19807137 | 712 | 16.900 | 76.731 | 4.54 | Full→Full | 187 | 0 | 0 | 0 | WAW_spine | S |  |
| 13217637 | 1100 | 4.436 | 74.019 | 16.68 | Opt→Full | 9 | 0 | 0 | 0 | mixed_RAW_WAW | S | occ_reuse_spike |
| 17666333 | 961 | 8.525 | 40.405 | 4.74 | Win_1→Win_1 | 3 | 0 | 1 | 0 | mixed_RAW_WAW | O,S |  |
| 15538827 | 823 | 5.987 | 29.608 | 4.95 | Full→Full | 13 | 0 | 0 | 0 | mixed_RAW_WAW | S |  |
| 19606599 | 367 | 13.094 | 29.380 | 2.24 | Opt→Full | 40 | 0 | 0 | 0 | mixed_RAW_WAW | - |  |
| 19716145 | 341 | 9.532 | 25.842 | 2.71 | Full→Full | 22 | 0 | 0 | 0 | mixed_RAW_WAW | - |  |
| 14334629 | 819 | 5.371 | 25.430 | 4.73 | Win_1→Win_1 | 8 | 0 | 0 | 0 | mixed_RAW_WAW | S |  |
| 19434587 | 390 | 12.896 | 22.769 | 1.77 | Opt→Opt | 68 | 0 | 0 | 0 | mixed_RAW_WAW | - | occ_reuse_spike |
| 15199017 | 866 | 4.112 | 22.474 | 5.47 | Full→Win_1 | 5 | 0 | 0 | 0 | mixed_RAW_WAW | S |  |
| 14383540 | 722 | 5.593 | 22.119 | 3.95 | Full→Win_1 | 13 | 0 | 0 | 0 | mixed_RAW_WAW | - |  |
| 14683600 | 660 | 8.570 | 22.045 | 2.57 | Opt→Opt | 27 | 0 | 0 | 0 | mixed_RAW_WAW | - |  |
| 19860366 | 430 | 10.337 | 20.683 | 2.00 | Full→Full | 25 | 0 | 0 | 0 | mixed_RAW_WAW | - |  |

## 5. 最慢榜 B — SF/OCC 比（all-median）

| block | n_tx | OCC wall | SF | ratio | arm | unf | dp | w_need | Soft | morph | class | spike |
|---:|---:|---:|---:|---:|---|---:|---:|---:|---:|---|---|---|
| 14396881 | 1346 | 3.863 | 106.041 | 27.45 | Opt→Win_1 | 2 | 0 | 1 | 0 | near_independent_meta_gap | O,S |  |
| 15274915 | 1226 | 4.924 | 79.337 | 16.11 | Opt→Full | 3 | 0 | 1 | 0 | mixed_RAW_WAW | S |  |
| 13217637 | 1100 | 4.436 | 68.202 | 15.37 | Opt→Full | 9 | 0 | 0 | 0 | mixed_RAW_WAW | S | occ_reuse_spike |
| 15199017 | 866 | 4.112 | 21.241 | 5.17 | Full→Win_1 | 5 | 0 | 0 | 0 | mixed_RAW_WAW | S |  |
| 15538827 | 823 | 5.987 | 29.425 | 4.91 | Full→Full | 13 | 0 | 0 | 0 | mixed_RAW_WAW | S |  |
| 14334629 | 819 | 5.371 | 25.133 | 4.68 | Win_1→Win_1 | 8 | 0 | 0 | 0 | mixed_RAW_WAW | S |  |
| 17666333 | 961 | 8.525 | 38.014 | 4.46 | Win_1→Win_1 | 3 | 0 | 1 | 0 | mixed_RAW_WAW | O,S |  |
| 14383540 | 722 | 5.593 | 21.088 | 3.77 | Full→Win_1 | 13 | 0 | 0 | 0 | mixed_RAW_WAW | - |  |
| 14029313 | 724 | 3.845 | 14.420 | 3.75 | Opt→Opt | 15 | 0 | 0 | 0 | mixed_RAW_WAW | - |  |
| 19807137 | 712 | 16.900 | 60.292 | 3.57 | Full→Full | 187 | 0 | 0 | 0 | WAW_spine | S |  |
| 16146267 | 473 | 3.776 | 12.513 | 3.31 | Opt→Opt | 27 | 0 | 0 | 0 | mixed_RAW_WAW | - |  |
| 14545870 | 456 | 7.146 | 18.696 | 2.62 | Opt→Opt | 27 | 0 | 0 | 0 | mixed_RAW_WAW | - |  |
| 8889776 | 330 | 2.545 | 5.988 | 2.35 | Opt→Opt | 30 | 0 | 0 | 0 | mixed_RAW_WAW | - |  |
| 19716145 | 341 | 9.532 | 21.872 | 2.29 | Full→Full | 22 | 0 | 0 | 0 | mixed_RAW_WAW | - |  |
| 13287210 | 1414 | 4.050 | 9.081 | 2.24 | Opt→Opt | 3 | 0 | 0 | 0 | near_independent_meta_gap | - |  |

稳定子集（无 spike flag）绝对墙 Top 与倍数 Top 高度重叠：`14396881, 15274915, 13217637, 19807137, 17666333, 15538827, 14334629, 15199017`。

## 6. 输家类（PR34 启发式，last-iter 遥测）

| 类 | 规则 | 本扫命中 |
|----|------|--------:|
| **U** | last arm `Win_2*` 且 unf≥8 | 5 |
| **O** | covering>0 且 ratio≥1.5 且 unf<8 | 2 |
| **D** | double_pay>0 | 6 |
| **L4** | sys>0 且 last Opt/Defer 且 unf≥20 | 0 |
| **S** | n≥512 且 ratio≥4 | 8 |
| 未贴以上标签 | — | 82 |

S 命中（稳定主名单）：15274915、14396881、13217637、17666333、15538827、14334629、15199017，以及 spike 的 2179522 若按 reuse 比也会撞 S（不采用）。

U 仍在：last `Win_2` + 大 unf 的中块（详见 JSON `loser_class`）。L4 本扫 last-iter 为 0（sys_reexec 几乎被预付刀吃掉）。

## 7. 形态（历史 DAG 标签，非本跑重算）

最慢稳定 8 块：

| block | morph | 在 52 集? | L | W | RAW | WAW | bound@8 |
|------:|--------|:--:|--:|--:|----:|----:|--------:|
| 14396881 | near_independent_meta_gap | N | 5 | 1337 | 0 | 13 | 8.0 |
| 15274915 | mixed_RAW_WAW | Y | 77 | 1121 | 35 | 120 | 8.0 |
| 19807137 | WAW_spine | N | 571 | 106 | 9 | 628 | 1.246935 |
| 17666333 | mixed_RAW_WAW | Y | 32 | 897 | 18 | 122 | 8.0 |
| 15538827 | mixed_RAW_WAW | Y | 35 | 696 | 62 | 147 | 8.0 |
| 14334629 | mixed_RAW_WAW | Y | 28 | 734 | 36 | 139 | 8.0 |
| 19606599 | mixed_RAW_WAW | Y | 57 | 261 | 42 | 177 | 6.438596 |
| 19716145 | mixed_RAW_WAW | Y | 46 | 226 | 51 | 285 | 7.413043 |

全集最慢绝对墙 **14396881** 是附录 `near_independent_meta_gap`（L=5, W=1337, RAW=0, WAW=13）——52 集故意排除。**跑全集的意义：** 预付刀没有修复「近独立肥块上 SF 调度壳」；S 类混合块 15274915 / 13217637 仍 15–27×。

## 8. 命名锚点（对照 PR34 land）

| block | 本扫 OCC / SF / reuse / ratio | land 52 集笔记 |
|------:|---|---|
| 3356896 | 0.818 / 1.145 / 1.247 / 1.40 · Win_1→Win_2 unf=0 | 薄输；Win_2 cover unf=0 |
| 14689597 | 5.837 / 12.503 / 12.893 / 2.14 · Opt→Opt unf=86 | L4 曾 unf 336→139；本扫 last Opt unf=86 ratio 2.14 |
| 15274915 | 4.924 / 79.337 / 152.046 / 16.11 · Opt→Full unf=3 | S 仍肥 |
| 13217637 | 4.436 / 68.202 / 74.019 / 15.37 · Opt→Full unf=9 | S 仍肥 |
| 19469097 | 7.145 / 12.795 / 13.491 / 1.79 · Full→Win_1 unf=54 | 不再 OOM；ratio ~1.8 |

## 9. Soft=0 确认

全部 99 对 `soft_wait_arms=0`。

## 10. 深挖名单（K=8，稳定绝对墙）

`14396881, 15274915, 19807137, 17666333, 15538827, 14334629, 19606599, 19716145`

见 `lab/notes/specfence-pr34-slowest-deepdive.md`。
