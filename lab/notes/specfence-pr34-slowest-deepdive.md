# PR #34 最慢 8 块：多方面 + 开销细粒度深挖（详版）

**文档 PR:** https://github.com/fengjy73/pevm/pull/35（draft，仅分析）  
**基线 tip:** `0144211ae115c87eb2e80828e17e0750d3e2cf6b` · Soft=0 · cores=8 · 物理核=4 · N=7  
**规则:** 不发明 ns；Instant-tax（PROFILE worker 求和）**不可**加总成墙；单时钟 `end_block_ns` 可与墙比量级。  
**摘要 JSON:** [`specfence-pr34-slowest-deepdive-summary.json`](specfence-pr34-slowest-deepdive-summary.json)  
**全量扫块:** [`specfence-pr34-allblocks-sweep.md`](specfence-pr34-allblocks-sweep.md)

---

## 0. 总定位

**一句话:** 全集最慢尾是 **n≈700–1300 肥块**：OCC 把 **basic_lazy 数百～千级连续写者**当廉价重叠更新；SpecFence 把同一 ℓ 学成 Win/Full/Defer，种 **数个～近百 begin 洞** → 暖机 **`pick_occ_n≈0`（整块离开 OCC pick）**，再付 **0.3–3 ms `end_block`**。局部 unfenced 常为个位数 → **局部 CC 不惨、整块 4–27×**。

这与 3356896「薄 WAW、cover 后 unf≈0、仍薄输 ~0.2 ms」**不同问题**；也与 PR34 预付刀吃掉的 U/D/L4 **正交**。

## 1. K8 Instant-off 主证墙（PRIMARY 口径）

| block | n | OCC med | SF cold | SF reuse | × | last arm | unf(last) | begin_n | pick_occ(暖) | end_block(暖) |
|------:|--:|--------:|--------:|---------:|--:|----------|----------:|--------:|-------------:|--------------:|
| 14396881 | 1346 | 4.403 | 107.4 | **109.9** | 25.0 | Defer | 4 | 6 | 0–0 | 0.59–1.25 ms |
| 15274915 | 1226 | 5.313 | 238.1 | **79.0** | 14.9 | Opt | 7 | 95 | 0–0 | 0.34–1.29 ms |
| 13217637 | 1100 | 4.731 | 68.8 | **64.1** | 13.5 | Win_1 | 7 | 26 | 0–1 | 0.29–1.36 ms |
| 19807137 | 712 | 17.590 | 50.2 | **64.7** | 3.7 | Win_1 | 236 | 38 | 0–3 | 0.50–1.73 ms |
| 17666333 | 961 | 7.374 | 33.8 | **35.1** | 4.8 | Win_1 | 13 | 36 | 0–0 | 0.36–1.98 ms |
| 15538827 | 823 | 5.675 | 29.5 | **28.6** | 5.0 | Win_1 | 21 | 85 | 0–0 | 0.31–3.07 ms |
| 14334629 | 819 | 6.246 | 27.8 | **27.1** | 4.3 | Full | 14 | 63 | 0–0 | 0.35–3.06 ms |
| 15199017 | 866 | 4.276 | 20.5 | **21.9** | 5.1 | Win_1 | 3 | 44 | 0–0 | 0.35–0.40 ms |

> 上表若摘要字段缺失，由逐 iter 重算；以 JSON `instant_off[].sf_iters` 为准。

---

## 2. 逐块细挖

### 2.x 块 14396881

#### 结构 / 目录

```json
{
  "ok": true,
  "dag": {
    "n_raw": 0,
    "n_waw": 13,
    "longest_chain": 5,
    "max_wave_width": 1337,
    "max_writers_on_loc": 5,
    "independent_frac": 0.9888558692421991,
    "multi_writer_locs": 9
  },
  "kind_histogram": {
    "basic": 64,
    "code_hash": 1,
    "basic_lazy": 1307,
    "storage": 86
  },
  "d1_kind_counts_in_top": {
    "basic_lazy": 7,
    "basic": 4,
    "storage": 1
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
    }
  ]
}
```

<details><summary>catalog 摘录</summary>

```json
{
  "morph": "near_independent_meta_gap",
  "selected": false,
  "L": 5,
  "W": 1337,
  "n_raw": 0,
  "n_waw": 13,
  "bound_at_8": 8.0,
  "max_writers_on_loc": 5
}
```

</details>

#### summary 字段

| key | value |
|-----|-------|
| `block` | `14396881` |
| `n_tx` | `1346` |
| `soft` | `0` |
| `profile_on` | `False` |
| `primary_is` | `Instant-off reuse median` |
| `occ_median_ms` | `4.403474999999999` |
| `sf_cold_ms` | `107.35618099999999` |
| `sf_reuse_median_ms` | `110.887096` |
| `sf_all_median_ms` | `108.97802700000001` |
| `sf_le_occ` | `False` |
| `last_arm` | `Defer` |
| `last_w_need` | `2` |
| `last_unfenced` | `4` |
| `last_double_pay` | `0` |
| `last_begin_n` | `6` |

#### OCC 逐 iter

| wall_ms | occ_aborts | soft_wait_arms | begin_blocked | begin_blocked_n | chosen_strategy | chosen_w_need | chosen_win_w | commute_skip | covering_n | double_pay_n | edge_ordered_admit | end_block_ns | gate_stall_ns | i |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 5.045898 | 3 | 0 | [] | 0 |  | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| 4.2023399999999995 | 3 | 0 | [] | 0 |  | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 1 |
| 4.31978 | 5 | 0 | [] | 0 |  | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 2 |
| 4.403474999999999 | 6 | 0 | [] | 0 |  | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 3 |
| 4.509721000000001 | 3 | 0 | [] | 0 |  | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 4 |
| 3.9168450000000004 | 6 | 0 | [] | 0 |  | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 5 |
| 4.443614999999999 | 4 | 0 | [] | 0 |  | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 6 |

#### SpecFence 逐 iter（Instant-off）

| wall_ms | chosen_strategy | chosen_win_w | chosen_w_need | unfenced_reexec | double_pay_n | begin_blocked_n | pick_occ_n | refuse_admit | wait_for_dependency | commute_skip | end_block_ns | reexec_ns | prepaid_ns | refuse_ns | gate_stall_ns | incarnation_gt0 | soft_wait_arms | covering_n | sys_reexec_n | selected_arms | edge_ordered_admit |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 107.35618099999999 | Win_1 | 1 | 1 | 3 | 0 | 9 | 1 | 7 | 0 | 0 | 0.864ms | 0.538ms | 0.063ms | 0.063ms | 0.063ms | 6 | 0 | 1 | 0 | ca19828886961863:Win_1/59 | 20 |
| 108.97802700000001 | Win_1 | 1 | 1 | 1 | 0 | 9 | 0 | 8 | 0 | 0 | 0.896ms | 0.306ms | 0.057ms | 0.057ms | 0.057ms | 3 | 0 | 1 | 0 | ca19828886961863:Win_1/59 | 19 |
| 99.471796 | Win_1 | 1 | 1 | 1 | 0 | 9 | 0 | 9 | 0 | 0 | 0.899ms | 8.332ms | 3.148ms | 3.148ms | 3.148ms | 4 | 0 | 1 | 0 | ca19828886961863:Win_1/59 | 19 |
| 104.20855499999999 | Win_1 | 1 | 1 | 3 | 0 | 9 | 0 | 6 | 0 | 0 | 1.215ms | 0.853ms | 2.582ms | 2.582ms | 2.582ms | 5 | 0 | 2 | 0 | a4e0371c68ff48ae:Win_1/1196,ca1982888696 | 20 |
| 110.887096 | Opt | 0 | 1 | 2 | 0 | 9 | 0 | 7 | 0 | 0 | 1.224ms | 0.551ms | 32.726ms | 32.726ms | 32.726ms | 5 | 0 | 1 | 0 | a4e0371c68ff48ae:Opt/1196,ca198288869618 | 20 |
| 111.03238 | Defer | 0 | 1 | 3 | 0 | 6 | 0 | 156 | 0 | 0 | 1.248ms | 1.132ms | 1.015ms | 1.015ms | 1.015ms | 6 | 0 | 1 | 0 | a4e0371c68ff48ae:Defer/1196,ca1982888696 | 69 |
| 115.432881 | Defer | 0 | 2 | 4 | 0 | 6 | 0 | 4 | 0 | 0 | 0.592ms | 0.421ms | 4.384ms | 4.384ms | 4.384ms | 7 | 0 | 1 | 1 | a4e0371c68ff48ae:Defer/1196,ca1982888696 | 12 |

**本块观测要点:**

- SF wall 范围 **99.47–115.43 ms**（冷=107.36）
- unfenced 逐 iter: [3, 1, 1, 3, 2, 3, 4]
- begin 洞数逐 iter: [9, 9, 9, 9, 9, 6, 6]
- pick_occ 逐 iter: [1, 0, 0, 0, 0, 0, 0]（暖机多为 0 ⇒ 整块不在 OCC pick）
- end_block **0.59–1.25 ms**（单时钟）
- 必要 vs 不必要：真 WAW/storage 脊必要；**lazy 长链 OrderedAdmit + 整块离开 OCC pick + 肥 end_block** 相对 OCC 为不必要壳。

### 2.x 块 15274915

#### 结构 / 目录

```json
{
  "ok": true,
  "dag": {
    "n_raw": 35,
    "n_waw": 120,
    "longest_chain": 77,
    "max_wave_width": 1121,
    "max_writers_on_loc": 77,
    "independent_frac": 0.9037520391517129,
    "multi_writer_locs": 24
  },
  "kind_histogram": {
    "storage": 185,
    "basic": 200,
    "basic_lazy": 1054,
    "code_hash": 1
  },
  "d1_kind_counts_in_top": {
    "basic": 3,
    "basic_lazy": 4,
    "storage": 5
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
    }
  ]
}
```

<details><summary>catalog 摘录</summary>

```json
{
  "morph": "mixed_RAW_WAW",
  "selected": true,
  "L": 77,
  "W": 1121,
  "n_raw": 35,
  "n_waw": 120,
  "bound_at_8": 8.0,
  "max_writers_on_loc": 77
}
```

</details>

#### summary 字段

| key | value |
|-----|-------|
| `block` | `15274915` |
| `n_tx` | `1226` |
| `soft` | `0` |
| `profile_on` | `False` |
| `primary_is` | `Instant-off reuse median` |
| `occ_median_ms` | `5.312949` |
| `sf_cold_ms` | `238.135562` |
| `sf_reuse_median_ms` | `79.783422` |
| `sf_all_median_ms` | `79.783422` |
| `sf_le_occ` | `False` |
| `last_arm` | `Opt` |
| `last_w_need` | `0` |
| `last_unfenced` | `7` |
| `last_double_pay` | `0` |
| `last_begin_n` | `95` |

#### OCC 逐 iter

| wall_ms | occ_aborts | soft_wait_arms | begin_blocked | begin_blocked_n | chosen_strategy | chosen_w_need | chosen_win_w | commute_skip | covering_n | double_pay_n | edge_ordered_admit | end_block_ns | gate_stall_ns | i |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 4.682101 | 72 | 0 | [] | 0 |  | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| 5.3450809999999995 | 103 | 0 | [] | 0 |  | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 1 |
| 5.0522279999999995 | 75 | 0 | [] | 0 |  | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 2 |
| 5.427137 | 102 | 0 | [] | 0 |  | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 3 |
| 5.8990800000000005 | 104 | 0 | [] | 0 |  | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 4 |
| 5.048897 | 64 | 0 | [] | 0 |  | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 5 |
| 5.312949 | 101 | 0 | [] | 0 |  | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 6 |

#### SpecFence 逐 iter（Instant-off）

| wall_ms | chosen_strategy | chosen_win_w | chosen_w_need | unfenced_reexec | double_pay_n | begin_blocked_n | pick_occ_n | refuse_admit | wait_for_dependency | commute_skip | end_block_ns | reexec_ns | prepaid_ns | refuse_ns | gate_stall_ns | incarnation_gt0 | soft_wait_arms | covering_n | sys_reexec_n | selected_arms | edge_ordered_admit |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 238.135562 | Opt | 0 | 0 | 7 | 0 | 95 | 0 | 433 | 0 | 0 | 1.226ms | 1.401ms | 200.995ms | 200.995ms | 200.995ms | 13 | 0 | 0 | 0 |  | 121 |
| 77.098251 | Opt | 0 | 0 | 3 | 0 | 95 | 0 | 78 | 3 | 0 | 1.164ms | 1.619ms | 29.250ms | 29.250ms | 29.250ms | 9 | 0 | 0 | 0 |  | 122 |
| 79.783422 | Opt | 0 | 0 | 3 | 0 | 95 | 0 | 99 | 2 | 0 | 1.265ms | 1.742ms | 38.458ms | 38.458ms | 38.458ms | 11 | 0 | 0 | 0 |  | 932 |
| 96.102343 | Full | 0 | 0 | 7 | 0 | 95 | 0 | 84 | 0 | 0 | 1.293ms | 8.163ms | 69.452ms | 69.452ms | 69.452ms | 14 | 0 | 0 | 0 | 313a4cd93f128a02:Full/3 | 143 |
| 78.153526 | Win_1 | 1 | 0 | 4 | 0 | 95 | 0 | 91 | 2 | 0 | 0.506ms | 1.247ms | 36.477ms | 36.477ms | 36.477ms | 7 | 0 | 1 | 0 | 313a4cd93f128a02:Win_1/4 | 116 |
| 77.922419 | Win_1 | 1 | 0 | 6 | 0 | 95 | 0 | 102 | 2 | 0 | 0.343ms | 2.598ms | 61.591ms | 61.591ms | 61.591ms | 8 | 0 | 1 | 0 | 313a4cd93f128a02:Win_1/4 | 119 |
| 86.34238599999999 | Opt | 0 | 0 | 7 | 0 | 95 | 0 | 90 | 1 | 0 | 0.345ms | 9.648ms | 29.150ms | 29.150ms | 29.150ms | 14 | 0 | 0 | 0 |  | 150 |

**本块观测要点:**

- SF wall 范围 **77.10–238.14 ms**（冷=238.14）
- unfenced 逐 iter: [7, 3, 3, 7, 4, 6, 7]
- begin 洞数逐 iter: [95, 95, 95, 95, 95, 95, 95]
- pick_occ 逐 iter: [0, 0, 0, 0, 0, 0, 0]（暖机多为 0 ⇒ 整块不在 OCC pick）
- end_block **0.34–1.29 ms**（单时钟）
- 必要 vs 不必要：真 WAW/storage 脊必要；**lazy 长链 OrderedAdmit + 整块离开 OCC pick + 肥 end_block** 相对 OCC 为不必要壳。

### 2.x 块 13217637

#### 结构 / 目录

```json
{
  "ok": true,
  "dag": {
    "n_raw": 21,
    "n_waw": 44,
    "longest_chain": 6,
    "max_wave_width": 1060,
    "max_writers_on_loc": 5,
    "independent_frac": 0.95,
    "multi_writer_locs": 28
  },
  "kind_histogram": {
    "basic": 155,
    "storage": 347,
    "code_hash": 1,
    "basic_lazy": 964
  },
  "d1_kind_counts_in_top": {
    "storage": 4,
    "basic_lazy": 4,
    "basic": 4
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
      "loc": 13758554269703882113,
      "kind": "basic",
      "n_writers": 6,
      "writers_head": [
        12,
        993,
        1063,
        1065,
        1074,
        1099
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
    }
  ]
}
```

<details><summary>catalog 摘录</summary>

```json
{
  "morph": "mixed_RAW_WAW",
  "selected": true,
  "L": 6,
  "W": 1060,
  "n_raw": 21,
  "n_waw": 44,
  "bound_at_8": 8.0,
  "max_writers_on_loc": 5
}
```

</details>

#### summary 字段

| key | value |
|-----|-------|
| `block` | `13217637` |
| `n_tx` | `1100` |
| `soft` | `0` |
| `profile_on` | `False` |
| `primary_is` | `Instant-off reuse median` |
| `occ_median_ms` | `4.731110999999999` |
| `sf_cold_ms` | `68.799318` |
| `sf_reuse_median_ms` | `64.264355` |
| `sf_all_median_ms` | `64.264355` |
| `sf_le_occ` | `False` |
| `last_arm` | `Win_1` |
| `last_w_need` | `0` |
| `last_unfenced` | `7` |
| `last_double_pay` | `0` |
| `last_begin_n` | `26` |

#### OCC 逐 iter

| wall_ms | occ_aborts | soft_wait_arms | begin_blocked | begin_blocked_n | chosen_strategy | chosen_w_need | chosen_win_w | commute_skip | covering_n | double_pay_n | edge_ordered_admit | end_block_ns | gate_stall_ns | i |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 4.731110999999999 | 10 | 0 | [] | 0 |  | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| 4.707282 | 4 | 0 | [] | 0 |  | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 1 |
| 7.888852999999999 | 15 | 0 | [] | 0 |  | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 2 |
| 4.599261 | 7 | 0 | [] | 0 |  | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 3 |
| 4.842113 | 5 | 0 | [] | 0 |  | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 4 |
| 5.109595 | 20 | 0 | [] | 0 |  | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 5 |
| 4.400369 | 8 | 0 | [] | 0 |  | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 6 |

#### SpecFence 逐 iter（Instant-off）

| wall_ms | chosen_strategy | chosen_win_w | chosen_w_need | unfenced_reexec | double_pay_n | begin_blocked_n | pick_occ_n | refuse_admit | wait_for_dependency | commute_skip | end_block_ns | reexec_ns | prepaid_ns | refuse_ns | gate_stall_ns | incarnation_gt0 | soft_wait_arms | covering_n | sys_reexec_n | selected_arms | edge_ordered_admit |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 68.799318 | Full | 0 | 0 | 6 | 0 | 29 | 0 | 11 | 0 | 0 | 0.275ms | 0.844ms | 0.394ms | 0.394ms | 0.394ms | 8 | 0 | 0 | 0 | f377c334c12229f8:Full/2 | 98 |
| 68.350346 | Full | 0 | 0 | 9 | 0 | 29 | 0 | 22 | 0 | 0 | 1.363ms | 3.026ms | 63.788ms | 63.788ms | 63.788ms | 17 | 0 | 0 | 0 | bef034365ca24581:Full/5,f377c334c12229f8 | 64 |
| 64.264355 | Full | 0 | 0 | 9 | 0 | 26 | 0 | 19 | 0 | 0 | 0.289ms | 1.877ms | 61.288ms | 61.288ms | 61.288ms | 17 | 0 | 0 | 0 | bef034365ca24581:Full/5,f377c334c12229f8 | 93 |
| 62.155404999999995 | Full | 0 | 0 | 5 | 0 | 26 | 0 | 12 | 0 | 0 | 0.356ms | 1.020ms | 0.274ms | 0.274ms | 0.274ms | 9 | 0 | 0 | 0 | bef034365ca24581:Full/5,f377c334c12229f8 | 73 |
| 63.425841999999996 | Full | 0 | 0 | 4 | 0 | 26 | 0 | 13 | 0 | 0 | 0.316ms | 4.395ms | 24.049ms | 24.049ms | 24.049ms | 7 | 0 | 0 | 0 | bef034365ca24581:Full/5,f377c334c12229f8 | 63 |
| 66.92074500000001 | Full | 0 | 0 | 6 | 0 | 26 | 0 | 14 | 0 | 0 | 0.358ms | 1.758ms | 0.258ms | 0.258ms | 0.258ms | 9 | 0 | 0 | 0 | bef034365ca24581:Full/5,f377c334c12229f8 | 265 |
| 63.89586499999999 | Win_1 | 1 | 0 | 7 | 0 | 26 | 1 | 14 | 0 | 0 | 0.530ms | 34.356ms | 32.819ms | 32.819ms | 32.819ms | 11 | 0 | 1 | 0 | c2923e960ea7f21e:Win_1/933,bef034365ca24 | 63 |

**本块观测要点:**

- SF wall 范围 **62.16–68.80 ms**（冷=68.80）
- unfenced 逐 iter: [6, 9, 9, 5, 4, 6, 7]
- begin 洞数逐 iter: [29, 29, 26, 26, 26, 26, 26]
- pick_occ 逐 iter: [0, 0, 0, 0, 0, 0, 1]（暖机多为 0 ⇒ 整块不在 OCC pick）
- end_block **0.28–1.36 ms**（单时钟）
- 必要 vs 不必要：真 WAW/storage 脊必要；**lazy 长链 OrderedAdmit + 整块离开 OCC pick + 肥 end_block** 相对 OCC 为不必要壳。

### 2.x 块 19807137

#### 结构 / 目录

```json
{
  "ok": true,
  "dag": {
    "n_raw": 9,
    "n_waw": 628,
    "longest_chain": 571,
    "max_wave_width": 106,
    "max_writers_on_loc": 571,
    "independent_frac": 0.13342696629213482,
    "multi_writer_locs": 30
  },
  "kind_histogram": {
    "basic": 744,
    "basic_lazy": 13,
    "storage": 1006
  },
  "d1_kind_counts_in_top": {
    "storage": 4,
    "basic": 5,
    "basic_lazy": 3
  },
  "max_basic_writers": 21,
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
      "n_writers": 21,
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
    }
  ]
}
```

<details><summary>catalog 摘录</summary>

```json
{
  "morph": "WAW_spine",
  "selected": false,
  "L": 571,
  "W": 106,
  "n_raw": 9,
  "n_waw": 628,
  "bound_at_8": 1.246935,
  "max_writers_on_loc": 571
}
```

</details>

#### summary 字段

| key | value |
|-----|-------|
| `block` | `19807137` |
| `n_tx` | `712` |
| `soft` | `0` |
| `profile_on` | `False` |
| `primary_is` | `Instant-off reuse median` |
| `occ_median_ms` | `17.590266` |
| `sf_cold_ms` | `50.222322` |
| `sf_reuse_median_ms` | `65.515349` |
| `sf_all_median_ms` | `63.980618` |
| `sf_le_occ` | `False` |
| `last_arm` | `Win_1` |
| `last_w_need` | `0` |
| `last_unfenced` | `236` |
| `last_double_pay` | `0` |
| `last_begin_n` | `38` |

#### OCC 逐 iter

| wall_ms | occ_aborts | soft_wait_arms | begin_blocked | begin_blocked_n | chosen_strategy | chosen_w_need | chosen_win_w | commute_skip | covering_n | double_pay_n | edge_ordered_admit | end_block_ns | gate_stall_ns | i |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 2277.9864540000003 | 1256 | 0 | [] | 0 |  | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| 14.045891 | 860 | 0 | [] | 0 |  | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 1 |
| 18.667365 | 929 | 0 | [] | 0 |  | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 2 |
| 16.631139 | 746 | 0 | [] | 0 |  | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 3 |
| 14.244288000000001 | 972 | 0 | [] | 0 |  | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 4 |
| 19.469582 | 1093 | 0 | [] | 0 |  | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 5 |
| 17.590266 | 1030 | 0 | [] | 0 |  | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 6 |

#### SpecFence 逐 iter（Instant-off）

| wall_ms | chosen_strategy | chosen_win_w | chosen_w_need | unfenced_reexec | double_pay_n | begin_blocked_n | pick_occ_n | refuse_admit | wait_for_dependency | commute_skip | end_block_ns | reexec_ns | prepaid_ns | refuse_ns | gate_stall_ns | incarnation_gt0 | soft_wait_arms | covering_n | sys_reexec_n | selected_arms | edge_ordered_admit |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 50.222322 | Opt | 0 | 0 | 575 | 0 | 38 | 0 | 51 | 0 | 0 | 1.453ms | 105.395ms | 3.696ms | 3.696ms | 3.696ms | 586 | 0 | 0 | 0 |  | 81 |
| 68.897004 | Full | 0 | 0 | 136 | 0 | 38 | 2 | 214 | 3 | 0 | 1.726ms | 180.433ms | 27.053ms | 27.053ms | 27.053ms | 582 | 0 | 0 | 0 | bef034365ca24581:Full/20,1d7b2457a1cdcd2 | 1542 |
| 61.01186 | Full | 0 | 0 | 183 | 0 | 38 | 0 | 292 | 5 | 0 | 0.615ms | 163.555ms | 9.519ms | 9.519ms | 9.519ms | 571 | 0 | 0 | 0 | bef034365ca24581:Full/20,1d7b2457a1cdcd2 | 1261 |
| 65.515349 | Full | 0 | 0 | 116 | 0 | 38 | 1 | 198 | 7 | 0 | 0.541ms | 163.631ms | 26.661ms | 26.661ms | 26.661ms | 576 | 0 | 0 | 0 | bef034365ca24581:Full/20,1d7b2457a1cdcd2 | 1743 |
| 56.868254 | Full | 0 | 0 | 164 | 0 | 38 | 0 | 220 | 3 | 0 | 0.504ms | 137.809ms | 16.989ms | 16.989ms | 16.989ms | 570 | 0 | 0 | 0 | bef034365ca24581:Full/20,1bbf59fc950a02a | 1489 |
| 63.980618 | Win_1 | 1 | 0 | 79 | 0 | 38 | 3 | 243 | 2 | 0 | 0.529ms | 150.850ms | 21.824ms | 21.824ms | 21.824ms | 586 | 0 | 1 | 0 | bef034365ca24581:Win_1/20,1bbf59fc950a02 | 1689 |
| 87.331911 | Win_1 | 1 | 0 | 236 | 0 | 38 | 0 | 226 | 6 | 0 | 1.705ms | 197.151ms | 4.344ms | 4.344ms | 4.344ms | 579 | 0 | 1 | 0 | bef034365ca24581:Win_1/20,1bbf59fc950a02 | 1434 |

**本块观测要点:**

- SF wall 范围 **50.22–87.33 ms**（冷=50.22）
- unfenced 逐 iter: [575, 136, 183, 116, 164, 79, 236]
- begin 洞数逐 iter: [38, 38, 38, 38, 38, 38, 38]
- pick_occ 逐 iter: [0, 2, 0, 1, 0, 3, 0]（暖机多为 0 ⇒ 整块不在 OCC pick）
- end_block **0.50–1.73 ms**（单时钟）
- 必要 vs 不必要：真 WAW/storage 脊必要；**lazy 长链 OrderedAdmit + 整块离开 OCC pick + 肥 end_block** 相对 OCC 为不必要壳。

### 2.x 块 17666333

#### 结构 / 目录

```json
{
  "ok": true,
  "dag": {
    "n_raw": 18,
    "n_waw": 122,
    "longest_chain": 32,
    "max_wave_width": 897,
    "max_writers_on_loc": 31,
    "independent_frac": 0.9261186264308012,
    "multi_writer_locs": 55
  },
  "kind_histogram": {
    "storage": 359,
    "basic_lazy": 829,
    "basic": 194
  },
  "d1_kind_counts_in_top": {
    "basic": 1,
    "basic_lazy": 2,
    "storage": 9
  },
  "max_basic_writers": 450,
  "max_storage_writers": 18,
  "d1_top_spines": [
    {
      "loc": 7014270218453032428,
      "kind": "basic_lazy",
      "n_writers": 450,
      "writers_head": [
        137,
        138,
        139,
        140,
        141,
        142,
        143,
        144,
        145,
        146,
        147,
        148
      ],
      "writers_tail": [
        944,
        949,
        951,
        952,
        953,
        959
      ]
    },
    {
      "loc": 5769419880838541918,
      "kind": "basic_lazy",
      "n_writers": 374,
      "writers_head": [
        313,
        314,
        315,
        317,
        327,
        330,
        331,
        332,
        345,
        346,
        347,
        349
      ],
      "writers_tail": [
        954,
        955,
        956,
        957,
        958,
        960
      ]
    },
    {
      "loc": 13758554269703882113,
      "kind": "basic",
      "n_writers": 31,
      "writers_head": [
        1,
        2,
        6,
        8,
        10,
        12,
        13,
        14,
        17,
        20,
        22,
        23
      ],
      "writers_tail": [
        96,
        97,
        127,
        128,
        133,
        136
      ]
    },
    {
      "loc": 14128746953556440765,
      "kind": "storage",
      "n_writers": 18,
      "writers_head": [
        67,
        68,
        69,
        71,
        72,
        75,
        78,
        81,
        82,
        83,
        85,
        86
      ],
      "writers_tail": [
        87,
        89,
        90,
        91,
        92,
        94
      ]
    },
    {
      "loc": 3016948819509370870,
      "kind": "storage",
      "n_writers": 4,
      "writers_head": [
        6,
        8,
        10,
        57
      ],
      "writers_tail": []
    },
    {
      "loc": 3794368620910201186,
      "kind": "storage",
      "n_writers": 4,
      "writers_head": [
        6,
        8,
        10,
        57
      ],
      "writers_tail": []
    }
  ]
}
```

<details><summary>catalog 摘录</summary>

```json
{
  "morph": "mixed_RAW_WAW",
  "selected": true,
  "L": 32,
  "W": 897,
  "n_raw": 18,
  "n_waw": 122,
  "bound_at_8": 8.0,
  "max_writers_on_loc": 31
}
```

</details>

#### summary 字段

| key | value |
|-----|-------|
| `block` | `17666333` |
| `n_tx` | `961` |
| `soft` | `0` |
| `profile_on` | `False` |
| `primary_is` | `Instant-off reuse median` |
| `occ_median_ms` | `7.374468` |
| `sf_cold_ms` | `33.835452000000004` |
| `sf_reuse_median_ms` | `35.64165` |
| `sf_all_median_ms` | `34.589672` |
| `sf_le_occ` | `False` |
| `last_arm` | `Win_1` |
| `last_w_need` | `0` |
| `last_unfenced` | `13` |
| `last_double_pay` | `0` |
| `last_begin_n` | `36` |

#### OCC 逐 iter

| wall_ms | occ_aborts | soft_wait_arms | begin_blocked | begin_blocked_n | chosen_strategy | chosen_w_need | chosen_win_w | commute_skip | covering_n | double_pay_n | edge_ordered_admit | end_block_ns | gate_stall_ns | i |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 6.679824 | 45 | 0 | [] | 0 |  | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| 7.374468 | 80 | 0 | [] | 0 |  | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 1 |
| 7.357557 | 58 | 0 | [] | 0 |  | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 2 |
| 7.728032 | 62 | 0 | [] | 0 |  | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 3 |
| 6.896494000000001 | 48 | 0 | [] | 0 |  | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 4 |
| 8.155366 | 69 | 0 | [] | 0 |  | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 5 |
| 8.400731 | 77 | 0 | [] | 0 |  | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 6 |

#### SpecFence 逐 iter（Instant-off）

| wall_ms | chosen_strategy | chosen_win_w | chosen_w_need | unfenced_reexec | double_pay_n | begin_blocked_n | pick_occ_n | refuse_admit | wait_for_dependency | commute_skip | end_block_ns | reexec_ns | prepaid_ns | refuse_ns | gate_stall_ns | incarnation_gt0 | soft_wait_arms | covering_n | sys_reexec_n | selected_arms | edge_ordered_admit |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 33.835452000000004 | Opt | 0 | 0 | 33 | 0 | 36 | 0 | 58 | 0 | 0 | 1.724ms | 13.757ms | 2.583ms | 2.583ms | 2.583ms | 55 | 0 | 0 | 0 |  | 127 |
| 47.330518000000005 | Opt | 0 | 0 | 7 | 0 | 36 | 0 | 48 | 20 | 0 | 1.976ms | 93.560ms | 8.563ms | 8.563ms | 8.563ms | 43 | 0 | 0 | 0 |  | 389 |
| 34.589672 | Opt | 0 | 0 | 24 | 0 | 36 | 0 | 35 | 14 | 0 | 1.875ms | 22.897ms | 4.398ms | 4.398ms | 4.398ms | 52 | 0 | 0 | 0 |  | 140 |
| 34.349815 | Opt | 0 | 0 | 9 | 0 | 36 | 0 | 53 | 9 | 0 | 1.910ms | 24.959ms | 2.644ms | 2.644ms | 2.644ms | 40 | 0 | 0 | 0 |  | 151 |
| 35.64165 | Full | 0 | 0 | 7 | 0 | 36 | 0 | 69 | 9 | 0 | 1.926ms | 18.830ms | 6.762ms | 6.762ms | 6.762ms | 36 | 0 | 0 | 0 | bef034365ca24581:Full/30 | 146 |
| 36.75345 | Win_1 | 1 | 0 | 18 | 0 | 36 | 0 | 76 | 9 | 0 | 0.394ms | 21.937ms | 32.093ms | 32.093ms | 32.093ms | 47 | 0 | 1 | 0 | bef034365ca24581:Win_1/30 | 147 |
| 33.894522 | Win_1 | 1 | 0 | 13 | 0 | 36 | 0 | 44 | 4 | 0 | 0.362ms | 24.661ms | 9.012ms | 9.012ms | 9.012ms | 38 | 0 | 1 | 0 | bef034365ca24581:Win_1/30 | 132 |

**本块观测要点:**

- SF wall 范围 **33.84–47.33 ms**（冷=33.84）
- unfenced 逐 iter: [33, 7, 24, 9, 7, 18, 13]
- begin 洞数逐 iter: [36, 36, 36, 36, 36, 36, 36]
- pick_occ 逐 iter: [0, 0, 0, 0, 0, 0, 0]（暖机多为 0 ⇒ 整块不在 OCC pick）
- end_block **0.36–1.98 ms**（单时钟）
- 必要 vs 不必要：真 WAW/storage 脊必要；**lazy 长链 OrderedAdmit + 整块离开 OCC pick + 肥 end_block** 相对 OCC 为不必要壳。

### 2.x 块 15538827

#### 结构 / 目录

```json
{
  "ok": true,
  "dag": {
    "n_raw": 62,
    "n_waw": 146,
    "longest_chain": 35,
    "max_wave_width": 697,
    "max_writers_on_loc": 35,
    "independent_frac": 0.8213851761846902,
    "multi_writer_locs": 46
  },
  "kind_histogram": {
    "basic_lazy": 568,
    "unknown": 2,
    "code_hash": 1,
    "storage": 539,
    "basic": 321
  },
  "d1_kind_counts_in_top": {
    "basic_lazy": 4,
    "storage": 4,
    "basic": 4
  },
  "max_basic_writers": 533,
  "max_storage_writers": 35,
  "d1_top_spines": [
    {
      "loc": 5083017834245034865,
      "kind": "basic_lazy",
      "n_writers": 533,
      "writers_head": [
        17,
        18,
        19,
        20,
        21,
        22,
        23,
        24,
        25,
        26,
        27,
        28
      ],
      "writers_tail": [
        544,
        545,
        546,
        547,
        548,
        549
      ]
    },
    {
      "loc": 8987249224064866732,
      "kind": "storage",
      "n_writers": 35,
      "writers_head": [
        648,
        659,
        667,
        738,
        742,
        745,
        746,
        749,
        751,
        758,
        759,
        761
      ],
      "writers_tail": [
        812,
        813,
        816,
        818,
        819,
        820
      ]
    },
    {
      "loc": 13758554269703882113,
      "kind": "basic",
      "n_writers": 16,
      "writers_head": [
        591,
        603,
        610,
        668,
        686,
        687,
        695,
        703,
        705,
        707,
        708,
        718
      ],
      "writers_tail": [
        708,
        718,
        731,
        752,
        770,
        811
      ]
    },
    {
      "loc": 416552959218310521,
      "kind": "storage",
      "n_writers": 15,
      "writers_head": [
        554,
        555,
        556,
        557,
        558,
        559,
        560,
        561,
        562,
        563,
        564,
        565
      ],
      "writers_tail": [
        563,
        564,
        565,
        566,
        567,
        568
      ]
    },
    {
      "loc": 7613939776377353113,
      "kind": "basic",
      "n_writers": 15,
      "writers_head": [
        554,
        555,
        556,
        557,
        558,
        559,
        560,
        561,
        562,
        563,
        564,
        565
      ],
      "writers_tail": [
        563,
        564,
        565,
        566,
        567,
        568
      ]
    },
    {
      "loc": 348090351077845393,
      "kind": "basic_lazy",
      "n_writers": 8,
      "writers_head": [
        604,
        605,
        606,
        607,
        608,
        609,
        612,
        730
      ],
      "writers_tail": []
    }
  ]
}
```

<details><summary>catalog 摘录</summary>

```json
{
  "morph": "mixed_RAW_WAW",
  "selected": true,
  "L": 35,
  "W": 696,
  "n_raw": 62,
  "n_waw": 147,
  "bound_at_8": 8.0,
  "max_writers_on_loc": 35
}
```

</details>

#### summary 字段

| key | value |
|-----|-------|
| `block` | `15538827` |
| `n_tx` | `823` |
| `soft` | `0` |
| `profile_on` | `False` |
| `primary_is` | `Instant-off reuse median` |
| `occ_median_ms` | `5.6749540000000005` |
| `sf_cold_ms` | `29.549725` |
| `sf_reuse_median_ms` | `29.414885` |
| `sf_all_median_ms` | `29.414885` |
| `sf_le_occ` | `False` |
| `last_arm` | `Win_1` |
| `last_w_need` | `0` |
| `last_unfenced` | `21` |
| `last_double_pay` | `0` |
| `last_begin_n` | `85` |

#### OCC 逐 iter

| wall_ms | occ_aborts | soft_wait_arms | begin_blocked | begin_blocked_n | chosen_strategy | chosen_w_need | chosen_win_w | commute_skip | covering_n | double_pay_n | edge_ordered_admit | end_block_ns | gate_stall_ns | i |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 6.071554 | 70 | 0 | [] | 0 |  | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| 5.289027 | 50 | 0 | [] | 0 |  | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 1 |
| 5.367782 | 67 | 0 | [] | 0 |  | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 2 |
| 6.1490149999999995 | 74 | 0 | [] | 0 |  | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 3 |
| 5.965936 | 71 | 0 | [] | 0 |  | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 4 |
| 5.22008 | 74 | 0 | [] | 0 |  | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 5 |
| 5.6749540000000005 | 68 | 0 | [] | 0 |  | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 6 |

#### SpecFence 逐 iter（Instant-off）

| wall_ms | chosen_strategy | chosen_win_w | chosen_w_need | unfenced_reexec | double_pay_n | begin_blocked_n | pick_occ_n | refuse_admit | wait_for_dependency | commute_skip | end_block_ns | reexec_ns | prepaid_ns | refuse_ns | gate_stall_ns | incarnation_gt0 | soft_wait_arms | covering_n | sys_reexec_n | selected_arms | edge_ordered_admit |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 29.549725 | Opt | 0 | 0 | 25 | 0 | 85 | 0 | 38 | 0 | 0 | 2.400ms | 9.418ms | 20.096ms | 20.096ms | 20.096ms | 52 | 0 | 0 | 0 |  | 175 |
| 29.414885 | Opt | 0 | 0 | 10 | 0 | 79 | 0 | 56 | 6 | 0 | 2.506ms | 7.827ms | 19.166ms | 19.166ms | 19.166ms | 33 | 0 | 0 | 0 |  | 165 |
| 27.524915 | Opt | 0 | 0 | 20 | 0 | 79 | 0 | 104 | 0 | 0 | 0.314ms | 7.844ms | 12.399ms | 12.399ms | 12.399ms | 49 | 0 | 0 | 0 |  | 151 |
| 32.545576 | Full | 0 | 0 | 23 | 0 | 79 | 0 | 73 | 5 | 0 | 2.803ms | 9.252ms | 1.949ms | 1.949ms | 1.949ms | 53 | 0 | 0 | 0 | 3b7b96f355b7221b:Full/1 | 166 |
| 27.762407 | Full | 0 | 0 | 33 | 0 | 85 | 0 | 63 | 1 | 0 | 0.328ms | 12.228ms | 5.676ms | 5.676ms | 5.676ms | 63 | 0 | 0 | 0 | 3b7b96f355b7221b:Full/1 | 171 |
| 27.637795 | Win_1 | 1 | 0 | 13 | 0 | 85 | 0 | 59 | 2 | 0 | 0.307ms | 6.187ms | 2.228ms | 2.228ms | 2.228ms | 45 | 0 | 1 | 0 | d2e42ea45af851ca:Win_1/3,3b7b96f355b7221 | 176 |
| 30.966179 | Win_1 | 1 | 0 | 21 | 0 | 85 | 0 | 61 | 1 | 0 | 3.066ms | 7.386ms | 1.182ms | 1.182ms | 1.182ms | 54 | 0 | 1 | 0 | d2e42ea45af851ca:Win_1/3,f76367e953e72f2 | 182 |

**本块观测要点:**

- SF wall 范围 **27.52–32.55 ms**（冷=29.55）
- unfenced 逐 iter: [25, 10, 20, 23, 33, 13, 21]
- begin 洞数逐 iter: [85, 79, 79, 79, 85, 85, 85]
- pick_occ 逐 iter: [0, 0, 0, 0, 0, 0, 0]（暖机多为 0 ⇒ 整块不在 OCC pick）
- end_block **0.31–3.07 ms**（单时钟）
- 必要 vs 不必要：真 WAW/storage 脊必要；**lazy 长链 OrderedAdmit + 整块离开 OCC pick + 肥 end_block** 相对 OCC 为不必要壳。

### 2.x 块 14334629

#### 结构 / 目录

```json
{
  "ok": true,
  "dag": {
    "n_raw": 36,
    "n_waw": 138,
    "longest_chain": 28,
    "max_wave_width": 735,
    "max_writers_on_loc": 26,
    "independent_frac": 0.8717948717948718,
    "multi_writer_locs": 79
  },
  "kind_histogram": {
    "storage": 578,
    "basic": 302,
    "basic_lazy": 577
  },
  "d1_kind_counts_in_top": {
    "storage": 2,
    "basic": 3,
    "basic_lazy": 7
  },
  "max_basic_writers": 486,
  "max_storage_writers": 7,
  "d1_top_spines": [
    {
      "loc": 14562814392964356195,
      "kind": "basic_lazy",
      "n_writers": 486,
      "writers_head": [
        293,
        294,
        295,
        296,
        297,
        298,
        299,
        300,
        301,
        302,
        303,
        304
      ],
      "writers_tail": [
        813,
        814,
        815,
        816,
        817,
        818
      ]
    },
    {
      "loc": 13758554269703882113,
      "kind": "basic",
      "n_writers": 27,
      "writers_head": [
        2,
        4,
        7,
        14,
        21,
        25,
        44,
        54,
        56,
        97,
        124,
        147
      ],
      "writers_tail": [
        340,
        426,
        501,
        519,
        596,
        604
      ]
    },
    {
      "loc": 10890103240121818830,
      "kind": "basic_lazy",
      "n_writers": 16,
      "writers_head": [
        99,
        105,
        218,
        238,
        245,
        252,
        261,
        278,
        284,
        289,
        291,
        468
      ],
      "writers_tail": [
        291,
        468,
        473,
        481,
        657,
        665
      ]
    },
    {
      "loc": 238884258749349807,
      "kind": "basic_lazy",
      "n_writers": 14,
      "writers_head": [
        98,
        101,
        106,
        108,
        227,
        234,
        239,
        247,
        251,
        253,
        275,
        277
      ],
      "writers_tail": [
        251,
        253,
        275,
        277,
        290,
        636
      ]
    },
    {
      "loc": 4374904034207278638,
      "kind": "basic_lazy",
      "n_writers": 13,
      "writers_head": [
        100,
        104,
        107,
        109,
        228,
        231,
        282,
        285,
        444,
        539,
        620,
        707
      ],
      "writers_tail": [
        285,
        444,
        539,
        620,
        707,
        711
      ]
    },
    {
      "loc": 13428981586540593318,
      "kind": "basic_lazy",
      "n_writers": 13,
      "writers_head": [
        169,
        170,
        171,
        172,
        173,
        174,
        175,
        176,
        177,
        178,
        179,
        180
      ],
      "writers_tail": [
        176,
        177,
        178,
        179,
        180,
        181
      ]
    }
  ]
}
```

<details><summary>catalog 摘录</summary>

```json
{
  "morph": "mixed_RAW_WAW",
  "selected": true,
  "L": 28,
  "W": 734,
  "n_raw": 36,
  "n_waw": 139,
  "bound_at_8": 8.0,
  "max_writers_on_loc": 26
}
```

</details>

#### summary 字段

| key | value |
|-----|-------|
| `block` | `14334629` |
| `n_tx` | `819` |
| `soft` | `0` |
| `profile_on` | `False` |
| `primary_is` | `Instant-off reuse median` |
| `occ_median_ms` | `6.24598` |
| `sf_cold_ms` | `27.787103000000002` |
| `sf_reuse_median_ms` | `27.691457999999997` |
| `sf_all_median_ms` | `27.691457999999997` |
| `sf_le_occ` | `False` |
| `last_arm` | `Full` |
| `last_w_need` | `0` |
| `last_unfenced` | `14` |
| `last_double_pay` | `0` |
| `last_begin_n` | `63` |

#### OCC 逐 iter

| wall_ms | occ_aborts | soft_wait_arms | begin_blocked | begin_blocked_n | chosen_strategy | chosen_w_need | chosen_win_w | commute_skip | covering_n | double_pay_n | edge_ordered_admit | end_block_ns | gate_stall_ns | i |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 5.7465090000000005 | 29 | 0 | [] | 0 |  | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| 6.109958000000001 | 41 | 0 | [] | 0 |  | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 1 |
| 6.425611 | 34 | 0 | [] | 0 |  | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 2 |
| 6.418183 | 45 | 0 | [] | 0 |  | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 3 |
| 6.24598 | 49 | 0 | [] | 0 |  | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 4 |
| 5.635243 | 38 | 0 | [] | 0 |  | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 5 |
| 6.424222 | 54 | 0 | [] | 0 |  | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 6 |

#### SpecFence 逐 iter（Instant-off）

| wall_ms | chosen_strategy | chosen_win_w | chosen_w_need | unfenced_reexec | double_pay_n | begin_blocked_n | pick_occ_n | refuse_admit | wait_for_dependency | commute_skip | end_block_ns | reexec_ns | prepaid_ns | refuse_ns | gate_stall_ns | incarnation_gt0 | soft_wait_arms | covering_n | sys_reexec_n | selected_arms | edge_ordered_admit |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 27.787103000000002 | Opt | 0 | 0 | 17 | 0 | 63 | 0 | 33 | 0 | 0 | 2.501ms | 10.301ms | 15.737ms | 15.737ms | 15.737ms | 46 | 0 | 0 | 0 |  | 166 |
| 29.450955 | Opt | 0 | 0 | 17 | 0 | 63 | 0 | 36 | 7 | 0 | 2.689ms | 16.897ms | 10.787ms | 10.787ms | 10.787ms | 49 | 0 | 0 | 0 |  | 164 |
| 26.547269 | Opt | 0 | 0 | 11 | 0 | 63 | 0 | 15 | 6 | 0 | 2.604ms | 10.209ms | 0.365ms | 0.365ms | 0.365ms | 39 | 0 | 0 | 0 |  | 174 |
| 29.530741 | Full | 0 | 0 | 15 | 0 | 63 | 0 | 39 | 12 | 0 | 3.056ms | 19.982ms | 1.388ms | 1.388ms | 1.388ms | 56 | 0 | 0 | 0 | bef034365ca24581:Full/26 | 176 |
| 25.472208 | Full | 0 | 0 | 11 | 0 | 63 | 0 | 34 | 13 | 0 | 0.379ms | 21.014ms | 3.916ms | 3.916ms | 3.916ms | 43 | 0 | 0 | 0 | bef034365ca24581:Full/26 | 171 |
| 27.691457999999997 | Full | 0 | 0 | 17 | 0 | 63 | 0 | 23 | 7 | 0 | 2.715ms | 35.378ms | 2.543ms | 2.543ms | 2.543ms | 54 | 0 | 0 | 0 | bef034365ca24581:Full/27,cc95e142ee736b6 | 175 |
| 25.529832 | Full | 0 | 0 | 14 | 0 | 63 | 0 | 23 | 3 | 0 | 0.353ms | 11.015ms | 3.289ms | 3.289ms | 3.289ms | 46 | 0 | 0 | 0 | bef034365ca24581:Full/27,cc95e142ee736b6 | 159 |

**本块观测要点:**

- SF wall 范围 **25.47–29.53 ms**（冷=27.79）
- unfenced 逐 iter: [17, 17, 11, 15, 11, 17, 14]
- begin 洞数逐 iter: [63, 63, 63, 63, 63, 63, 63]
- pick_occ 逐 iter: [0, 0, 0, 0, 0, 0, 0]（暖机多为 0 ⇒ 整块不在 OCC pick）
- end_block **0.35–3.06 ms**（单时钟）
- 必要 vs 不必要：真 WAW/storage 脊必要；**lazy 长链 OrderedAdmit + 整块离开 OCC pick + 肥 end_block** 相对 OCC 为不必要壳。

### 2.x 块 15199017

#### 结构 / 目录

```json
{
  "ok": true,
  "dag": {
    "n_raw": 12,
    "n_waw": 42,
    "longest_chain": 7,
    "max_wave_width": 831,
    "max_writers_on_loc": 7,
    "independent_frac": 0.941108545034642,
    "multi_writer_locs": 25
  },
  "kind_histogram": {
    "basic_lazy": 713,
    "code_hash": 1,
    "storage": 479,
    "basic": 200
  },
  "d1_kind_counts_in_top": {
    "basic_lazy": 9,
    "basic": 1,
    "storage": 2
  },
  "max_basic_writers": 459,
  "max_storage_writers": 4,
  "d1_top_spines": [
    {
      "loc": 6509387228596508080,
      "kind": "basic_lazy",
      "n_writers": 459,
      "writers_head": [
        134,
        135,
        136,
        137,
        140,
        141,
        142,
        143,
        144,
        145,
        146,
        147
      ],
      "writers_tail": [
        856,
        857,
        859,
        861,
        862,
        863
      ]
    },
    {
      "loc": 11181477507518635332,
      "kind": "basic_lazy",
      "n_writers": 169,
      "writers_head": [
        138,
        149,
        150,
        156,
        159,
        165,
        168,
        179,
        187,
        196,
        200,
        203
      ],
      "writers_tail": [
        849,
        852,
        855,
        858,
        860,
        864
      ]
    },
    {
      "loc": 12960885164707752850,
      "kind": "basic_lazy",
      "n_writers": 16,
      "writers_head": [
        43,
        44,
        50,
        51,
        52,
        53,
        54,
        55,
        56,
        57,
        58,
        59
      ],
      "writers_tail": [
        58,
        59,
        60,
        61,
        62,
        63
      ]
    },
    {
      "loc": 14476036925195257868,
      "kind": "basic_lazy",
      "n_writers": 16,
      "writers_head": [
        76,
        77,
        78,
        79,
        80,
        82,
        83,
        88,
        89,
        90,
        94,
        95
      ],
      "writers_tail": [
        94,
        95,
        96,
        97,
        98,
        99
      ]
    },
    {
      "loc": 17951153645096425929,
      "kind": "basic_lazy",
      "n_writers": 11,
      "writers_head": [
        212,
        267,
        348,
        467,
        557,
        596,
        649,
        652,
        708,
        731,
        780
      ],
      "writers_tail": []
    },
    {
      "loc": 18023714613962337131,
      "kind": "basic_lazy",
      "n_writers": 9,
      "writers_head": [
        211,
        220,
        268,
        285,
        328,
        402,
        476,
        564,
        816
      ],
      "writers_tail": []
    }
  ]
}
```

<details><summary>catalog 摘录</summary>

```json
{
  "morph": "mixed_RAW_WAW",
  "selected": true,
  "L": 7,
  "W": 831,
  "n_raw": 12,
  "n_waw": 42,
  "bound_at_8": 8.0,
  "max_writers_on_loc": 7
}
```

</details>

#### summary 字段

| key | value |
|-----|-------|
| `block` | `15199017` |
| `n_tx` | `866` |
| `soft` | `0` |
| `profile_on` | `False` |
| `primary_is` | `Instant-off reuse median` |
| `occ_median_ms` | `4.276062` |
| `sf_cold_ms` | `20.492467` |
| `sf_reuse_median_ms` | `21.867901` |
| `sf_all_median_ms` | `21.834316` |
| `sf_le_occ` | `False` |
| `last_arm` | `Win_1` |
| `last_w_need` | `1` |
| `last_unfenced` | `3` |
| `last_double_pay` | `0` |
| `last_begin_n` | `44` |

#### OCC 逐 iter

| wall_ms | occ_aborts | soft_wait_arms | begin_blocked | begin_blocked_n | chosen_strategy | chosen_w_need | chosen_win_w | commute_skip | covering_n | double_pay_n | edge_ordered_admit | end_block_ns | gate_stall_ns | i |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 4.146325999999999 | 4 | 0 | [] | 0 |  | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| 4.276062 | 8 | 0 | [] | 0 |  | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 1 |
| 4.320081 | 9 | 0 | [] | 0 |  | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 2 |
| 4.214688000000001 | 8 | 0 | [] | 0 |  | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 3 |
| 4.260797 | 6 | 0 | [] | 0 |  | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 4 |
| 4.47817 | 13 | 0 | [] | 0 |  | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 5 |
| 4.295222 | 5 | 0 | [] | 0 |  | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 6 |

#### SpecFence 逐 iter（Instant-off）

| wall_ms | chosen_strategy | chosen_win_w | chosen_w_need | unfenced_reexec | double_pay_n | begin_blocked_n | pick_occ_n | refuse_admit | wait_for_dependency | commute_skip | end_block_ns | reexec_ns | prepaid_ns | refuse_ns | gate_stall_ns | incarnation_gt0 | soft_wait_arms | covering_n | sys_reexec_n | selected_arms | edge_ordered_admit |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 20.492467 | Win_1 | 1 | 0 | 6 | 0 | 44 | 0 | 35 | 0 | 0 | 0.308ms | 10.241ms | 0.500ms | 0.500ms | 0.500ms | 11 | 0 | 1 | 0 | 5a55fd3239f595b0:Win_1/458 | 92 |
| 21.867901 | Win_1 | 1 | 0 | 5 | 0 | 44 | 0 | 49 | 0 | 0 | 0.368ms | 1.675ms | 1.045ms | 1.045ms | 1.045ms | 9 | 0 | 1 | 0 | 5a55fd3239f595b0:Win_1/458,d760e4ddc1ebd | 91 |
| 21.967733 | Win_1 | 1 | 0 | 4 | 0 | 44 | 0 | 35 | 0 | 0 | 0.348ms | 1.526ms | 0.869ms | 0.869ms | 0.869ms | 8 | 0 | 1 | 0 | 5a55fd3239f595b0:Win_1/458,d760e4ddc1ebd | 92 |
| 21.834316 | Win_1 | 1 | 0 | 5 | 0 | 44 | 0 | 38 | 0 | 0 | 0.348ms | 6.098ms | 0.963ms | 0.963ms | 0.963ms | 8 | 0 | 1 | 0 | 5a55fd3239f595b0:Win_1/458,d760e4ddc1ebd | 90 |
| 21.595996 | Win_1 | 1 | 0 | 5 | 0 | 44 | 0 | 22 | 0 | 0 | 0.350ms | 8.764ms | 10.231ms | 10.231ms | 10.231ms | 8 | 0 | 1 | 0 | 5a55fd3239f595b0:Win_1/458,d760e4ddc1ebd | 90 |
| 21.342158 | Win_1 | 1 | 0 | 5 | 0 | 44 | 0 | 25 | 0 | 0 | 0.399ms | 1.192ms | 0.998ms | 0.998ms | 0.998ms | 9 | 0 | 1 | 0 | 5a55fd3239f595b0:Win_1/458,b9e348049554a | 91 |
| 22.567444 | Win_1 | 1 | 1 | 3 | 0 | 44 | 0 | 39 | 1 | 0 | 0.355ms | 2.683ms | 1.431ms | 1.431ms | 1.431ms | 11 | 0 | 1 | 0 | 5a55fd3239f595b0:Win_1/458,b9e348049554a | 95 |

**本块观测要点:**

- SF wall 范围 **20.49–22.57 ms**（冷=20.49）
- unfenced 逐 iter: [6, 5, 4, 5, 5, 5, 3]
- begin 洞数逐 iter: [44, 44, 44, 44, 44, 44, 44]
- pick_occ 逐 iter: [0, 0, 0, 0, 0, 0, 0]（暖机多为 0 ⇒ 整块不在 OCC pick）
- end_block **0.31–0.40 ms**（单时钟）
- 必要 vs 不必要：真 WAW/storage 脊必要；**lazy 长链 OrderedAdmit + 整块离开 OCC pick + 肥 end_block** 相对 OCC 为不必要壳。

---

## 3. PROFILE Instant-tax（不可加进墙）

`SPECFENCE_PROFILE=1` 会改学习；下列只比较「SF 是否系统性重于 OCC」的 worker 求和。

### PROFILE 块 14396881

| key | value |
|-----|-------|
| `block` | `14396881` |
| `n_tx` | `1346` |
| `soft` | `0` |
| `profile_on` | `True` |
| `primary_is` | `Instant-tax reuse median (not wall PRIMARY)` |
| `occ_median_ms` | `4.5759360000000004` |
| `sf_cold_ms` | `106.52049600000001` |
| `sf_reuse_median_ms` | `112.47485999999999` |
| `sf_all_median_ms` | `111.51578099999999` |
| `sf_le_occ` | `False` |
| `last_arm` | `Win_1` |
| `last_w_need` | `1` |
| `last_unfenced` | `2` |
| `last_double_pay` | `0` |
| `last_begin_n` | `6` |

| handler_ns | i | idle_core_ns | learn | maybe_wait_ns | mode | scheduler_ns | validate_ns | wall_ms |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 2761312 | 0 | 0 |  | 0 | occ | 1949291 | 763622 | 5.743374 |
| 5276022 | 0 | 0 | Opt | 0 | specfence | 2170743 | 584023 | 106.52049600000001 |
| 2885095 | 1 | 0 |  | 0 | occ | 1818566 | 844275 | 4.788304 |
| 2409197 | 1 | 0 | Opt | 0 | specfence | 2212048 | 598788 | 111.51578099999999 |
| 2418529 | 2 | 0 |  | 0 | occ | 1776223 | 741909 | 4.400068 |
| 2549966 | 2 | 83138 | Opt | 0 | specfence | 2653073 | 609483 | 109.15370100000001 |
| 2826407 | 3 | 0 |  | 0 | occ | 2068402 | 848088 | 4.5759360000000004 |
| 2271621 | 3 | 248728 | Opt | 0 | specfence | 2278939 | 571054 | 109.768931 |
| 2615495 | 4 | 0 |  | 0 | occ | 2044659 | 749077 | 4.598543 |
| 2313285 | 4 | 665278 | Opt | 0 | specfence | 2790421 | 3370758 | 120.81783 |
| 2518327 | 5 | 0 |  | 0 | occ | 1449667 | 698198 | 4.221004000000001 |
| 2356069 | 5 | 1123805 | Opt | 0 | specfence | 11331673 | 4564677 | 112.47485999999999 |

> Instant-tax 规则: worker-sum Instant; do not add into wall; PROFILE can change learn

### PROFILE 块 15274915

| key | value |
|-----|-------|
| `block` | `15274915` |
| `n_tx` | `1226` |
| `soft` | `0` |
| `profile_on` | `True` |
| `primary_is` | `Instant-tax reuse median (not wall PRIMARY)` |
| `occ_median_ms` | `5.162276` |
| `sf_cold_ms` | `82.819821` |
| `sf_reuse_median_ms` | `79.11109900000001` |
| `sf_all_median_ms` | `79.11109900000001` |
| `sf_le_occ` | `False` |
| `last_arm` | `Full` |
| `last_w_need` | `0` |
| `last_unfenced` | `6` |
| `last_double_pay` | `0` |
| `last_begin_n` | `95` |

| handler_ns | i | idle_core_ns | learn | maybe_wait_ns | mode | scheduler_ns | validate_ns | wall_ms |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 6481421 | 0 | 0 |  | 0 | occ | 1903787 | 1143752 | 4.863456 |
| 8474418 | 0 | 0 | Opt | 0 | specfence | 2171003 | 766195 | 82.819821 |
| 5746400 | 1 | 0 |  | 0 | occ | 1789095 | 1975582 | 5.015842 |
| 6745595 | 1 | 7084109 | Opt | 0 | specfence | 19278404 | 3560544 | 79.11109900000001 |
| 5307276 | 2 | 0 |  | 0 | occ | 1934524 | 1346250 | 5.162276 |
| 5293369 | 2 | 1468648 | Opt | 0 | specfence | 17920537 | 1825078 | 77.540751 |
| 5805005 | 3 | 0 |  | 0 | occ | 2964593 | 3119951 | 5.590413 |
| 11456183 | 3 | 2033753 | Opt | 0 | specfence | 40686883 | 3160830 | 78.365493 |
| 5487467 | 4 | 0 |  | 0 | occ | 2092409 | 1598585 | 5.180753 |
| 5051274 | 4 | 2471296 | Opt | 0 | specfence | 9440364 | 6454834 | 77.862001 |
| 5687214 | 5 | 0 |  | 0 | occ | 2081121 | 1415641 | 5.0195300000000005 |
| 17316619 | 5 | 6457683 | Opt | 0 | specfence | 19248544 | 5696060 | 79.81692799999999 |

> Instant-tax 规则: worker-sum Instant; do not add into wall; PROFILE can change learn

### PROFILE 块 13217637

| key | value |
|-----|-------|
| `block` | `13217637` |
| `n_tx` | `1100` |
| `soft` | `0` |
| `profile_on` | `True` |
| `primary_is` | `Instant-tax reuse median (not wall PRIMARY)` |
| `occ_median_ms` | `4.97731` |
| `sf_cold_ms` | `73.199994` |
| `sf_reuse_median_ms` | `72.693267` |
| `sf_all_median_ms` | `72.693267` |
| `sf_le_occ` | `False` |
| `last_arm` | `Win_2` |
| `last_w_need` | `0` |
| `last_unfenced` | `6` |
| `last_double_pay` | `0` |
| `last_begin_n` | `26` |

| handler_ns | i | idle_core_ns | learn | maybe_wait_ns | mode | scheduler_ns | validate_ns | wall_ms |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 6403933 | 0 | 0 |  | 0 | occ | 2238228 | 1573466 | 4.995439 |
| 5767069 | 0 | 0 | Full | 0 | specfence | 1119517 | 11575258 | 73.199994 |
| 8444074 | 1 | 0 |  | 0 | occ | 5825678 | 1341037 | 7.041029 |
| 6358530 | 1 | 467493 | Win_1 | 0 | specfence | 4439897 | 897651 | 72.693267 |
| 6649915 | 2 | 0 |  | 0 | occ | 1357960 | 1011347 | 4.951982 |
| 36189857 | 2 | 576742 | Win_2 | 0 | specfence | 25030298 | 1401617 | 77.716774 |
| 6185092 | 3 | 0 |  | 0 | occ | 1420659 | 1104541 | 4.779879 |
| 6568980 | 3 | 205475 | Win_2 | 0 | specfence | 1485284 | 1272433 | 71.959152 |
| 5894553 | 4 | 0 |  | 0 | occ | 1819617 | 1102487 | 4.97731 |
| 12679053 | 4 | 103544 | Win_2 | 0 | specfence | 2495242 | 1128209 | 71.878843 |
| 6916963 | 5 | 0 |  | 0 | occ | 1712175 | 1672140 | 5.038849 |
| 13086219 | 5 | 719021 | Win_2 | 0 | specfence | 2194694 | 1001311 | 73.07079 |

> Instant-tax 规则: worker-sum Instant; do not add into wall; PROFILE can change learn

### PROFILE 块 19807137

| key | value |
|-----|-------|
| `block` | `19807137` |
| `n_tx` | `712` |
| `soft` | `0` |
| `profile_on` | `True` |
| `primary_is` | `Instant-tax reuse median (not wall PRIMARY)` |
| `occ_median_ms` | `16.7854` |
| `sf_cold_ms` | `61.94731` |
| `sf_reuse_median_ms` | `70.637501` |
| `sf_all_median_ms` | `68.14058` |
| `sf_le_occ` | `False` |
| `last_arm` | `Full` |
| `last_w_need` | `0` |
| `last_unfenced` | `192` |
| `last_double_pay` | `0` |
| `last_begin_n` | `38` |

| handler_ns | i | idle_core_ns | learn | maybe_wait_ns | mode | scheduler_ns | validate_ns | wall_ms |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 2318202060 | 0 | 0 |  | 0 | occ | 15917565096 | 13669507 | 2288.150126 |
| 41646349 | 0 | 36867585 | Full | 0 | specfence | 208616145 | 63206985 | 61.94731 |
| 36626034 | 1 | 0 |  | 0 | occ | 28085163 | 35332549 | 16.817805 |
| 108489746 | 1 | 36889052 | Full | 0 | specfence | 156560598 | 48866660 | 66.55575 |
| 36869092 | 2 | 0 |  | 0 | occ | 26499468 | 11369103 | 15.70175 |
| 113182060 | 2 | 31241653 | Full | 0 | specfence | 181859117 | 50203880 | 71.123696 |
| 35088494 | 3 | 0 |  | 0 | occ | 26544369 | 19993340 | 16.300783 |
| 129062385 | 3 | 28097111 | Win_1 | 0 | specfence | 184168253 | 56478875 | 70.637501 |
| 38505067 | 4 | 0 |  | 0 | occ | 31048678 | 18270318 | 16.7854 |
| 83830505 | 4 | 32844327 | Win_1 | 0 | specfence | 151670888 | 57892973 | 61.242354999999996 |
| 31014143 | 5 | 0 |  | 0 | occ | 24172808 | 21444485 | 14.581090999999999 |
| 100200374 | 5 | 28167286 | Full | 0 | specfence | 163000020 | 60520729 | 68.14058 |

> Instant-tax 规则: worker-sum Instant; do not add into wall; PROFILE can change learn

### PROFILE 块 17666333

| key | value |
|-----|-------|
| `block` | `17666333` |
| `n_tx` | `961` |
| `soft` | `0` |
| `profile_on` | `True` |
| `primary_is` | `Instant-tax reuse median (not wall PRIMARY)` |
| `occ_median_ms` | `8.05384` |
| `sf_cold_ms` | `34.811364` |
| `sf_reuse_median_ms` | `38.8632` |
| `sf_all_median_ms` | `38.709932` |
| `sf_le_occ` | `False` |
| `last_arm` | `Win_1` |
| `last_w_need` | `0` |
| `last_unfenced` | `11` |
| `last_double_pay` | `1` |
| `last_begin_n` | `36` |

| handler_ns | i | idle_core_ns | learn | maybe_wait_ns | mode | scheduler_ns | validate_ns | wall_ms |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 17472365 | 0 | 0 |  | 0 | occ | 3418183 | 2415651 | 7.529985 |
| 18603915 | 0 | 1082510 | Win_1 | 0 | specfence | 29796722 | 1423493 | 34.811364 |
| 21685411 | 1 | 0 |  | 0 | occ | 3530035 | 4968057 | 8.46501 |
| 21580017 | 1 | 637185 | Win_1 | 0 | specfence | 36364494 | 7064661 | 35.949014 |
| 14228336 | 2 | 0 |  | 0 | occ | 2076754 | 9377621 | 6.91221 |
| 24900199 | 2 | 634494 | Win_2 | 0 | specfence | 26817061 | 2368883 | 39.764351000000005 |
| 15409567 | 3 | 0 |  | 0 | occ | 2778551 | 4386196 | 7.300971 |
| 28084617 | 3 | 140566 | Opt | 0 | specfence | 59602450 | 1817037 | 42.78853 |
| 18063756 | 4 | 0 |  | 0 | occ | 4142071 | 9597048 | 8.641324000000001 |
| 21709909 | 4 | 314341 | Win_1 | 0 | specfence | 31998599 | 3652811 | 37.262578 |
| 20848763 | 5 | 0 |  | 0 | occ | 2412668 | 4357500 | 8.05384 |
| 22331043 | 5 | 493187 | Win_1 | 0 | specfence | 21216316 | 7377745 | 38.8632 |

> Instant-tax 规则: worker-sum Instant; do not add into wall; PROFILE can change learn

### PROFILE 块 15538827

| key | value |
|-----|-------|
| `block` | `15538827` |
| `n_tx` | `823` |
| `soft` | `0` |
| `profile_on` | `True` |
| `primary_is` | `Instant-tax reuse median (not wall PRIMARY)` |
| `occ_median_ms` | `5.804089` |
| `sf_cold_ms` | `30.616772` |
| `sf_reuse_median_ms` | `31.278623` |
| `sf_all_median_ms` | `31.141071` |
| `sf_le_occ` | `False` |
| `last_arm` | `Win_1` |
| `last_w_need` | `0` |
| `last_unfenced` | `17` |
| `last_double_pay` | `0` |
| `last_begin_n` | `85` |

| handler_ns | i | idle_core_ns | learn | maybe_wait_ns | mode | scheduler_ns | validate_ns | wall_ms |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 14219028 | 0 | 0 |  | 0 | occ | 2124851 | 1799724 | 5.732357 |
| 11353249 | 0 | 2010234 | Opt | 0 | specfence | 9154435 | 2248081 | 30.616772 |
| 15988981 | 1 | 0 |  | 0 | occ | 6182801 | 1519077 | 7.468896 |
| 11342245 | 1 | 807090 | Opt | 0 | specfence | 13879378 | 1927250 | 31.141071 |
| 12941782 | 2 | 0 |  | 0 | occ | 1638237 | 6942799 | 6.560695 |
| 10228972 | 2 | 706340 | Opt | 0 | specfence | 7717942 | 6480014 | 31.278623 |
| 11614131 | 3 | 0 |  | 0 | occ | 2479213 | 3091242 | 5.749882 |
| 13433569 | 3 | 2232846 | Opt | 0 | specfence | 12841294 | 1608159 | 32.270925 |
| 13481806 | 4 | 0 |  | 0 | occ | 1625209 | 1247164 | 5.804089 |
| 12549447 | 4 | 1481056 | Win_1 | 0 | specfence | 20624721 | 2682358 | 28.899767999999998 |
| 15623982 | 5 | 0 |  | 0 | occ | 2099933 | 1652709 | 6.345343 |
| 11388940 | 5 | 2060417 | Win_1 | 0 | specfence | 32417555 | 2619163 | 29.894876 |

> Instant-tax 规则: worker-sum Instant; do not add into wall; PROFILE can change learn

### PROFILE 块 14334629

| key | value |
|-----|-------|
| `block` | `14334629` |
| `n_tx` | `819` |
| `soft` | `0` |
| `profile_on` | `True` |
| `primary_is` | `Instant-tax reuse median (not wall PRIMARY)` |
| `occ_median_ms` | `5.792982` |
| `sf_cold_ms` | `28.937606000000002` |
| `sf_reuse_median_ms` | `26.858365` |
| `sf_all_median_ms` | `26.858365` |
| `sf_le_occ` | `False` |
| `last_arm` | `Win_2` |
| `last_w_need` | `2` |
| `last_unfenced` | `8` |
| `last_double_pay` | `1` |
| `last_begin_n` | `63` |

| handler_ns | i | idle_core_ns | learn | maybe_wait_ns | mode | scheduler_ns | validate_ns | wall_ms |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 12518681 | 0 | 0 |  | 0 | occ | 1656410 | 2588377 | 5.877908000000001 |
| 11724220 | 0 | 0 | Opt | 0 | specfence | 16909215 | 1345970 | 28.937606000000002 |
| 13502941 | 1 | 0 |  | 0 | occ | 1439963 | 2322910 | 5.488913 |
| 17966958 | 1 | 388767 | Opt | 0 | specfence | 32813757 | 2625880 | 29.266292999999997 |
| 22494894 | 2 | 0 |  | 0 | occ | 1705268 | 1277649 | 10.361244000000001 |
| 18552253 | 2 | 595628 | Win_1 | 0 | specfence | 14663651 | 4981540 | 27.978485 |
| 9912224 | 3 | 0 |  | 0 | occ | 3421447 | 959359 | 5.5092680000000005 |
| 13101083 | 3 | 410213 | Win_1 | 0 | specfence | 25408333 | 5720726 | 26.54353 |
| 13069298 | 4 | 0 |  | 0 | occ | 1287571 | 1143652 | 5.792982 |
| 15969443 | 4 | 0 | Win_2 | 0 | specfence | 25512819 | 10831736 | 26.331799999999998 |
| 33771230 | 5 | 0 |  | 0 | occ | 2762325 | 2588198 | 12.539999 |
| 14132929 | 5 | 577000 | Win_2 | 0 | specfence | 19859616 | 4188074 | 26.368206 |

> Instant-tax 规则: worker-sum Instant; do not add into wall; PROFILE can change learn

### PROFILE 块 15199017

| key | value |
|-----|-------|
| `block` | `15199017` |
| `n_tx` | `866` |
| `soft` | `0` |
| `profile_on` | `True` |
| `primary_is` | `Instant-tax reuse median (not wall PRIMARY)` |
| `occ_median_ms` | `4.596685` |
| `sf_cold_ms` | `21.540112` |
| `sf_reuse_median_ms` | `23.512041` |
| `sf_all_median_ms` | `23.261228` |
| `sf_le_occ` | `False` |
| `last_arm` | `Win_1` |
| `last_w_need` | `1` |
| `last_unfenced` | `3` |
| `last_double_pay` | `0` |
| `last_begin_n` | `44` |

| handler_ns | i | idle_core_ns | learn | maybe_wait_ns | mode | scheduler_ns | validate_ns | wall_ms |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 7292776 | 0 | 0 |  | 0 | occ | 1424527 | 1574180 | 4.767346000000001 |
| 6645944 | 0 | 0 | Full | 0 | specfence | 1635359 | 13082878 | 21.540112 |
| 6708517 | 1 | 0 |  | 0 | occ | 1076264 | 910336 | 4.390254 |
| 8785126 | 1 | 0 | Full | 0 | specfence | 1347026 | 2067214 | 22.215854 |
| 6378774 | 2 | 0 |  | 0 | occ | 1418485 | 1253941 | 4.546643 |
| 7149761 | 2 | 0 | Win_1 | 0 | specfence | 11838773 | 919164 | 23.536735999999998 |
| 8811999 | 3 | 0 |  | 0 | occ | 1602553 | 2281345 | 4.7556519999999995 |
| 7916724 | 3 | 0 | Win_1 | 0 | specfence | 1167136 | 894316 | 23.769740000000002 |
| 8008212 | 4 | 0 |  | 0 | occ | 1309597 | 1742631 | 4.401927 |
| 7431836 | 4 | 0 | Win_1 | 0 | specfence | 13802803 | 971409 | 23.512041 |
| 7947465 | 5 | 0 |  | 0 | occ | 1266851 | 1683754 | 4.607483 |
| 7168056 | 5 | 0 | Win_1 | 0 | specfence | 1534603 | 8534458 | 22.554918999999998 |

> Instant-tax 规则: worker-sum Instant; do not add into wall; PROFILE can change learn


---

## 4. 问题定位摘要

1. **全集杠杆在肥块 S-lazy/S-mixed，不在 3356896 薄输。**
2. **共同机制:** `basic_lazy` 长链 → begin 洞 → `pick_occ≈0` → n 笔 SF 调度壳 + 肥 `end_block`；OCC 用 lazy 更新 + 少量 abort。
3. **PR34 U/D/L4 下降与 K8 最慢尾正交。**
4. **局部 unfenced 低不能证明全局接近 OCC。**（15199017  clearest）
5. **Spine-U（19807137）** 仍是真 storage 长脊 CC 问题，与 S-lazy 不同刀。

## 5. 下一刀假设（分析，未落地）

| 序 | 假设 | 预期 | 风险 | 依据 |
|----|------|------|------|------|
| 1 | basic_lazy 长链永不种 OrderedAdmit（交 OCC lazy） | 砍 S-lazy 4–25× 主因 | 误标真 Basic | K8 D1 头名 lazy 459–1197 |
| 2 | 有闸时未闸仍走 `next_occ_task` | 砍整块 pick 壳 | 漏 refuse | 暖机 pick_occ≈0 |
| 3 | 肥块 end_block 再削 | 砍 0.3–3 ms | 学丢短边 | 单时钟已测 |
| 4 | n≥512 时 begin 洞 soft-cap | 砍 95 洞过预付 | 真长 Basic 欠覆盖 | 15274915/15538827 |
| 5 | lazy 预付打墙时钟（禁 Instant idle→ĉ） | 少误 Win/Defer | 仪表污染 | PROFILE 改学习史 |
| 6 | 禁 Full 超长 storage；禁 mid-plant / 长脊 Full | 防回归 | — | 19807137 / 安全史 |

## 6. Soft=0

全 K8 Instant-off / PROFILE：`soft=0`（见 JSON）。
