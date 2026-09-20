# PR #36 最慢块：多方面 + 开销细粒度深挖（详版）

**基线 tip:** `c0638440813484b1f55abe4351383a4e8c9f8110` · Soft=0 · cores=8 · 物理核=4 · N=7  
**规则:** 不发明 ns；Instant-tax（PROFILE worker 求和）**不可**加总成墙；单时钟 `end_block_ns` / `prepaid_ns` / `pick_occ` 可与墙比量级。  
**摘要 JSON:** [`specfence-pr36-slowest-deepdive-summary.json`](specfence-pr36-slowest-deepdive-summary.json)  
**全量扫块:** [`specfence-pr36-allblocks-sweep.md`](specfence-pr36-allblocks-sweep.md)  
**主读（三面）:** [`specfence-pr36-k8-pc-cc-learn-analysis.md`](specfence-pr36-k8-pc-cc-learn-analysis.md)

---

## 0. 读法

本文件是 K8 **数据详版**。定位与「做了什么 / 没做好」在 [`specfence-pr36-k8-pc-cc-learn-analysis.md`](specfence-pr36-k8-pc-cc-learn-analysis.md)。不要只读本表下结论。

- **PRIMARY 墙** = Instant-off、同一 `Pevm` reuse、iters 1..N-1 中位数。
- PROFILE 节只标 Instant-tax，用来看 handler/validate/sched 桶，**不加进墙**。
- 结构 JSON 来自 finegrain 另跑，墙作废。
- 数字只来自本 tip 实测；缺字段写「未测」，不编 ns。
- Instant-off OCC 尖刺：`19807137` i=0 = 2254 ms，中位数仍用 13.331。扫块里 `19434587` OCC reuse=2273 ms 是 N=3 尖刺，本文件 OCC 稳定。

## 1. Instant-off 主证墙（PRIMARY）

| block | n | OCC med | SF cold | SF reuse | × | last arm | unf(last) | begin_n | pick_occ(暖) | pick_while_gated(暖) | end_block(暖) | prepaid(暖) |
|------:|--:|--------:|--------:|---------:|--:|----------|----------:|--------:|-------------:|---------------------:|--------------:|------------:|
| 19807137 | 712 | 13.331 | 35.461 | **54.794** | 4.11 | Seg_3 | 292 | 8 | 1956–2561 | 1956–2561 | 0.61–0.71 ms | 1.167–6.914 ms |
| 19434587 | 390 | 12.444 | 17.336 | **22.507** | 1.81 | Opt | 56 | 53 | 543–640 | 543–640 | 2.18–2.30 ms | 2.221–17.158 ms |
| 19606599 | 367 | 12.782 | 19.685 | **23.592** | 1.85 | Win_1 | 33 | 54 | 411–500 | 411–500 | 2.42–2.56 ms | 2.829–11.548 ms |
| 19716145 | 341 | 10.147 | 17.595 | **20.340** | 2.00 | Win_1 | 20 | 108 | 293–369 | 293–369 | 2.07–2.37 ms | 3.842–6.120 ms |
| 19860366 | 430 | 8.890 | 18.111 | **20.433** | 2.30 | Win_7 | 30 | 78 | 402–440 | 402–440 | 3.85–4.29 ms | 2.660–4.588 ms |
| 14396881 | 1346 | 3.992 | 9.996 | **13.858** | 3.47 | Full | 5 | 5 | 0–1069 | 0–1069 | 0.30–0.51 ms | 0.000–0.519 ms |
| 15274915 | 1226 | 4.922 | 11.506 | **15.770** | 3.20 | Win_1 | 13 | 7 | 110–1270 | 110–1270 | 0.35–0.42 ms | 0.000–0.041 ms |
| 13217637 | 1100 | 4.745 | 9.012 | **12.820** | 2.70 | Full | 7 | 8 | 1067–1084 | 1067–1084 | 0.30–0.46 ms | 0.160–0.406 ms |

> 上表由 `instant_off[].sf_iters` 重算；以 JSON 为准。

## 2. 逐块细挖

### 2.1 块 `19807137`

- catalog morph=`WAW_spine` L=571 W=106 RAW=9 WAW=628 bound@8=1.246935 in52=False
- PR34 Instant-off：OCC=17.590 SF reuse=65.515 arm=Win_1 begin=38 unf=236

#### 结构 / 目录

```json
{
  "ok": true,
  "dag": {
    "n_edges": 637,
    "n_raw": 9,
    "n_waw": 628,
    "longest_chain": 571,
    "independent_txs": 95,
    "independent_frac": 0.13342696629213482,
    "max_wave_width": 106,
    "mean_wave_width": 1.2469352014010509,
    "n_levels": 571,
    "multi_writer_locs": 30,
    "max_writers_on_loc": 571,
    "conflict_component_sizes_top10": [
      571,
      35,
      3,
      3,
      3,
      2
    ],
    "max_conflict_component": 571
  },
  "kind_histogram": {
    "storage": 1006,
    "basic_lazy": 12,
    "basic": 745
  },
  "d1_kind_counts_in_top": {
    "basic": 5,
    "storage": 4,
    "basic_lazy": 3
  },
  "max_basic_writers": 22,
  "max_storage_writers": 571,
  "d1_top_spines": [
    {
      "loc": 6996519588683120047,
      "kind": "storage",
      "n_writers": 571,
      "writers_head": [
        140,
        141,
        142,
        143,
        144,
        145,
        146,
        147,
        148,
        149,
        150,
        151
      ],
      "writers_tail": [
        705,
        706,
        707,
        708,
        709,
        710
      ]
    },
    {
      "loc": 13758554269703882113,
      "kind": "basic",
      "n_writers": 22,
      "writers_head": [
        9,
        24,
        40,
        44,
        62,
        69,
        74,
        75,
        84,
        86,
        90,
        92
      ],
      "writers_tail": [
        116,
        119,
        120,
        121,
        127,
        135
      ]
    },
    {
      "loc": 10657587388766081695,
      "kind": "storage",
      "n_writers": 6,
      "writers_head": [
        69,
        74,
        114,
        116,
        121,
        135
      ],
      "writers_tail": []
    },
    {
      "loc": 1578729947362715697,
      "kind": "basic_lazy",
      "n_writers": 3,
      "writers_head": [
        137,
        138,
        139
      ],
      "writers_tail": []
    },
    {
      "loc": 1986135301972538956,
      "kind": "basic",
      "n_writers": 3,
      "writers_head": [
        0,
        1,
        5
      ],
      "writers_tail": []
    },
    {
      "loc": 1999415700942881455,
      "kind": "basic_lazy",
      "n_writers": 3,
      "writers_head": [
        49,
        50,
        51
      ],
      "writers_tail": []
    },
    {
      "loc": 2124331608030694697,
      "kind": "basic_lazy",
      "n_writers": 3,
      "writers_head": [
        15,
        22,
        23
      ],
      "writers_tail": []
    },
    {
      "loc": 4564472699467847881,
      "kind": "basic",
      "n_writers": 3,
      "writers_head": [
        93,
        94,
        95
      ],
      "writers_tail": []
    },
    {
      "loc": 5802796195494741615,
      "kind": "storage",
      "n_writers": 3,
      "writers_head": [
        26,
        54,
        105
      ],
      "writers_tail": []
    },
    {
      "loc": 6661591636648175780,
      "kind": "basic",
      "n_writers": 3,
      "writers_head": [
        99,
        128,
        133
      ],
      "writers_tail": []
    },
    {
      "loc": 12706960404452454218,
      "kind": "storage",
      "n_writers": 3,
      "writers_head": [
        99,
        128,
        133
      ],
      "writers_tail": []
    },
    {
      "loc": 15813797744450235922,
      "kind": "basic",
      "n_writers": 3,
      "writers_head": [
        26,
        54,
        105
      ],
      "writers_tail": []
    }
  ]
}
```

#### Instant-off 逐 iter（OCC / SF 交错）

| i | mode | wall_ms | arm | w | need | unf | dp | sys | refuse | begin | pick_occ | pick_gate | skip_gate | occ_while_gated | end_ms | prepaid_ms | reexec_ms | oa | commute | soft | ready_w |
|--:|------|--------:|-----|--:|-----:|----:|---:|----:|-------:|------:|---------:|----------:|----------:|----------------:|-------:|-----------:|----------:|---:|--------:|-----:|--------:|
| 0 | occ | 2254.809 | — | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 | 0.000 | 0.000 | 0 | 0 | 0 | 0.00 |
| 0 | specfence | 35.461 | Full | 0 | 0 | 595 | 0 | 0 | 2 | 8 | 107 | 17 | 2 | 107 | 0.415 | 0.095 | 90.508 | 55 | 0 | 0 | 0.00 |
| 1 | occ | 15.443 | — | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 | 0.000 | 0.000 | 0 | 0 | 0 | 0.00 |
| 1 | specfence | 54.346 | Full | 0 | 0 | 321 | 0 | 0 | 3 | 8 | 2292 | 754 | 33 | 2292 | 0.608 | 4.396 | 134.750 | 803 | 0 | 0 | 0.00 |
| 2 | occ | 13.331 | — | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 | 0.000 | 0.000 | 0 | 0 | 0 | 0.00 |
| 2 | specfence | 55.715 | Win_1 | 1 | 0 | 246 | 1 | 0 | 2 | 8 | 2043 | 940 | 45 | 2043 | 0.629 | 2.735 | 145.938 | 995 | 0 | 0 | 1.00 |
| 3 | occ | 11.725 | — | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 | 0.000 | 0.000 | 0 | 0 | 0 | 0.00 |
| 3 | specfence | 54.794 | Win_2 | 2 | 2 | 253 | 1 | 1 | 2 | 8 | 2561 | 789 | 31 | 2561 | 0.693 | 6.914 | 146.410 | 832 | 0 | 0 | 1.00 |
| 4 | occ | 12.533 | — | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 | 0.000 | 0.000 | 0 | 0 | 0 | 0.00 |
| 4 | specfence | 54.179 | Opt | 0 | 2 | 278 | 1 | 2 | 2 | 8 | 2360 | 977 | 58 | 2360 | 0.705 | 1.167 | 179.383 | 1025 | 0 | 0 | 0.00 |
| 5 | occ | 13.301 | — | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 | 0.000 | 0.000 | 0 | 0 | 0 | 0.00 |
| 5 | specfence | 52.527 | Seg_2 | 1 | 2 | 296 | 1 | 2 | 3 | 8 | 1956 | 990 | 35 | 1956 | 0.637 | 1.759 | 136.844 | 1051 | 1 | 0 | 0.00 |
| 6 | occ | 17.656 | — | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 | 0.000 | 0.000 | 0 | 0 | 0 | 0.00 |
| 6 | specfence | 57.637 | Seg_3 | 2 | 2 | 292 | 1 | 2 | 2 | 8 | 2472 | 901 | 43 | 2472 | 0.635 | 2.101 | 162.446 | 951 | 0 | 0 | 1.00 |

轨迹摘要：

- 臂: `['Full', 'Full', 'Win_1', 'Win_2', 'Opt', 'Seg_2', 'Seg_3']`
- unfenced: `[595, 321, 246, 253, 278, 296, 292]`
- begin 洞: `[8, 8, 8, 8, 8, 8, 8]`
- pick_occ: `[107, 2292, 2043, 2561, 2360, 1956, 2472]`
- occ_pick_while_gated: `[107, 2292, 2043, 2561, 2360, 1956, 2472]`
- end_block_ms: `['0.415', '0.608', '0.629', '0.693', '0.705', '0.637', '0.635']`
- prepaid_ms: `['0.095', '4.396', '2.735', '6.914', '1.167', '1.759', '2.101']`
- refuse: `[2, 3, 2, 2, 2, 3, 2]`
- dp: `[0, 0, 1, 1, 1, 1, 1]`
- Soft: `[0, 0, 0, 0, 0, 0, 0]`（必须全 0）

### 2.2 块 `19434587`

- catalog morph=`mixed_RAW_WAW` L=62 W=208 RAW=65 WAW=255 bound@8=6.290323 in52=False

#### 结构 / 目录

```json
{
  "ok": true,
  "dag": {
    "n_edges": 320,
    "n_raw": 65,
    "n_waw": 255,
    "longest_chain": 62,
    "independent_txs": 188,
    "independent_frac": 0.48205128205128206,
    "max_wave_width": 208,
    "mean_wave_width": 6.290322580645161,
    "n_levels": 62,
    "multi_writer_locs": 68,
    "max_writers_on_loc": 62,
    "conflict_component_sizes_top10": [
      91,
      63,
      10,
      6,
      5,
      4,
      3,
      3,
      3,
      2
    ],
    "max_conflict_component": 91
  },
  "kind_histogram": {
    "basic": 436,
    "storage": 838,
    "basic_lazy": 31
  },
  "d1_kind_counts_in_top": {
    "basic_lazy": 2,
    "basic": 4,
    "storage": 6
  },
  "max_basic_writers": 56,
  "max_storage_writers": 62,
  "d1_top_spines": [
    {
      "loc": 15128534048206632394,
      "kind": "storage",
      "n_writers": 62,
      "writers_head": [
        141,
        142,
        143,
        144,
        145,
        146,
        147,
        148,
        149,
        150,
        151,
        152
      ],
      "writers_tail": [
        198,
        199,
        200,
        201,
        202,
        203
      ]
    },
    {
      "loc": 13758554269703882113,
      "kind": "basic",
      "n_writers": 56,
      "writers_head": [
        1,
        2,
        3,
        5,
        7,
        8,
        10,
        11,
        12,
        15,
        16,
        18
      ],
      "writers_tail": [
        327,
        329,
        351,
        367,
        372,
        382
      ]
    },
    {
      "loc": 3136403394186661372,
      "kind": "storage",
      "n_writers": 11,
      "writers_head": [
        0,
        1,
        2,
        4,
        5,
        6,
        7,
        8,
        9,
        12,
        15
      ],
      "writers_tail": []
    },
    {
      "loc": 4432550207863775364,
      "kind": "storage",
      "n_writers": 11,
      "writers_head": [
        0,
        1,
        2,
        4,
        5,
        6,
        7,
        8,
        9,
        12,
        15
      ],
      "writers_tail": []
    },
    {
      "loc": 5240750072915126637,
      "kind": "storage",
      "n_writers": 11,
      "writers_head": [
        0,
        1,
        2,
        4,
        5,
        6,
        7,
        8,
        9,
        12,
        15
      ],
      "writers_tail": []
    },
    {
      "loc": 5029830881688391922,
      "kind": "storage",
      "n_writers": 10,
      "writers_head": [
        82,
        84,
        85,
        91,
        93,
        95,
        100,
        104,
        111,
        118
      ],
      "writers_tail": []
    },
    {
      "loc": 6848973244721538073,
      "kind": "basic_lazy",
      "n_writers": 6,
      "writers_head": [
        39,
        50,
        62,
        64,
        65,
        66
      ],
      "writers_tail": []
    },
    {
      "loc": 7392778884173490200,
      "kind": "basic",
      "n_writers": 6,
      "writers_head": [
        92,
        99,
        103,
        107,
        112,
        115
      ],
      "writers_tail": []
    },
    {
      "loc": 13283012944938055657,
      "kind": "basic_lazy",
      "n_writers": 6,
      "writers_head": [
        43,
        44,
        45,
        46,
        47,
        48
      ],
      "writers_tail": []
    },
    {
      "loc": 11677032071339632034,
      "kind": "basic",
      "n_writers": 5,
      "writers_head": [
        132,
        134,
        135,
        136,
        139
      ],
      "writers_tail": []
    },
    {
      "loc": 3565139037764453269,
      "kind": "basic",
      "n_writers": 4,
      "writers_head": [
        242,
        243,
        245,
        248
      ],
      "writers_tail": []
    },
    {
      "loc": 5337137606227373238,
      "kind": "storage",
      "n_writers": 4,
      "writers_head": [
        16,
        18,
        21,
        327
      ],
      "writers_tail": []
    }
  ]
}
```

#### Instant-off 逐 iter（OCC / SF 交错）

| i | mode | wall_ms | arm | w | need | unf | dp | sys | refuse | begin | pick_occ | pick_gate | skip_gate | occ_while_gated | end_ms | prepaid_ms | reexec_ms | oa | commute | soft | ready_w |
|--:|------|--------:|-----|--:|-----:|----:|---:|----:|-------:|------:|---------:|----------:|----------:|----------------:|-------:|-----------:|----------:|---:|--------:|-----:|--------:|
| 0 | occ | 11.795 | — | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 | 0.000 | 0.000 | 0 | 0 | 0 | 0.00 |
| 0 | specfence | 17.336 | Win_1 | 1 | 0 | 135 | 0 | 0 | 29 | 49 | 688 | 90 | 14 | 688 | 2.110 | 2.036 | 35.205 | 140 | 0 | 0 | 1.00 |
| 1 | occ | 10.598 | — | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 | 0.000 | 0.000 | 0 | 0 | 0 | 0.00 |
| 1 | specfence | 22.507 | Win_1 | 1 | 0 | 58 | 1 | 0 | 68 | 49 | 640 | 226 | 36 | 640 | 2.259 | 2.333 | 39.293 | 281 | 0 | 0 | 1.00 |
| 2 | occ | 12.897 | — | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 | 0.000 | 0.000 | 0 | 0 | 0 | 0.00 |
| 2 | specfence | 22.320 | Win_2 | 2 | 3 | 59 | 1 | 1 | 98 | 51 | 601 | 250 | 45 | 601 | 2.199 | 2.737 | 44.240 | 302 | 1 | 0 | 0.00 |
| 3 | occ | 17.457 | — | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 | 0.000 | 0.000 | 0 | 0 | 0 | 0.00 |
| 3 | specfence | 20.108 | Opt | 0 | 3 | 48 | 0 | 2 | 81 | 49 | 543 | 245 | 45 | 543 | 2.257 | 3.029 | 36.218 | 299 | 1 | 0 | 0.00 |
| 4 | occ | 13.123 | — | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 | 0.000 | 0.000 | 0 | 0 | 0 | 0.00 |
| 4 | specfence | 22.277 | Win_1 | 1 | 0 | 62 | 1 | 2 | 90 | 52 | 544 | 224 | 57 | 544 | 2.253 | 2.221 | 42.839 | 278 | 0 | 0 | 0.00 |
| 5 | occ | 11.213 | — | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 | 0.000 | 0.000 | 0 | 0 | 0 | 0.00 |
| 5 | specfence | 23.591 | Win_2 | 2 | 8 | 69 | 2 | 2 | 108 | 51 | 597 | 242 | 45 | 597 | 2.180 | 3.636 | 46.984 | 301 | 0 | 0 | 1.00 |
| 6 | occ | 12.444 | — | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 | 0.000 | 0.000 | 0 | 0 | 0 | 0.00 |
| 6 | specfence | 25.210 | Opt | 0 | 8 | 56 | 1 | 3 | 57 | 53 | 561 | 261 | 37 | 561 | 2.298 | 17.158 | 48.338 | 317 | 0 | 0 | 1.00 |

轨迹摘要：

- 臂: `['Win_1', 'Win_1', 'Win_2', 'Opt', 'Win_1', 'Win_2', 'Opt']`
- unfenced: `[135, 58, 59, 48, 62, 69, 56]`
- begin 洞: `[49, 49, 51, 49, 52, 51, 53]`
- pick_occ: `[688, 640, 601, 543, 544, 597, 561]`
- occ_pick_while_gated: `[688, 640, 601, 543, 544, 597, 561]`
- end_block_ms: `['2.110', '2.259', '2.199', '2.257', '2.253', '2.180', '2.298']`
- prepaid_ms: `['2.036', '2.333', '2.737', '3.029', '2.221', '3.636', '17.158']`
- refuse: `[29, 68, 98, 81, 90, 108, 57]`
- dp: `[0, 1, 1, 0, 1, 2, 1]`
- Soft: `[0, 0, 0, 0, 0, 0, 0]`（必须全 0）

### 2.3 块 `19606599`

- catalog morph=`mixed_RAW_WAW` L=57 W=261 RAW=42 WAW=177 bound@8=6.438596 in52=True

#### 结构 / 目录

```json
{
  "ok": true,
  "dag": {
    "n_edges": 219,
    "n_raw": 42,
    "n_waw": 177,
    "longest_chain": 57,
    "independent_txs": 236,
    "independent_frac": 0.6430517711171662,
    "max_wave_width": 261,
    "mean_wave_width": 6.43859649122807,
    "n_levels": 57,
    "multi_writer_locs": 87,
    "max_writers_on_loc": 54,
    "conflict_component_sizes_top10": [
      79,
      7,
      4,
      3,
      3,
      3,
      3,
      3,
      3,
      3
    ],
    "max_conflict_component": 79
  },
  "kind_histogram": {
    "storage": 802,
    "basic_lazy": 30,
    "basic": 447
  },
  "d1_kind_counts_in_top": {
    "basic": 5,
    "storage": 4,
    "basic_lazy": 3
  },
  "max_basic_writers": 55,
  "max_storage_writers": 7,
  "d1_top_spines": [
    {
      "loc": 13758554269703882113,
      "kind": "basic",
      "n_writers": 55,
      "writers_head": [
        4,
        7,
        24,
        25,
        26,
        28,
        31,
        42,
        43,
        45,
        50,
        52
      ],
      "writers_tail": [
        331,
        336,
        337,
        347,
        359,
        360
      ]
    },
    {
      "loc": 4152107800617711764,
      "kind": "basic_lazy",
      "n_writers": 12,
      "writers_head": [
        10,
        11,
        12,
        13,
        14,
        15,
        16,
        17,
        18,
        19,
        20,
        21
      ],
      "writers_tail": []
    },
    {
      "loc": 6661591636648175780,
      "kind": "basic",
      "n_writers": 7,
      "writers_head": [
        297,
        303,
        315,
        329,
        349,
        350,
        361
      ],
      "writers_tail": []
    },
    {
      "loc": 12706960404452454218,
      "kind": "storage",
      "n_writers": 7,
      "writers_head": [
        297,
        303,
        315,
        329,
        349,
        350,
        361
      ],
      "writers_tail": []
    },
    {
      "loc": 11482287534235715937,
      "kind": "basic_lazy",
      "n_writers": 5,
      "writers_head": [
        47,
        261,
        262,
        263,
        264
      ],
      "writers_tail": []
    },
    {
      "loc": 3565139037764453269,
      "kind": "basic",
      "n_writers": 4,
      "writers_head": [
        7,
        226,
        229,
        231
      ],
      "writers_tail": []
    },
    {
      "loc": 5770457006036027408,
      "kind": "basic_lazy",
      "n_writers": 4,
      "writers_head": [
        112,
        113,
        114,
        115
      ],
      "writers_tail": []
    },
    {
      "loc": 17821644390987094499,
      "kind": "storage",
      "n_writers": 4,
      "writers_head": [
        100,
        103,
        104,
        107
      ],
      "writers_tail": []
    },
    {
      "loc": 35115354374784020,
      "kind": "basic",
      "n_writers": 3,
      "writers_head": [
        296,
        298,
        299
      ],
      "writers_tail": []
    },
    {
      "loc": 1969659763500873606,
      "kind": "storage",
      "n_writers": 3,
      "writers_head": [
        196,
        204,
        319
      ],
      "writers_tail": []
    },
    {
      "loc": 2131844429468590708,
      "kind": "basic",
      "n_writers": 3,
      "writers_head": [
        157,
        206,
        222
      ],
      "writers_tail": []
    },
    {
      "loc": 2411129661802360381,
      "kind": "storage",
      "n_writers": 3,
      "writers_head": [
        63,
        64,
        65
      ],
      "writers_tail": []
    }
  ]
}
```

#### Instant-off 逐 iter（OCC / SF 交错）

| i | mode | wall_ms | arm | w | need | unf | dp | sys | refuse | begin | pick_occ | pick_gate | skip_gate | occ_while_gated | end_ms | prepaid_ms | reexec_ms | oa | commute | soft | ready_w |
|--:|------|--------:|-----|--:|-----:|----:|---:|----:|-------:|------:|---------:|----------:|----------:|----------------:|-------:|-----------:|----------:|---:|--------:|-----:|--------:|
| 0 | occ | 14.833 | — | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 | 0.000 | 0.000 | 0 | 0 | 0 | 0.00 |
| 0 | specfence | 19.685 | Opt | 0 | 0 | 61 | 0 | 0 | 17 | 54 | 415 | 102 | 8 | 415 | 2.338 | 3.206 | 28.050 | 159 | 0 | 0 | 0.00 |
| 1 | occ | 13.367 | — | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 | 0.000 | 0.000 | 0 | 0 | 0 | 0.00 |
| 1 | specfence | 25.891 | Opt | 0 | 0 | 48 | 0 | 0 | 47 | 54 | 474 | 197 | 18 | 474 | 2.416 | 7.559 | 56.097 | 257 | 0 | 0 | 0.00 |
| 2 | occ | 12.782 | — | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 | 0.000 | 0.000 | 0 | 0 | 0 | 0.00 |
| 2 | specfence | 21.063 | Opt | 0 | 0 | 41 | 0 | 0 | 46 | 54 | 460 | 153 | 20 | 460 | 2.463 | 4.137 | 41.387 | 213 | 0 | 0 | 1.00 |
| 3 | occ | 11.645 | — | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 | 0.000 | 0.000 | 0 | 0 | 0 | 0.00 |
| 3 | specfence | 19.195 | Opt | 0 | 0 | 37 | 0 | 0 | 22 | 54 | 411 | 131 | 14 | 411 | 2.479 | 3.307 | 25.582 | 188 | 0 | 0 | 0.00 |
| 4 | occ | 10.732 | — | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 | 0.000 | 0.000 | 0 | 0 | 0 | 0.00 |
| 4 | specfence | 20.645 | Opt | 0 | 0 | 39 | 0 | 0 | 42 | 54 | 448 | 144 | 20 | 448 | 2.544 | 2.829 | 31.915 | 202 | 0 | 0 | 0.00 |
| 5 | occ | 13.808 | — | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 | 0.000 | 0.000 | 0 | 0 | 0 | 0.00 |
| 5 | specfence | 27.624 | Opt | 0 | 0 | 46 | 0 | 0 | 25 | 54 | 500 | 251 | 14 | 500 | 2.439 | 11.548 | 66.063 | 311 | 0 | 0 | 1.00 |
| 6 | occ | 10.972 | — | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 | 0.000 | 0.000 | 0 | 0 | 0 | 0.00 |
| 6 | specfence | 23.592 | Win_1 | 1 | 0 | 33 | 0 | 0 | 48 | 54 | 452 | 191 | 23 | 452 | 2.561 | 3.230 | 38.520 | 250 | 0 | 0 | 1.00 |

轨迹摘要：

- 臂: `['Opt', 'Opt', 'Opt', 'Opt', 'Opt', 'Opt', 'Win_1']`
- unfenced: `[61, 48, 41, 37, 39, 46, 33]`
- begin 洞: `[54, 54, 54, 54, 54, 54, 54]`
- pick_occ: `[415, 474, 460, 411, 448, 500, 452]`
- occ_pick_while_gated: `[415, 474, 460, 411, 448, 500, 452]`
- end_block_ms: `['2.338', '2.416', '2.463', '2.479', '2.544', '2.439', '2.561']`
- prepaid_ms: `['3.206', '7.559', '4.137', '3.307', '2.829', '11.548', '3.230']`
- refuse: `[17, 47, 46, 22, 42, 25, 48]`
- dp: `[0, 0, 0, 0, 0, 0, 0]`
- Soft: `[0, 0, 0, 0, 0, 0, 0]`（必须全 0）

### 2.4 块 `19716145`

- catalog morph=`mixed_RAW_WAW` L=46 W=226 RAW=51 WAW=285 bound@8=7.413043 in52=True

#### 结构 / 目录

```json
{
  "ok": true,
  "dag": {
    "n_edges": 336,
    "n_raw": 51,
    "n_waw": 285,
    "longest_chain": 46,
    "independent_txs": 211,
    "independent_frac": 0.6187683284457478,
    "max_wave_width": 226,
    "mean_wave_width": 7.413043478260869,
    "n_levels": 46,
    "multi_writer_locs": 70,
    "max_writers_on_loc": 45,
    "conflict_component_sizes_top10": [
      56,
      25,
      7,
      7,
      7,
      5,
      4,
      4,
      3,
      2
    ],
    "max_conflict_component": 56
  },
  "kind_histogram": {
    "basic_lazy": 10,
    "basic": 416,
    "storage": 709,
    "unknown": 28
  },
  "d1_kind_counts_in_top": {
    "basic": 5,
    "storage": 7
  },
  "max_basic_writers": 46,
  "max_storage_writers": 25,
  "d1_top_spines": [
    {
      "loc": 13758554269703882113,
      "kind": "basic",
      "n_writers": 46,
      "writers_head": [
        1,
        2,
        5,
        6,
        7,
        8,
        10,
        12,
        13,
        14,
        15,
        16
      ],
      "writers_tail": [
        199,
        301,
        311,
        314,
        320,
        322
      ]
    },
    {
      "loc": 2131844429468590708,
      "kind": "basic",
      "n_writers": 25,
      "writers_head": [
        59,
        83,
        84,
        85,
        86,
        87,
        88,
        89,
        90,
        91,
        92,
        93
      ],
      "writers_tail": [
        101,
        102,
        103,
        104,
        105,
        106
      ]
    },
    {
      "loc": 3674476363946301486,
      "kind": "storage",
      "n_writers": 25,
      "writers_head": [
        59,
        83,
        84,
        85,
        86,
        87,
        88,
        89,
        90,
        91,
        92,
        93
      ],
      "writers_tail": [
        101,
        102,
        103,
        104,
        105,
        106
      ]
    },
    {
      "loc": 13028994558959264776,
      "kind": "basic",
      "n_writers": 25,
      "writers_head": [
        59,
        83,
        84,
        85,
        86,
        87,
        88,
        89,
        90,
        91,
        92,
        93
      ],
      "writers_tail": [
        101,
        102,
        103,
        104,
        105,
        106
      ]
    },
    {
      "loc": 4467519899597117654,
      "kind": "storage",
      "n_writers": 8,
      "writers_head": [
        0,
        2,
        4,
        10,
        14,
        16,
        29,
        73
      ],
      "writers_tail": []
    },
    {
      "loc": 1316388805071925421,
      "kind": "storage",
      "n_writers": 7,
      "writers_head": [
        0,
        2,
        4,
        10,
        14,
        16,
        29
      ],
      "writers_tail": []
    },
    {
      "loc": 4250158584651359420,
      "kind": "storage",
      "n_writers": 7,
      "writers_head": [
        0,
        2,
        4,
        10,
        14,
        16,
        29
      ],
      "writers_tail": []
    },
    {
      "loc": 6661591636648175780,
      "kind": "basic",
      "n_writers": 7,
      "writers_head": [
        137,
        166,
        173,
        183,
        185,
        191,
        192
      ],
      "writers_tail": []
    },
    {
      "loc": 11531033656851144744,
      "kind": "storage",
      "n_writers": 7,
      "writers_head": [
        5,
        6,
        7,
        8,
        9,
        15,
        50
      ],
      "writers_tail": []
    },
    {
      "loc": 11677032071339632034,
      "kind": "basic",
      "n_writers": 7,
      "writers_head": [
        250,
        251,
        252,
        253,
        254,
        255,
        256
      ],
      "writers_tail": []
    },
    {
      "loc": 14446654419013847499,
      "kind": "storage",
      "n_writers": 7,
      "writers_head": [
        115,
        116,
        118,
        119,
        120,
        131,
        132
      ],
      "writers_tail": []
    },
    {
      "loc": 14492463056885141782,
      "kind": "storage",
      "n_writers": 7,
      "writers_head": [
        5,
        6,
        7,
        8,
        9,
        15,
        50
      ],
      "writers_tail": []
    }
  ]
}
```

#### Instant-off 逐 iter（OCC / SF 交错）

| i | mode | wall_ms | arm | w | need | unf | dp | sys | refuse | begin | pick_occ | pick_gate | skip_gate | occ_while_gated | end_ms | prepaid_ms | reexec_ms | oa | commute | soft | ready_w |
|--:|------|--------:|-----|--:|-----:|----:|---:|----:|-------:|------:|---------:|----------:|----------:|----------------:|-------:|-----------:|----------:|---:|--------:|-----:|--------:|
| 0 | occ | 10.521 | — | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 | 0.000 | 0.000 | 0 | 0 | 0 | 0.00 |
| 0 | specfence | 17.595 | Win_1 | 1 | 0 | 45 | 0 | 0 | 44 | 102 | 380 | 218 | 35 | 380 | 2.065 | 5.626 | 30.720 | 287 | 0 | 0 | 1.00 |
| 1 | occ | 9.865 | — | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 | 0.000 | 0.000 | 0 | 0 | 0 | 0.00 |
| 1 | specfence | 18.195 | Win_1 | 1 | 0 | 28 | 1 | 0 | 92 | 102 | 369 | 232 | 43 | 369 | 2.072 | 5.754 | 28.241 | 308 | 0 | 0 | 1.00 |
| 2 | occ | 10.147 | — | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 | 0.000 | 0.000 | 0 | 0 | 0 | 0.00 |
| 2 | specfence | 19.713 | Win_2 | 2 | 5 | 29 | 1 | 1 | 70 | 103 | 332 | 260 | 39 | 332 | 2.230 | 5.739 | 34.845 | 330 | 0 | 0 | 1.00 |
| 3 | occ | 9.596 | — | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 | 0.000 | 0.000 | 0 | 0 | 0 | 0.00 |
| 3 | specfence | 18.581 | Opt | 0 | 5 | 21 | 1 | 2 | 68 | 103 | 323 | 318 | 44 | 323 | 2.289 | 4.247 | 39.651 | 396 | 0 | 0 | 0.00 |
| 4 | occ | 9.293 | — | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 | 0.000 | 0.000 | 0 | 0 | 0 | 0.00 |
| 4 | specfence | 20.340 | Defer | 0 | 5 | 17 | 2 | 2 | 67 | 106 | 293 | 286 | 48 | 293 | 2.272 | 4.943 | 48.849 | 363 | 0 | 0 | 0.00 |
| 5 | occ | 10.237 | — | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 | 0.000 | 0.000 | 0 | 0 | 0 | 0.00 |
| 5 | specfence | 21.553 | Win_5 | 5 | 6 | 35 | 1 | 3 | 72 | 102 | 359 | 319 | 33 | 359 | 2.366 | 3.842 | 52.838 | 396 | 0 | 0 | 0.00 |
| 6 | occ | 10.216 | — | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 | 0.000 | 0.000 | 0 | 0 | 0 | 0.00 |
| 6 | specfence | 20.850 | Win_1 | 1 | 0 | 20 | 2 | 3 | 96 | 108 | 332 | 335 | 67 | 332 | 2.264 | 6.120 | 54.683 | 420 | 0 | 0 | 1.00 |

轨迹摘要：

- 臂: `['Win_1', 'Win_1', 'Win_2', 'Opt', 'Defer', 'Win_5', 'Win_1']`
- unfenced: `[45, 28, 29, 21, 17, 35, 20]`
- begin 洞: `[102, 102, 103, 103, 106, 102, 108]`
- pick_occ: `[380, 369, 332, 323, 293, 359, 332]`
- occ_pick_while_gated: `[380, 369, 332, 323, 293, 359, 332]`
- end_block_ms: `['2.065', '2.072', '2.230', '2.289', '2.272', '2.366', '2.264']`
- prepaid_ms: `['5.626', '5.754', '5.739', '4.247', '4.943', '3.842', '6.120']`
- refuse: `[44, 92, 70, 68, 67, 72, 96]`
- dp: `[0, 1, 1, 1, 2, 1, 2]`
- Soft: `[0, 0, 0, 0, 0, 0, 0]`（必须全 0）

### 2.5 块 `19860366`

- catalog morph=`mixed_RAW_WAW` L=33 W=287 RAW=34 WAW=237 bound@8=8.0 in52=True

#### 结构 / 目录

```json
{
  "ok": true,
  "dag": {
    "n_edges": 270,
    "n_raw": 34,
    "n_waw": 236,
    "longest_chain": 33,
    "independent_txs": 255,
    "independent_frac": 0.5930232558139535,
    "max_wave_width": 288,
    "mean_wave_width": 13.030303030303031,
    "n_levels": 33,
    "multi_writer_locs": 116,
    "max_writers_on_loc": 31,
    "conflict_component_sizes_top10": [
      58,
      19,
      10,
      9,
      8,
      7,
      5,
      5,
      5,
      4
    ],
    "max_conflict_component": 58
  },
  "kind_histogram": {
    "code_hash": 1,
    "basic": 472,
    "storage": 975,
    "basic_lazy": 49
  },
  "d1_kind_counts_in_top": {
    "basic": 4,
    "storage": 6,
    "basic_lazy": 2
  },
  "max_basic_writers": 31,
  "max_storage_writers": 19,
  "d1_top_spines": [
    {
      "loc": 13758554269703882113,
      "kind": "basic",
      "n_writers": 31,
      "writers_head": [
        2,
        3,
        4,
        6,
        9,
        10,
        21,
        22,
        24,
        28,
        43,
        49
      ],
      "writers_tail": [
        386,
        388,
        392,
        398,
        409,
        413
      ]
    },
    {
      "loc": 6593269198944130716,
      "kind": "storage",
      "n_writers": 19,
      "writers_head": [
        79,
        92,
        94,
        97,
        98,
        99,
        100,
        101,
        102,
        106,
        107,
        109
      ],
      "writers_tail": [
        111,
        112,
        113,
        114,
        115,
        116
      ]
    },
    {
      "loc": 11677032071339632034,
      "kind": "basic",
      "n_writers": 9,
      "writers_head": [
        136,
        138,
        139,
        141,
        142,
        144,
        146,
        147,
        151
      ],
      "writers_tail": []
    },
    {
      "loc": 11901834437072914027,
      "kind": "storage",
      "n_writers": 9,
      "writers_head": [
        70,
        87,
        88,
        89,
        90,
        93,
        95,
        105,
        117
      ],
      "writers_tail": []
    },
    {
      "loc": 3412464983054901818,
      "kind": "storage",
      "n_writers": 8,
      "writers_head": [
        277,
        278,
        279,
        280,
        307,
        308,
        314,
        329
      ],
      "writers_tail": []
    },
    {
      "loc": 10039926504477720815,
      "kind": "storage",
      "n_writers": 8,
      "writers_head": [
        277,
        278,
        279,
        280,
        307,
        308,
        314,
        329
      ],
      "writers_tail": []
    },
    {
      "loc": 13028994558959264776,
      "kind": "basic",
      "n_writers": 7,
      "writers_head": [
        131,
        207,
        252,
        261,
        366,
        372,
        397
      ],
      "writers_tail": []
    },
    {
      "loc": 106387738491603213,
      "kind": "basic_lazy",
      "n_writers": 5,
      "writers_head": [
        135,
        137,
        140,
        143,
        145
      ],
      "writers_tail": []
    },
    {
      "loc": 3267826814471304034,
      "kind": "storage",
      "n_writers": 5,
      "writers_head": [
        26,
        120,
        133,
        360,
        419
      ],
      "writers_tail": []
    },
    {
      "loc": 3800305236383378638,
      "kind": "basic",
      "n_writers": 5,
      "writers_head": [
        216,
        218,
        220,
        221,
        222
      ],
      "writers_tail": []
    },
    {
      "loc": 10373103219644659558,
      "kind": "basic_lazy",
      "n_writers": 5,
      "writers_head": [
        186,
        187,
        188,
        189,
        196
      ],
      "writers_tail": []
    },
    {
      "loc": 12092923029596316778,
      "kind": "storage",
      "n_writers": 5,
      "writers_head": [
        61,
        62,
        64,
        65,
        66
      ],
      "writers_tail": []
    }
  ]
}
```

#### Instant-off 逐 iter（OCC / SF 交错）

| i | mode | wall_ms | arm | w | need | unf | dp | sys | refuse | begin | pick_occ | pick_gate | skip_gate | occ_while_gated | end_ms | prepaid_ms | reexec_ms | oa | commute | soft | ready_w |
|--:|------|--------:|-----|--:|-----:|----:|---:|----:|-------:|------:|---------:|----------:|----------:|----------------:|-------:|-----------:|----------:|---:|--------:|-----:|--------:|
| 0 | occ | 8.473 | — | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 | 0.000 | 0.000 | 0 | 0 | 0 | 0.00 |
| 0 | specfence | 18.111 | Win_1 | 1 | 0 | 45 | 0 | 0 | 41 | 76 | 445 | 151 | 22 | 445 | 3.797 | 2.256 | 23.583 | 215 | 0 | 0 | 1.00 |
| 1 | occ | 8.178 | — | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 | 0.000 | 0.000 | 0 | 0 | 0 | 0.00 |
| 1 | specfence | 20.433 | Win_2 | 2 | 6 | 35 | 1 | 0 | 72 | 77 | 440 | 181 | 44 | 440 | 3.860 | 3.887 | 26.362 | 255 | 0 | 0 | 1.00 |
| 2 | occ | 8.939 | — | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 | 0.000 | 0.000 | 0 | 0 | 0 | 0.00 |
| 2 | specfence | 19.601 | Opt | 0 | 6 | 18 | 0 | 1 | 62 | 76 | 402 | 158 | 44 | 402 | 3.847 | 3.084 | 23.713 | 223 | 0 | 0 | 0.00 |
| 3 | occ | 8.890 | — | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 | 0.000 | 0.000 | 0 | 0 | 0 | 0.00 |
| 3 | specfence | 28.524 | Win_6 | 6 | 7 | 26 | 1 | 1 | 68 | 76 | 406 | 208 | 51 | 406 | 3.960 | 2.808 | 40.845 | 277 | 0 | 0 | 1.00 |
| 4 | occ | 9.476 | — | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 | 0.000 | 0.000 | 0 | 0 | 0 | 0.00 |
| 4 | specfence | 21.831 | Defer | 0 | 7 | 26 | 0 | 1 | 116 | 77 | 407 | 177 | 54 | 407 | 4.287 | 3.884 | 22.372 | 248 | 0 | 0 | 1.00 |
| 5 | occ | 9.705 | — | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 | 0.000 | 0.000 | 0 | 0 | 0 | 0.00 |
| 5 | specfence | 18.453 | Win_7 | 7 | 7 | 23 | 0 | 1 | 77 | 77 | 411 | 156 | 37 | 411 | 3.888 | 2.660 | 21.969 | 267 | 0 | 0 | 0.00 |
| 6 | occ | 8.819 | — | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 | 0.000 | 0.000 | 0 | 0 | 0 | 0.00 |
| 6 | specfence | 19.396 | Win_7 | 7 | 8 | 30 | 1 | 1 | 100 | 78 | 423 | 152 | 59 | 423 | 3.928 | 4.588 | 26.726 | 227 | 1 | 0 | 1.00 |

轨迹摘要：

- 臂: `['Win_1', 'Win_2', 'Opt', 'Win_6', 'Defer', 'Win_7', 'Win_7']`
- unfenced: `[45, 35, 18, 26, 26, 23, 30]`
- begin 洞: `[76, 77, 76, 76, 77, 77, 78]`
- pick_occ: `[445, 440, 402, 406, 407, 411, 423]`
- occ_pick_while_gated: `[445, 440, 402, 406, 407, 411, 423]`
- end_block_ms: `['3.797', '3.860', '3.847', '3.960', '4.287', '3.888', '3.928']`
- prepaid_ms: `['2.256', '3.887', '3.084', '2.808', '3.884', '2.660', '4.588']`
- refuse: `[41, 72, 62, 68, 116, 77, 100]`
- dp: `[0, 1, 0, 1, 0, 0, 1]`
- Soft: `[0, 0, 0, 0, 0, 0, 0]`（必须全 0）

### 2.6 块 `14396881`

- catalog morph=`near_independent_meta_gap` L=5 W=1337 RAW=0 WAW=13 bound@8=8.0 in52=False
- PR34 Instant-off：OCC=4.403 SF reuse=110.887 arm=Defer begin=6 unf=4

#### 结构 / 目录

```json
{
  "ok": true,
  "dag": {
    "n_edges": 13,
    "n_raw": 0,
    "n_waw": 13,
    "longest_chain": 5,
    "independent_txs": 1331,
    "independent_frac": 0.9888558692421991,
    "max_wave_width": 1337,
    "mean_wave_width": 269.2,
    "n_levels": 5,
    "multi_writer_locs": 9,
    "max_writers_on_loc": 5,
    "conflict_component_sizes_top10": [
      6,
      3,
      2,
      2,
      2
    ],
    "max_conflict_component": 6
  },
  "kind_histogram": {
    "basic": 64,
    "basic_lazy": 1307,
    "storage": 86,
    "code_hash": 1
  },
  "d1_kind_counts_in_top": {
    "basic": 4,
    "storage": 1,
    "basic_lazy": 7
  },
  "max_basic_writers": 1197,
  "max_storage_writers": 2,
  "d1_top_spines": [
    {
      "loc": 11880556412163541166,
      "kind": "basic_lazy",
      "n_writers": 1197,
      "writers_head": [
        47,
        48,
        49,
        50,
        51,
        52,
        53,
        54,
        55,
        56,
        57,
        58
      ],
      "writers_tail": [
        1266,
        1267,
        1268,
        1269,
        1270,
        1271
      ]
    },
    {
      "loc": 14562814392964356195,
      "kind": "basic_lazy",
      "n_writers": 60,
      "writers_head": [
        1272,
        1273,
        1274,
        1275,
        1276,
        1278,
        1279,
        1280,
        1281,
        1282,
        1283,
        1284
      ],
      "writers_tail": [
        1338,
        1339,
        1340,
        1342,
        1343,
        1344
      ]
    },
    {
      "loc": 18085074139216513186,
      "kind": "basic_lazy",
      "n_writers": 10,
      "writers_head": [
        431,
        509,
        510,
        514,
        516,
        519,
        644,
        646,
        1060,
        1206
      ],
      "writers_tail": []
    },
    {
      "loc": 3001978160548923980,
      "kind": "basic_lazy",
      "n_writers": 5,
      "writers_head": [
        1312,
        1334,
        1336,
        1341,
        1345
      ],
      "writers_tail": []
    },
    {
      "loc": 13758554269703882113,
      "kind": "basic",
      "n_writers": 5,
      "writers_head": [
        29,
        39,
        1311,
        1314,
        1327
      ],
      "writers_tail": []
    },
    {
      "loc": 15321224630279306243,
      "kind": "basic_lazy",
      "n_writers": 4,
      "writers_head": [
        79,
        253,
        475,
        767
      ],
      "writers_tail": []
    },
    {
      "loc": 5204584229706170905,
      "kind": "basic",
      "n_writers": 3,
      "writers_head": [
        31,
        32,
        33
      ],
      "writers_tail": []
    },
    {
      "loc": 1324244158569404167,
      "kind": "storage",
      "n_writers": 2,
      "writers_head": [
        1029,
        1195
      ],
      "writers_tail": []
    },
    {
      "loc": 3558184673650001547,
      "kind": "basic",
      "n_writers": 2,
      "writers_head": [
        1292,
        1327
      ],
      "writers_tail": []
    },
    {
      "loc": 3644419856594474976,
      "kind": "basic",
      "n_writers": 2,
      "writers_head": [
        763,
        764
      ],
      "writers_tail": []
    },
    {
      "loc": 5281552824467248440,
      "kind": "basic_lazy",
      "n_writers": 2,
      "writers_head": [
        1277,
        1285
      ],
      "writers_tail": []
    },
    {
      "loc": 8898878514797201221,
      "kind": "basic_lazy",
      "n_writers": 2,
      "writers_head": [
        38,
        42
      ],
      "writers_tail": []
    }
  ]
}
```

#### Instant-off 逐 iter（OCC / SF 交错）

| i | mode | wall_ms | arm | w | need | unf | dp | sys | refuse | begin | pick_occ | pick_gate | skip_gate | occ_while_gated | end_ms | prepaid_ms | reexec_ms | oa | commute | soft | ready_w |
|--:|------|--------:|-----|--:|-----:|----:|---:|----:|-------:|------:|---------:|----------:|----------:|----------------:|-------:|-----------:|----------:|---:|--------:|-----:|--------:|
| 0 | occ | 3.992 | — | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 | 0.000 | 0.000 | 0 | 0 | 0 | 0.00 |
| 0 | specfence | 9.996 | Opt | 0 | 0 | 3 | 0 | 0 | 0 | 5 | 810 | 8 | 1 | 810 | 0.289 | 0.640 | 0.482 | 17 | 0 | 0 | 1.00 |
| 1 | occ | 3.823 | — | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 | 0.000 | 0.000 | 0 | 0 | 0 | 0.00 |
| 1 | specfence | 14.482 | Opt | 0 | 0 | 7 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.337 | 0.000 | 0.524 | 0 | 0 | 0 | 0.00 |
| 2 | occ | 4.324 | — | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 | 0.000 | 0.000 | 0 | 0 | 0 | 0.00 |
| 2 | specfence | 13.331 | Opt | 0 | 0 | 3 | 0 | 0 | 0 | 5 | 774 | 8 | 1 | 774 | 0.300 | 0.068 | 0.484 | 17 | 0 | 0 | 1.00 |
| 3 | occ | 4.191 | — | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 | 0.000 | 0.000 | 0 | 0 | 0 | 0.00 |
| 3 | specfence | 13.408 | Opt | 0 | 0 | 3 | 0 | 0 | 0 | 5 | 766 | 8 | 1 | 766 | 0.339 | 0.519 | 0.646 | 17 | 0 | 0 | 1.00 |
| 4 | occ | 3.912 | — | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 | 0.000 | 0.000 | 0 | 0 | 0 | 0.00 |
| 4 | specfence | 13.858 | Opt | 0 | 0 | 3 | 0 | 0 | 0 | 5 | 814 | 7 | 1 | 814 | 0.329 | 0.041 | 0.417 | 16 | 0 | 0 | 1.00 |
| 5 | occ | 4.291 | — | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 | 0.000 | 0.000 | 0 | 0 | 0 | 0.00 |
| 5 | specfence | 13.501 | Full | 0 | 0 | 4 | 0 | 0 | 0 | 5 | 770 | 8 | 1 | 770 | 0.445 | 0.066 | 0.533 | 17 | 0 | 0 | 1.00 |
| 6 | occ | 3.959 | — | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 | 0.000 | 0.000 | 0 | 0 | 0 | 0.00 |
| 6 | specfence | 13.950 | Full | 0 | 0 | 5 | 0 | 0 | 0 | 5 | 1069 | 8 | 1 | 1069 | 0.513 | 0.062 | 0.544 | 17 | 0 | 0 | 1.00 |

轨迹摘要：

- 臂: `['Opt', 'Opt', 'Opt', 'Opt', 'Opt', 'Full', 'Full']`
- unfenced: `[3, 7, 3, 3, 3, 4, 5]`
- begin 洞: `[5, 0, 5, 5, 5, 5, 5]`
- pick_occ: `[810, 0, 774, 766, 814, 770, 1069]`
- occ_pick_while_gated: `[810, 0, 774, 766, 814, 770, 1069]`
- end_block_ms: `['0.289', '0.337', '0.300', '0.339', '0.329', '0.445', '0.513']`
- prepaid_ms: `['0.640', '0.000', '0.068', '0.519', '0.041', '0.066', '0.062']`
- refuse: `[0, 0, 0, 0, 0, 0, 0]`
- dp: `[0, 0, 0, 0, 0, 0, 0]`
- Soft: `[0, 0, 0, 0, 0, 0, 0]`（必须全 0）

### 2.7 块 `15274915`

- catalog morph=`mixed_RAW_WAW` L=77 W=1121 RAW=35 WAW=120 bound@8=8.0 in52=True
- PR34 Instant-off：OCC=5.313 SF reuse=79.783 arm=Opt begin=95 unf=7

#### 结构 / 目录

```json
{
  "ok": true,
  "dag": {
    "n_edges": 155,
    "n_raw": 35,
    "n_waw": 120,
    "longest_chain": 77,
    "independent_txs": 1108,
    "independent_frac": 0.9037520391517129,
    "max_wave_width": 1121,
    "mean_wave_width": 15.922077922077921,
    "n_levels": 77,
    "multi_writer_locs": 24,
    "max_writers_on_loc": 77,
    "conflict_component_sizes_top10": [
      77,
      14,
      8,
      4,
      3,
      3,
      3,
      2,
      2,
      2
    ],
    "max_conflict_component": 77
  },
  "kind_histogram": {
    "basic": 201,
    "basic_lazy": 1053,
    "storage": 185,
    "code_hash": 1
  },
  "d1_kind_counts_in_top": {
    "basic_lazy": 4,
    "storage": 5,
    "basic": 3
  },
  "max_basic_writers": 997,
  "max_storage_writers": 4,
  "d1_top_spines": [
    {
      "loc": 9135760758566843158,
      "kind": "basic_lazy",
      "n_writers": 997,
      "writers_head": [
        110,
        111,
        112,
        113,
        114,
        115,
        117,
        118,
        119,
        120,
        121,
        123
      ],
      "writers_tail": [
        1220,
        1221,
        1222,
        1223,
        1224,
        1225
      ]
    },
    {
      "loc": 12382290081011030935,
      "kind": "basic",
      "n_writers": 77,
      "writers_head": [
        116,
        122,
        129,
        131,
        145,
        162,
        177,
        232,
        252,
        288,
        291,
        312
      ],
      "writers_tail": [
        1163,
        1169,
        1182,
        1183,
        1184,
        1219
      ]
    },
    {
      "loc": 16003053646933732166,
      "kind": "basic_lazy",
      "n_writers": 42,
      "writers_head": [
        128,
        130,
        195,
        201,
        202,
        249,
        265,
        276,
        283,
        317,
        353,
        366
      ],
      "writers_tail": [
        1120,
        1128,
        1132,
        1154,
        1189,
        1197
      ]
    },
    {
      "loc": 5204584229706170905,
      "kind": "basic",
      "n_writers": 8,
      "writers_head": [
        73,
        74,
        77,
        78,
        79,
        80,
        81,
        82
      ],
      "writers_tail": []
    },
    {
      "loc": 13758554269703882113,
      "kind": "basic",
      "n_writers": 6,
      "writers_head": [
        30,
        59,
        91,
        92,
        94,
        98
      ],
      "writers_tail": []
    },
    {
      "loc": 1369820900522840264,
      "kind": "basic_lazy",
      "n_writers": 5,
      "writers_head": [
        83,
        84,
        85,
        86,
        87
      ],
      "writers_tail": []
    },
    {
      "loc": 3547232152457480706,
      "kind": "storage",
      "n_writers": 4,
      "writers_head": [
        70,
        97,
        98,
        99
      ],
      "writers_tail": []
    },
    {
      "loc": 4151836190621970488,
      "kind": "storage",
      "n_writers": 4,
      "writers_head": [
        70,
        97,
        98,
        99
      ],
      "writers_tail": []
    },
    {
      "loc": 10181952278015498013,
      "kind": "storage",
      "n_writers": 4,
      "writers_head": [
        26,
        27,
        67,
        95
      ],
      "writers_tail": []
    },
    {
      "loc": 12224527505079642600,
      "kind": "storage",
      "n_writers": 4,
      "writers_head": [
        77,
        78,
        80,
        81
      ],
      "writers_tail": []
    },
    {
      "loc": 15579821769123922296,
      "kind": "storage",
      "n_writers": 4,
      "writers_head": [
        70,
        97,
        98,
        99
      ],
      "writers_tail": []
    },
    {
      "loc": 348090351077845393,
      "kind": "basic_lazy",
      "n_writers": 3,
      "writers_head": [
        26,
        27,
        68
      ],
      "writers_tail": []
    }
  ]
}
```

#### Instant-off 逐 iter（OCC / SF 交错）

| i | mode | wall_ms | arm | w | need | unf | dp | sys | refuse | begin | pick_occ | pick_gate | skip_gate | occ_while_gated | end_ms | prepaid_ms | reexec_ms | oa | commute | soft | ready_w |
|--:|------|--------:|-----|--:|-----:|----:|---:|----:|-------:|------:|---------:|----------:|----------:|----------------:|-------:|-----------:|----------:|---:|--------:|-----:|--------:|
| 0 | occ | 4.658 | — | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 | 0.000 | 0.000 | 0 | 0 | 0 | 0.00 |
| 0 | specfence | 11.506 | Win_1 | 1 | 0 | 39 | 0 | 0 | 0 | 5 | 116 | 7 | 0 | 116 | 0.340 | 0.000 | 2.670 | 17 | 0 | 0 | 0.00 |
| 1 | occ | 4.726 | — | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 | 0.000 | 0.000 | 0 | 0 | 0 | 0.00 |
| 1 | specfence | 15.770 | Win_1 | 1 | 0 | 41 | 0 | 0 | 0 | 6 | 114 | 15 | 0 | 114 | 0.355 | 0.000 | 7.441 | 27 | 0 | 0 | 0.00 |
| 2 | occ | 4.862 | — | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 | 0.000 | 0.000 | 0 | 0 | 0 | 0.00 |
| 2 | specfence | 13.965 | Win_1 | 1 | 0 | 29 | 0 | 0 | 0 | 6 | 1270 | 12 | 0 | 1270 | 0.378 | 0.000 | 5.702 | 24 | 0 | 0 | 0.00 |
| 3 | occ | 4.922 | — | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 | 0.000 | 0.000 | 0 | 0 | 0 | 0.00 |
| 3 | specfence | 13.532 | Win_1 | 1 | 0 | 21 | 0 | 0 | 3 | 7 | 1252 | 12 | 3 | 1252 | 0.423 | 0.029 | 2.990 | 25 | 0 | 0 | 0.00 |
| 4 | occ | 5.164 | — | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 | 0.000 | 0.000 | 0 | 0 | 0 | 0.00 |
| 4 | specfence | 16.391 | Win_1 | 1 | 0 | 63 | 0 | 0 | 4 | 8 | 113 | 11 | 4 | 113 | 0.415 | 0.035 | 3.323 | 26 | 0 | 0 | 0.00 |
| 5 | occ | 5.136 | — | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 | 0.000 | 0.000 | 0 | 0 | 0 | 0.00 |
| 5 | specfence | 16.405 | Win_1 | 1 | 0 | 74 | 0 | 0 | 0 | 7 | 112 | 12 | 0 | 112 | 0.384 | 0.000 | 6.381 | 25 | 0 | 0 | 0.00 |
| 6 | occ | 5.016 | — | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 | 0.000 | 0.000 | 0 | 0 | 0 | 0.00 |
| 6 | specfence | 14.220 | Win_1 | 1 | 0 | 13 | 0 | 0 | 1 | 7 | 110 | 11 | 1 | 110 | 0.408 | 0.041 | 2.163 | 24 | 0 | 0 | 0.00 |

轨迹摘要：

- 臂: `['Win_1', 'Win_1', 'Win_1', 'Win_1', 'Win_1', 'Win_1', 'Win_1']`
- unfenced: `[39, 41, 29, 21, 63, 74, 13]`
- begin 洞: `[5, 6, 6, 7, 8, 7, 7]`
- pick_occ: `[116, 114, 1270, 1252, 113, 112, 110]`
- occ_pick_while_gated: `[116, 114, 1270, 1252, 113, 112, 110]`
- end_block_ms: `['0.340', '0.355', '0.378', '0.423', '0.415', '0.384', '0.408']`
- prepaid_ms: `['0.000', '0.000', '0.000', '0.029', '0.035', '0.000', '0.041']`
- refuse: `[0, 0, 0, 3, 4, 0, 1]`
- dp: `[0, 0, 0, 0, 0, 0, 0]`
- Soft: `[0, 0, 0, 0, 0, 0, 0]`（必须全 0）

### 2.8 块 `13217637`

- catalog morph=`mixed_RAW_WAW` L=6 W=1060 RAW=21 WAW=44 bound@8=8.0 in52=True
- PR34 Instant-off：OCC=4.731 SF reuse=64.264 arm=Win_1 begin=26 unf=7

#### 结构 / 目录

```json
{
  "ok": true,
  "dag": {
    "n_edges": 65,
    "n_raw": 21,
    "n_waw": 44,
    "longest_chain": 6,
    "independent_txs": 1045,
    "independent_frac": 0.95,
    "max_wave_width": 1060,
    "mean_wave_width": 183.33333333333334,
    "n_levels": 6,
    "multi_writer_locs": 28,
    "max_writers_on_loc": 5,
    "conflict_component_sizes_top10": [
      22,
      5,
      4,
      4,
      3,
      3,
      2,
      2,
      2,
      2
    ],
    "max_conflict_component": 22
  },
  "kind_histogram": {
    "storage": 347,
    "code_hash": 1,
    "basic_lazy": 965,
    "basic": 154
  },
  "d1_kind_counts_in_top": {
    "basic": 4,
    "storage": 4,
    "basic_lazy": 4
  },
  "max_basic_writers": 934,
  "max_storage_writers": 4,
  "d1_top_spines": [
    {
      "loc": 14020337404169679390,
      "kind": "basic_lazy",
      "n_writers": 934,
      "writers_head": [
        31,
        32,
        33,
        34,
        35,
        36,
        37,
        38,
        39,
        40,
        41,
        42
      ],
      "writers_tail": [
        991,
        992,
        994,
        995,
        996,
        1002
      ]
    },
    {
      "loc": 8898878514797201221,
      "kind": "basic_lazy",
      "n_writers": 14,
      "writers_head": [
        278,
        602,
        603,
        651,
        652,
        666,
        679,
        680,
        697,
        710,
        768,
        782
      ],
      "writers_tail": [
        697,
        710,
        768,
        782,
        794,
        795
      ]
    },
    {
      "loc": 238884258749349807,
      "kind": "basic_lazy",
      "n_writers": 8,
      "writers_head": [
        1056,
        1057,
        1058,
        1059,
        1060,
        1062,
        1082,
        1083
      ],
      "writers_tail": []
    },
    {
      "loc": 13758554269703882113,
      "kind": "basic",
      "n_writers": 7,
      "writers_head": [
        12,
        993,
        1063,
        1065,
        1073,
        1074,
        1099
      ],
      "writers_tail": []
    },
    {
      "loc": 1369820900522840264,
      "kind": "basic_lazy",
      "n_writers": 6,
      "writers_head": [
        1030,
        1031,
        1032,
        1033,
        1034,
        1035
      ],
      "writers_tail": []
    },
    {
      "loc": 4374904034207278638,
      "kind": "basic",
      "n_writers": 5,
      "writers_head": [
        1050,
        1061,
        1081,
        1085,
        1086
      ],
      "writers_tail": []
    },
    {
      "loc": 13863823162240261549,
      "kind": "basic",
      "n_writers": 4,
      "writers_head": [
        1048,
        1054,
        1055,
        1084
      ],
      "writers_tail": []
    },
    {
      "loc": 15163438867136924100,
      "kind": "storage",
      "n_writers": 4,
      "writers_head": [
        0,
        3,
        30,
        286
      ],
      "writers_tail": []
    },
    {
      "loc": 15885311102163095806,
      "kind": "storage",
      "n_writers": 4,
      "writers_head": [
        651,
        652,
        666,
        679
      ],
      "writers_tail": []
    },
    {
      "loc": 4514499022326566259,
      "kind": "storage",
      "n_writers": 3,
      "writers_head": [
        1048,
        1054,
        1055
      ],
      "writers_tail": []
    },
    {
      "loc": 5204584229706170905,
      "kind": "basic",
      "n_writers": 3,
      "writers_head": [
        1022,
        1025,
        1026
      ],
      "writers_tail": []
    },
    {
      "loc": 5531623313762462144,
      "kind": "storage",
      "n_writers": 3,
      "writers_head": [
        1050,
        1081,
        1086
      ],
      "writers_tail": []
    }
  ]
}
```

#### Instant-off 逐 iter（OCC / SF 交错）

| i | mode | wall_ms | arm | w | need | unf | dp | sys | refuse | begin | pick_occ | pick_gate | skip_gate | occ_while_gated | end_ms | prepaid_ms | reexec_ms | oa | commute | soft | ready_w |
|--:|------|--------:|-----|--:|-----:|----:|---:|----:|-------:|------:|---------:|----------:|----------:|----------------:|-------:|-----------:|----------:|---:|--------:|-----:|--------:|
| 0 | occ | 4.630 | — | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 | 0.000 | 0.000 | 0 | 0 | 0 | 0.00 |
| 0 | specfence | 9.012 | Opt | 0 | 0 | 14 | 0 | 0 | 1 | 8 | 1063 | 10 | 1 | 1063 | 0.256 | 0.236 | 1.916 | 92 | 0 | 0 | 0.00 |
| 1 | occ | 4.412 | — | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 | 0.000 | 0.000 | 0 | 0 | 0 | 0.00 |
| 1 | specfence | 12.663 | Opt | 0 | 0 | 8 | 0 | 0 | 1 | 8 | 1079 | 12 | 1 | 1079 | 0.316 | 0.393 | 1.935 | 44 | 0 | 0 | 0.00 |
| 2 | occ | 4.862 | — | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 | 0.000 | 0.000 | 0 | 0 | 0 | 0.00 |
| 2 | specfence | 12.534 | Opt | 0 | 0 | 11 | 0 | 0 | 1 | 8 | 1068 | 9 | 1 | 1068 | 0.300 | 0.394 | 3.172 | 81 | 0 | 0 | 0.00 |
| 3 | occ | 4.745 | — | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 | 0.000 | 0.000 | 0 | 0 | 0 | 0.00 |
| 3 | specfence | 13.014 | Full | 0 | 0 | 13 | 0 | 0 | 1 | 8 | 1079 | 9 | 1 | 1079 | 0.425 | 0.406 | 3.296 | 72 | 0 | 0 | 0.00 |
| 4 | occ | 6.296 | — | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 | 0.000 | 0.000 | 0 | 0 | 0 | 0.00 |
| 4 | specfence | 13.024 | Full | 0 | 0 | 13 | 0 | 0 | 1 | 8 | 1084 | 11 | 1 | 1084 | 0.398 | 0.160 | 2.914 | 122 | 0 | 0 | 0.00 |
| 5 | occ | 4.803 | — | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 | 0.000 | 0.000 | 0 | 0 | 0 | 0.00 |
| 5 | specfence | 12.820 | Full | 0 | 0 | 12 | 1 | 0 | 1 | 8 | 1083 | 9 | 1 | 1083 | 0.461 | 0.315 | 3.027 | 66 | 0 | 0 | 0.00 |
| 6 | occ | 4.733 | — | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.000 | 0.000 | 0.000 | 0 | 0 | 0 | 0.00 |
| 6 | specfence | 12.671 | Full | 0 | 0 | 7 | 0 | 1 | 1 | 8 | 1067 | 11 | 1 | 1067 | 0.410 | 0.375 | 4.878 | 50 | 0 | 0 | 0.00 |

轨迹摘要：

- 臂: `['Opt', 'Opt', 'Opt', 'Full', 'Full', 'Full', 'Full']`
- unfenced: `[14, 8, 11, 13, 13, 12, 7]`
- begin 洞: `[8, 8, 8, 8, 8, 8, 8]`
- pick_occ: `[1063, 1079, 1068, 1079, 1084, 1083, 1067]`
- occ_pick_while_gated: `[1063, 1079, 1068, 1079, 1084, 1083, 1067]`
- end_block_ms: `['0.256', '0.316', '0.300', '0.425', '0.398', '0.461', '0.410']`
- prepaid_ms: `['0.236', '0.393', '0.394', '0.406', '0.160', '0.315', '0.375']`
- refuse: `[1, 1, 1, 1, 1, 1, 1]`
- dp: `[0, 0, 0, 0, 0, 1, 0]`
- Soft: `[0, 0, 0, 0, 0, 0, 0]`（必须全 0）

## 3. PROFILE Instant-tax（单独进程，不可加总进墙）

这些桶是 worker 求和 Instant，**量级参考、不是墙分解**。PROFILE 可能改学习轨迹，所以也不用这节的臂当主证。

### `19807137` Instant-tax

- labeled PRIMARY_is=`Instant-tax reuse median (not wall PRIMARY)` profile_on=`True`
- 规则: worker-sum Instant; do not add into wall; PROFILE can change learn

| i | mode | wall_ms | learn | handler_ns | maybe_wait_ns | validate_ns | scheduler_ns | idle_core_ns |
|--:|------|--------:|-------|-----------:|--------------:|------------:|-------------:|-------------:|
| 0 | occ | 2283.453 | — | 2317603788 | 0 | 17519718 | 15875365666 | 0 |
| 0 | specfence | 41.403 | Opt | 37556006 | 0 | 33114539 | 127204344 | 0 |
| 1 | occ | 15.882 | — | 40098033 | 0 | 13540317 | 30635257 | 0 |
| 1 | specfence | 55.473 | Win_1 | 68302834 | 0 | 60072804 | 106736576 | 0 |
| 2 | occ | 17.434 | — | 36031173 | 0 | 17020763 | 36942839 | 0 |
| 2 | specfence | 55.043 | Win_2 | 73454551 | 0 | 48491012 | 118776308 | 0 |
| 3 | occ | 14.197 | — | 28938099 | 0 | 20564611 | 22523346 | 0 |
| 3 | specfence | 59.497 | Opt | 84440483 | 0 | 64389485 | 115770486 | 0 |
| 4 | occ | 17.513 | — | 46321001 | 0 | 9754452 | 35035149 | 0 |
| 4 | specfence | 58.390 | Seg_2 | 74585716 | 0 | 57892802 | 120116487 | 0 |
| 5 | occ | 17.146 | — | 39781371 | 0 | 15330583 | 44331362 | 0 |
| 5 | specfence | 52.361 | Defer | 75213757 | 0 | 58767326 | 84362344 | 0 |
| 6 | occ | 14.455 | — | 30377953 | 0 | 13578301 | 24453261 | 0 |
| 6 | specfence | 61.544 | Seg_3 | 90156916 | 0 | 63401176 | 135816116 | 0 |

### `19434587` Instant-tax

- labeled PRIMARY_is=`Instant-tax reuse median (not wall PRIMARY)` profile_on=`True`
- 规则: worker-sum Instant; do not add into wall; PROFILE can change learn

| i | mode | wall_ms | learn | handler_ns | maybe_wait_ns | validate_ns | scheduler_ns | idle_core_ns |
|--:|------|--------:|-------|-----------:|--------------:|------------:|-------------:|-------------:|
| 0 | occ | 13.750 | — | 44420593 | 0 | 5546396 | 19752848 | 0 |
| 0 | specfence | 19.991 | Opt | 37252309 | 0 | 2651614 | 46743747 | 5295189 |
| 1 | occ | 13.281 | — | 42445073 | 0 | 10958592 | 29115906 | 0 |
| 1 | specfence | 22.742 | Win_1 | 36528098 | 0 | 10678259 | 27918273 | 896053 |
| 2 | occ | 11.121 | — | 40130833 | 0 | 7200551 | 7799357 | 0 |
| 2 | specfence | 24.720 | Win_2 | 44766108 | 0 | 10050716 | 43210016 | 6700699 |
| 3 | occ | 14.298 | — | 42804204 | 0 | 6341645 | 27188198 | 0 |
| 3 | specfence | 23.501 | Opt | 41041171 | 0 | 2548585 | 42808000 | 3131237 |
| 4 | occ | 9.846 | — | 35589016 | 0 | 4153375 | 8276787 | 0 |
| 4 | specfence | 22.155 | Win_8 | 37748937 | 0 | 4218950 | 45450233 | 3129817 |
| 5 | occ | 10.711 | — | 34394191 | 0 | 6654858 | 11336800 | 0 |
| 5 | specfence | 26.695 | Defer | 61307940 | 0 | 7733166 | 38853371 | 5433004 |
| 6 | occ | 13.516 | — | 44453219 | 0 | 1311204 | 30139348 | 0 |
| 6 | specfence | 25.562 | Seg_7 | 44519056 | 0 | 7774456 | 34336865 | 2020256 |

### `19606599` Instant-tax

- labeled PRIMARY_is=`Instant-tax reuse median (not wall PRIMARY)` profile_on=`True`
- 规则: worker-sum Instant; do not add into wall; PROFILE can change learn

| i | mode | wall_ms | learn | handler_ns | maybe_wait_ns | validate_ns | scheduler_ns | idle_core_ns |
|--:|------|--------:|-------|-----------:|--------------:|------------:|-------------:|-------------:|
| 0 | occ | 10.736 | — | 39106225 | 0 | 6283817 | 13806080 | 0 |
| 0 | specfence | 19.736 | Opt | 40812276 | 0 | 4343277 | 28025113 | 9871740 |
| 1 | occ | 12.538 | — | 37944874 | 0 | 3135346 | 31048448 | 0 |
| 1 | specfence | 21.426 | Win_1 | 38041347 | 0 | 7680852 | 28836887 | 11753259 |
| 2 | occ | 10.668 | — | 37512489 | 0 | 5620708 | 20095218 | 0 |
| 2 | specfence | 408.551 | Win_2 | 48850319 | 0 | 5053287 | 2335303148 | 907275322 |
| 3 | occ | 12.640 | — | 43871465 | 0 | 6988458 | 20647202 | 0 |
| 3 | specfence | 23.292 | Opt | 37495330 | 0 | 2809033 | 48193666 | 15033615 |
| 4 | occ | 11.904 | — | 37939738 | 0 | 3376203 | 25635207 | 0 |
| 4 | specfence | 22.796 | Win_8 | 35944938 | 0 | 2406422 | 45775651 | 11159363 |
| 5 | occ | 12.654 | — | 41125301 | 0 | 2135126 | 31189055 | 0 |
| 5 | specfence | 24.318 | Defer | 46872412 | 0 | 10317108 | 43448265 | 9220262 |
| 6 | occ | 14.371 | — | 47875377 | 0 | 10800327 | 16633379 | 0 |
| 6 | specfence | 17.216 | Seg_7 | 32620109 | 0 | 1181805 | 24616627 | 4895813 |

### `19716145` Instant-tax

- labeled PRIMARY_is=`Instant-tax reuse median (not wall PRIMARY)` profile_on=`True`
- 规则: worker-sum Instant; do not add into wall; PROFILE can change learn

| i | mode | wall_ms | learn | handler_ns | maybe_wait_ns | validate_ns | scheduler_ns | idle_core_ns |
|--:|------|--------:|-------|-----------:|--------------:|------------:|-------------:|-------------:|
| 0 | occ | 9.006 | — | 25172157 | 0 | 5458262 | 12044212 | 0 |
| 0 | specfence | 20.832 | Win_1 | 32120536 | 0 | 13775554 | 45766283 | 2370939 |
| 1 | occ | 9.386 | — | 27509286 | 0 | 9093674 | 12294947 | 0 |
| 1 | specfence | 17.628 | Win_2 | 30763288 | 0 | 6755800 | 22965217 | 934601 |
| 2 | occ | 11.000 | — | 32792397 | 0 | 6666535 | 18410512 | 0 |
| 2 | specfence | 21.813 | Win_1 | 40595204 | 0 | 9078240 | 20730248 | 1384891 |
| 3 | occ | 8.942 | — | 31088379 | 0 | 6878204 | 11201782 | 0 |
| 3 | specfence | 21.457 | Win_2 | 41070264 | 0 | 4234767 | 30893869 | 1840843 |
| 4 | occ | 9.782 | — | 27340493 | 0 | 10565431 | 12120007 | 0 |
| 4 | specfence | 32.208 | Opt | 67135620 | 0 | 9376457 | 53608471 | 1965795 |
| 5 | occ | 8.266 | — | 24064167 | 0 | 3188261 | 10653277 | 0 |
| 5 | specfence | 18.309 | Win_8 | 31987668 | 0 | 7046900 | 27272772 | 1862378 |
| 6 | occ | 10.478 | — | 29427826 | 0 | 8382818 | 16494273 | 0 |
| 6 | specfence | 18.631 | Defer | 32382627 | 0 | 3469174 | 30736289 | 1748619 |

### `19860366` Instant-tax

- labeled PRIMARY_is=`Instant-tax reuse median (not wall PRIMARY)` profile_on=`True`
- 规则: worker-sum Instant; do not add into wall; PROFILE can change learn

| i | mode | wall_ms | learn | handler_ns | maybe_wait_ns | validate_ns | scheduler_ns | idle_core_ns |
|--:|------|--------:|-------|-----------:|--------------:|------------:|-------------:|-------------:|
| 0 | occ | 9.192 | — | 25765463 | 0 | 14441193 | 6184003 | 0 |
| 0 | specfence | 22.382 | Win_1 | 22849791 | 0 | 3625020 | 56669965 | 13294127 |
| 1 | occ | 10.611 | — | 31535513 | 0 | 6030785 | 15876913 | 0 |
| 1 | specfence | 19.127 | Win_2 | 23769368 | 0 | 3062413 | 22038682 | 1374650 |
| 2 | occ | 9.866 | — | 29870471 | 0 | 8711430 | 7862756 | 0 |
| 2 | specfence | 21.352 | Opt | 27425985 | 0 | 2547379 | 19972288 | 4016051 |
| 3 | occ | 9.551 | — | 27034022 | 0 | 3005948 | 14934306 | 0 |
| 3 | specfence | 19.567 | Win_5 | 27950952 | 0 | 2604146 | 27382830 | 3107788 |
| 4 | occ | 11.364 | — | 31494680 | 0 | 8319753 | 14508700 | 0 |
| 4 | specfence | 19.107 | Defer | 22980938 | 0 | 2879304 | 23210365 | 4121721 |
| 5 | occ | 8.940 | — | 26824505 | 0 | 11048197 | 7311949 | 0 |
| 5 | specfence | 19.672 | Win_6 | 27940507 | 0 | 5574530 | 16443561 | 3867365 |
| 6 | occ | 9.514 | — | 25844976 | 0 | 9926253 | 8787636 | 0 |
| 6 | specfence | 19.091 | Seg_4 | 22774406 | 0 | 1781237 | 23499677 | 5664902 |

### `14396881` Instant-tax

- labeled PRIMARY_is=`Instant-tax reuse median (not wall PRIMARY)` profile_on=`True`
- 规则: worker-sum Instant; do not add into wall; PROFILE can change learn

| i | mode | wall_ms | learn | handler_ns | maybe_wait_ns | validate_ns | scheduler_ns | idle_core_ns |
|--:|------|--------:|-------|-----------:|--------------:|------------:|-------------:|-------------:|
| 0 | occ | 3.922 | — | 2713629 | 0 | 665635 | 1688731 | 0 |
| 0 | specfence | 11.113 | Full | 2222681 | 0 | 2904521 | 1990746 | 0 |
| 1 | occ | 4.106 | — | 2807979 | 0 | 633661 | 1549499 | 0 |
| 1 | specfence | 13.933 | Full | 2317157 | 0 | 582048 | 2121145 | 0 |
| 2 | occ | 4.489 | — | 2473627 | 0 | 723453 | 1889255 | 0 |
| 2 | specfence | 13.732 | Full | 2460363 | 0 | 595609 | 3208865 | 0 |
| 3 | occ | 4.308 | — | 2403127 | 0 | 667359 | 1795757 | 0 |
| 3 | specfence | 13.863 | Full | 2351259 | 0 | 601274 | 2164998 | 0 |
| 4 | occ | 4.365 | — | 2417526 | 0 | 804464 | 1639896 | 0 |
| 4 | specfence | 13.777 | Full | 3127272 | 0 | 584884 | 2410727 | 0 |
| 5 | occ | 4.846 | — | 2766103 | 0 | 904873 | 2114706 | 0 |
| 5 | specfence | 14.149 | Full | 2311141 | 0 | 606496 | 2436794 | 0 |
| 6 | occ | 4.090 | — | 2714055 | 0 | 770487 | 1774473 | 0 |
| 6 | specfence | 13.935 | Full | 2413212 | 0 | 1191998 | 2273517 | 0 |

### `15274915` Instant-tax

- labeled PRIMARY_is=`Instant-tax reuse median (not wall PRIMARY)` profile_on=`True`
- 规则: worker-sum Instant; do not add into wall; PROFILE can change learn

| i | mode | wall_ms | learn | handler_ns | maybe_wait_ns | validate_ns | scheduler_ns | idle_core_ns |
|--:|------|--------:|-------|-----------:|--------------:|------------:|-------------:|-------------:|
| 0 | occ | 5.118 | — | 5287457 | 0 | 1092649 | 3881923 | 0 |
| 0 | specfence | 11.452 | Full | 4951627 | 0 | 2933743 | 6429307 | 0 |
| 1 | occ | 5.376 | — | 5775541 | 0 | 2372719 | 1841763 | 0 |
| 1 | specfence | 13.999 | Full | 5873776 | 0 | 1211870 | 2474347 | 0 |
| 2 | occ | 5.076 | — | 5512294 | 0 | 1732657 | 2258126 | 0 |
| 2 | specfence | 14.195 | Full | 5453887 | 0 | 1095030 | 2420506 | 0 |
| 3 | occ | 4.868 | — | 6214792 | 0 | 969204 | 3136703 | 0 |
| 3 | specfence | 15.619 | Full | 6287390 | 0 | 4058468 | 14133720 | 0 |
| 4 | occ | 5.307 | — | 4917492 | 0 | 2253537 | 2608713 | 0 |
| 4 | specfence | 13.860 | Full | 5552879 | 0 | 988008 | 1793751 | 0 |
| 5 | occ | 5.156 | — | 4883335 | 0 | 1147083 | 2686899 | 0 |
| 5 | specfence | 14.053 | Full | 6698599 | 0 | 909043 | 1737023 | 0 |
| 6 | occ | 5.385 | — | 6595983 | 0 | 2010961 | 4489145 | 0 |
| 6 | specfence | 13.713 | Full | 5039046 | 0 | 815703 | 1861460 | 0 |

### `13217637` Instant-tax

- labeled PRIMARY_is=`Instant-tax reuse median (not wall PRIMARY)` profile_on=`True`
- 规则: worker-sum Instant; do not add into wall; PROFILE can change learn

| i | mode | wall_ms | learn | handler_ns | maybe_wait_ns | validate_ns | scheduler_ns | idle_core_ns |
|--:|------|--------:|-------|-----------:|--------------:|------------:|-------------:|-------------:|
| 0 | occ | 4.721 | — | 6682322 | 0 | 885036 | 1266646 | 0 |
| 0 | specfence | 9.564 | Opt | 6093119 | 0 | 1534176 | 1448620 | 0 |
| 1 | occ | 4.527 | — | 6004373 | 0 | 938772 | 1410424 | 0 |
| 1 | specfence | 13.150 | Opt | 6682571 | 0 | 826124 | 1420583 | 0 |
| 2 | occ | 4.640 | — | 5836560 | 0 | 1522670 | 1052730 | 0 |
| 2 | specfence | 13.566 | Opt | 7363056 | 0 | 784955 | 2151057 | 0 |
| 3 | occ | 9.145 | — | 21153785 | 0 | 1989736 | 1963065 | 0 |
| 3 | specfence | 13.076 | Opt | 6800179 | 0 | 728502 | 1319319 | 0 |
| 4 | occ | 11.238 | — | 24541589 | 0 | 1615850 | 2057761 | 0 |
| 4 | specfence | 13.301 | Opt | 6507186 | 0 | 752859 | 2183403 | 0 |
| 5 | occ | 4.948 | — | 7123463 | 0 | 1472341 | 2195316 | 0 |
| 5 | specfence | 12.890 | Opt | 6337318 | 0 | 952451 | 1727706 | 0 |
| 6 | occ | 4.798 | — | 7247899 | 0 | 1086213 | 1634401 | 0 |
| 6 | specfence | 12.768 | Opt | 6470983 | 0 | 645197 | 1593809 | 0 |

## 4. Soft=0

Instant-off / PROFILE 两套 JSON 的 `soft` 均为 0；逐 iter `soft_wait_arms` 见上表。

