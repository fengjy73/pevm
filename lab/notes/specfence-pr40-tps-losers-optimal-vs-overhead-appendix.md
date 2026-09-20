# PR #40 K=11 Instant-off / DAG 附录

主读：[`specfence-pr40-tps-losers-optimal-vs-overhead.md`](specfence-pr40-tps-losers-optimal-vs-overhead.md)。  
机器 JSON：[`specfence-pr40-tps-losers-optimal-vs-overhead-summary.json`](specfence-pr40-tps-losers-optimal-vs-overhead-summary.json)。  
原始：`lab/results/pr40-k11-optimal-overhead/`（gitignore）。

约定：墙单位 ms；end_block 报 µs（`end_block_ns/1000`）。`yield_ns` 为 Instant-tax，不写入墙分解。Soft=0 全行。

---

## 14396881 — NEAR · 近独立

| | |
|--|--|
| n / gas | 1346 / 30,020,813 |
| DAG 有效 | L=5 W=1337 RAW=0 WAW=13 indep=0.989 maxW=5 |
| DAG+lazy | L=1197 maxW=1197 |
| bound@8 / 波 | 8.00 / 169 |
| serial / OCC@1 / OCC@8 fg | 2.64 / 8.82 / 5.95 |
| Instant-off OCC / SF reuse | 4.536 / 15.154（3.34×） |
| 臂 | Opt, Full, Full, Full, Full |
| wait / cover | 4,4,4,4,4 / 0 |
| ungated | 768,828,769,768,768 |
| end_block µs | 300, 223, 215, 281, 244 |
| unfenced / SF abort / OCC abort | 4–5 / 3–4 / 3–6 |
| begin_blocked 末 | `[24,32,764,1069]` |
| 热 | lazy 1197, lazy 60, Basic 5 |

## 15274915 — FAR · 错对象 Full

| | |
|--|--|
| n | 1226 |
| DAG 有效 | L=77 W=1121 RAW=35 WAW=120 indep=0.904 maxW=77（Basic） |
| DAG+lazy | L=997 |
| bound@8 / 波 | 8.00 / 154 |
| serial / OCC@1 / OCC@8 fg | 3.71 / 6.83 / 5.52 |
| Instant-off OCC / SF reuse | 5.349 / 15.647（2.93×） |
| 臂 | Full×5 · `Full/996`,`Full/51` |
| wait / cover | 1 / 0 |
| ungated | 117,178,136,122,113 |
| end_block µs | 291,245,218,235,429 |
| unfenced / SF abort / OCC abort | 18–68 / 10–62 / 64–73 |
| begin_blocked 末 | `[105]` |

## 13217637 — NEAR · 近独立 + wait-set=8

| | |
|--|--|
| n | 1100 |
| DAG 有效 | L=6 W=1060 RAW=21 WAW=44 indep=0.950 maxW=5 |
| DAG+lazy | L=934 |
| bound@8 / 波 | 8.00 / 138 |
| serial / OCC@1 / OCC@8 fg | 5.35 / 8.11 / 5.27 |
| Instant-off OCC / SF reuse | 5.615 / 14.841（2.64×） |
| 臂 | Opt×5 |
| wait / cover | 8 / 0 |
| ungated | 1105,1106,1105,1107,1102 |
| end_block µs | 339,328,319,488,336 |
| unfenced / SF abort / OCC abort | 11–14 / 4–8 / 4–19 |
| begin_blocked 末 | `[23,1023,1025,1062,1069,1073,1076,1097]` |

## 16146267 — FAR · storage-50

| | |
|--|--|
| n | 473 |
| DAG 有效 | L=50 W=375 RAW=11 WAW=123 indep=0.757 maxW=50（storage） |
| DAG+lazy | L=268 |
| bound@8 / 波 | 8.00 / 60 |
| serial / OCC@1 / OCC@8 fg | 5.82 / 10.58 / 4.66 |
| Instant-off OCC / SF reuse | 4.358 / 11.781（2.70×） |
| 臂 | Opt×5 |
| wait / cover | 8 / 0 |
| ungated | 51,99,251,70,102 |
| end_block µs | 1000,236,216,211,211 |
| unfenced / SF abort / OCC abort | 47–68 / 49–75 / 64–100 |
| begin_blocked 末 | `[17,19,31,34,38,39,43,64]` |

## 19807137 — FAR · 欠盖 storage-571

| | |
|--|--|
| n | 712 |
| DAG 有效 | L=571 W=106 RAW=9 WAW=628 indep=0.133 maxW=571 |
| bound@8 / 波 | **1.25** / 571 |
| serial | **2412 ms 病态** → t_work=OCC@1 16.78 |
| OCC@8 fg | 16.54（abort_rate 1.54，finegrain 税；主墙用 Instant-off） |
| Instant-off OCC / SF reuse | 18.015 / 44.977（2.50×）；OCC/理想 1.3× |
| 臂 | Opt×5 |
| wait / cover | 8 / 0 |
| ungated | 550,401,344,848,395 |
| end_block µs | 412,469,474,473,529 |
| unfenced / SF abort / OCC abort | 564–594 / 815–1108 / 826–1088 |
| begin_blocked 末 | `[1,54,94,97,98,109,120,133]` |

## 8889776 — FAR · storage-56 + 臂振荡

| | |
|--|--|
| n | 330 |
| DAG 有效 | L=56 W=128 RAW=16 WAW=225 indep=0.312 maxW=56 |
| bound@8 / 波 | **5.89** / 56 |
| serial / OCC@1 / OCC@8 fg | 2.16 / 3.90 / 3.33 |
| Instant-off OCC / SF reuse | 2.965 / 5.990（2.02×） |
| 臂 | Full, Win_1, Opt, Defer, Win_1 |
| wait / cover | 8 / 0 |
| ungated | 410,338,373,381,375 |
| end_block µs | 129,142,131,125,135 |
| unfenced / SF abort / OCC abort | 85–99 / 53–76 / 64–144 |
| begin_blocked 末 | `[47,76,98,102,103,107,154,284]` |

## 19638737 — NEAR-Opt · 短 Basic-19

| | |
|--|--|
| n | 381 |
| DAG 有效 | L=20 W=350 RAW=11 WAW=81 indep=0.906 maxW=19 |
| DAG+lazy | L=188 |
| bound@8 / 波 | 8.00 / 48 |
| serial / OCC@1 / OCC@8 fg | 5.12 / 7.04 / 4.42 |
| Instant-off OCC / SF reuse | 5.046 / 9.808（1.94×） |
| 臂 | Opt×5 |
| wait / cover | 6 / 0 |
| ungated | 340,359,376,388,326 |
| end_block µs | 1151,276,216,183,205 |
| unfenced / SF abort / OCC abort | 17–21 / 17–30 / 19–32 |
| begin_blocked 末 | `[4,6,48,299,308,348]` |

## 19716145 — FAR · Full 打短链

| | |
|--|--|
| n | 341 |
| DAG 有效 | L=46 W=226 RAW=51 WAW=285 indep=0.619 maxW=45（Basic） |
| bound@8 / 波 | **7.41** / 46 |
| serial / OCC@1 / OCC@8 fg | 10.94 / 14.66 / 10.60 |
| Instant-off OCC / SF reuse | 10.887 / 19.879（1.83×） |
| 臂 | Full×5 · Full/6, Full/5, Win_1/3 |
| wait / cover | 8 / 0 |
| ungated | 373,356,450,345,521 |
| pick_gate 末 | 193 |
| end_block µs | 175,219,203,218,221 |
| unfenced / SF abort / OCC abort / reexec | 44–84 / 107–192 / 103–122 / 241–321 |
| begin_blocked 末 | `[4,14,73,81,112,127,130,149]` |

## 19860366 — FAR · Win_1 欠盖 L=33

| | |
|--|--|
| n | 430 |
| DAG 有效 | L=33 W=287 RAW=34 WAW=237 indep=0.588 maxW=31 |
| bound@8 / 波 | 8.00 / 54 |
| serial / OCC@1 / OCC@8 fg | 11.02 / 16.25 / 11.74 |
| Instant-off OCC / SF reuse | 9.753 / 18.054（1.85×） |
| 臂 | Full, Win_1, Win_1, Win_1, Win_1 |
| wait / cover | 8 / 0（covering_n 0→1） |
| ungated | 325,484,264,176,147 |
| end_block µs | 4346,268,268,507,266（仅冷 iter 重） |
| unfenced / SF abort / OCC abort | 66–78 / 67–91 / 68–100 |
| begin_blocked 末 | `[5,13,16,23,25,36,37,170]` |

## 19469101 — FAR · OA 队列 44 仍不盖

| | |
|--|--|
| n | 469 |
| DAG 有效 | L=36 W=279 RAW=24 WAW=222 indep=0.499 maxW=36（storage） |
| bound@8 / 波 | 8.00 / 59 |
| serial / OCC@1 / OCC@8 fg | 11.59 / 15.68 / 13.57 |
| Instant-off OCC / SF reuse | 9.973 / 13.984（1.40×） |
| 臂 | Opt, Opt, Opt, Full, Full |
| wait / cover | 8 / 0 |
| OA 队列 | 44 全 iter |
| ungated | 188,156,156,176,143 |
| end_block µs | 2507,341,320,235,227 |
| unfenced / SF abort / OCC abort | 51–67 / 50–68 / 63–102 |
| begin_blocked 末 | `[2,5,7,125,145,149,151,153]` |

## 3356896 — NEAR · 薄块

| | |
|--|--|
| n | 176 |
| DAG 有效 | L=17 W=154 RAW=0 WAW=24 indep=0.847 maxW=17（Basic） |
| bound@8 / 波 | 8.00 / 22 |
| serial / OCC@1 / OCC@8 fg | 0.30 / 1.11 / 1.04 |
| Instant-off OCC / SF reuse | 1.062 / 1.334（1.26×） |
| 臂 | Win_1, Win_2, Opt, Win_8, Defer |
| wait / cover | 2,3,4,2,3 / 0,8,8,8,8 |
| ungated | 40,36,37,30,57 |
| end_block µs | 51,61,61,55,70 |
| commute / ignore | 77 / 77 |
| unfenced / SF abort / OCC abort | 14–16 / 14–19 / 8–27 |
| begin_blocked 末 | `[16,19,20]` |
| opt_maj | true 全 SF iter |

## 未测（显式）

- per-wave admit 轨迹 / 精确波宽  
- PROFILE Instant 桶（本分析 Instant-off，不用）  
- 8 物理核墙（宿主 nproc=4）  
- 19807137 finegrain OCC@8 不作主墙（与 Instant-off OCC 18 ms 分开）
