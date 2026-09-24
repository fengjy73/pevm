# PR #34 最慢 8 块：PC / CC / 学习 三面分析（非纯数据）

**位置:** `lab/notes/`（本文件）· 镜像 [specfence-lab](https://github.com/fengjy73/specfence-lab)  
**文档 PR:** https://github.com/fengjy73/pevm/pull/35（分析，未改 CC）  
**数据底座:** [`specfence-pr34-slowest-deepdive.md`](specfence-pr34-slowest-deepdive.md)（逐 iter 表）· [`specfence-pr34-allblocks-sweep.md`](specfence-pr34-allblocks-sweep.md)  
**用语:** OptimisticRead / OrderedAdmit；Soft=0；PC 与 CC 是分析透镜，不是可拆模块  
**读法:** 先 §0 总论 → 每块 §「做了什么 / 没做好」→ §9 对照矩阵

---

## 0. 总论：三面分别在干什么

### 0.1 并行计算（PC）——目标与本尖端现实

**目标:** 在 W 核上压 makespan：依赖链走关键路径，**反链宽度吃满核**；独立工作零 SF 调度税。

**当前尖端实际做了什么:**
- 保留 OccKernel 执行核（多数块 `occ_kernel_execs≈n` 量级信号在薄块上成立；肥块 validate 常不走 OCC bool 核）。
- S1 意图：闸是边约束，未闸应 OCC pick（`occ_pick_while_gated`）。
- Soft=0：不用 soft-wait 填核。

**没做好什么（K8 共性）:**
- 暖机几乎 **`pick_occ_n≈0`**：不是「未闸在 OCC pick」，而是 **几乎没有未闸发行**——少量 OrderedAdmit 洞把 **整块 pick** 打进 wave/refuse 分支，800–1300 笔独立工作付 SF 调度壳。
- `ready_width≈0/1`：袋空，税在控制流不在堆深度。
- 肥块 `end_block` 0.3–3 ms 占关键路径，PC 视角是 **串行尾部**，与 8 核无关。
- 结果：OCC 墙 4–8 ms（lazy 重叠更新）；SF 20–110 ms —— **宽度没吃到，壳吃满了**。

### 0.2 并发控制（CC）——目标与本尖端现实

**目标:** 对真冲突边 Detect→有序或廉价 Resolve；对可交换/lazy 不当事务锁链。

**做了什么:**
- `select_arm` 唯一嘴；系统 reexec→CC；轻覆盖 `w_need`；禁双付半截+OCC 尾；Done-on-success；真 Basic/storage 脊仍可 Win/Full。
- PR34：砍 leftover Detect slide 误撤、U/D/L4 在 52 集下降。

**没做好什么:**
- 把 **basic_lazy 数百～千写者** 当成与 Basic WAW 同类的 OrderedAdmit 对象 → 种洞 / Defer / Win_1，而 OCC 几乎 abort≈个位。
- **局部 unfenced 低 ≠ 全局正确**：CC 计数「好看」时 makespan 仍 5–25×（典型 15199017）。
- Spine-U（19807137 storage 571）：有序盖不住，unf 仍高——真冲突上 Detect 强度不够或臂选错，与 S-lazy 不同刀。
- commute 在 K8 多为 0（与 3356896 的 77 不同）——肥块路径不是「commute 税」，是 **错误对象上的 Detect**。

### 0.3 学习——目标与本尖端现实

**目标:** 用墙后果更新 ĉ，让臂收敛到「整脊墙最小」；冷探热用；形态可迁移。

**做了什么:**
- 墙时钟 prepaid/refuse（产品路径非全 0）；UCB/σ 探索；热 sticky；sys-reexec 重开有序；L4 限制 cover 后被未测 Opt 先验打穿。

**没做好什么:**
- **奖励/特征与 OCC 成本模型不对齐：** 学「灭 unfenced / 降 reexec_ns」在 lazy 链上会选 Win/Defer，但 OCC 的代价几乎是 0 abort——学习优化了 **错误代理目标**。
- PROFILE Instant idle→ĉ 仍会教错（分析时 Instant-off 主证）。
- 肥块上臂在 Win_1 / Full / Defer / Opt 间摆动，**没有学到「对此 morph 永不有序」**。
- 3356896 薄输上 Win_2 已稳，学习闭环局部成功，但迁移到 lazy 肥块失败。

### 0.4 一句话因果链（K8）

```
D1/学习把 basic_lazy 长链标成热 ℓ
  → select_arm 选 Win/Defer/Full 并 begin 种洞
    → has_any_gated 贯穿 → pick_occ≈0（PC 宽度崩）
      → 整块 SF validate/调度壳 + 肥 end_block
        → 墙 4–27× OCC（OCC 只付 lazy 重叠 + 少量 abort）
```

---
## 1. 块 `14396881` — S-lazy / near_independent_meta_gap

**一句话:** 极值 25×：洞少但整块离开 OCC pick

| OCC med | SF reuse | × | Soft |
|--------:|---------:|--:|------|
| 4.403 ms | **109.9** ms | **25.0×** | 0 |

- 臂轨迹: `['Win_1', 'Win_1', 'Win_1', 'Win_1', 'Opt', 'Defer', 'Defer']`
- unfenced: `[3, 1, 1, 3, 2, 3, 4]`
- begin 洞: `[9, 9, 9, 9, 9, 6, 6]`
- pick_occ: `[1, 0, 0, 0, 0, 0, 0]`
- end_block: 0.59–1.25 ms
- w_need: `[1, 1, 1, 1, 1, 1, 2]` · refuse: `[7, 8, 9, 6, 7, 156, 4]` · dp: `[0, 0, 0, 0, 0, 0, 0]`

<details><summary>结构摘录（数据）</summary>

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

</details>

### 并行计算：做了什么 / 没做好

**做了什么**

- 仍用多 worker（cores=8）跑块；Soft=0 不靠挂起填核。
- 设计上保留「未闸 OCC pick」接口；独立集不应被有序洞串行化。
- 本块偶发 `pick_occ>0`（见轨迹），说明双路径并非从未触发。

**没做好**

- **PC 主败:** 暖机 `pick_occ≡0` → 有闸模式污染整块发行；反链宽度（OCC 能吃的独立更新）被 SF 调度壳吞掉。
- begin 洞与 n_tx 完全不成比例时（洞≪n 仍 pick_occ=0），证明失败模式是 **全局模式开关**，不是「洞上的串行前缀」本身。
- 块末 1.25 ms 级串行尾，直接加在 makespan 上，与核数无关。
- 相对 OCC：OCC 在短 L、大 W 的 lazy 形态上墙低，说明 **硬件并行度够**；SF 慢不是算力不够，是发行策略把宽度关了。

### 并发控制：做了什么 / 没做好

**做了什么**

- 走 SpecFence 嘴：有臂选择、可能有序覆盖、统计 unfenced/refuse/dp。
- 若干 iter unfenced 维持个位数 → **局部** Detect/覆盖对「可见 abort 列车」有效或冲突本就不爆。
- 本块 dp 多为 0：不是双付账本主导。

**没做好**

- **对象错误:** 对 basic_lazy 长链做 OrderedAdmit/Defer，OCC 侧几乎不付等价代价 → CC 在锁一条「不该锁的链」。
- **指标错位:** 优化 unfenced 不能证明对 OCC 竞争；本块可以「CC 计数及格、墙惨败」。
- 与 OCC 对比：OCC abort 往往远小于 SF 种洞数所暗示的「冲突规模」→ SF 的 Detect 集合 **过近似**。

### 学习：做了什么 / 没做好

**做了什么**

- 跨 iter 臂有变化（见轨迹），说明嘴在动，不是完全死阈值。
- 使用墙相关计量（prepaid/refuse/end 等字段在 tip 上非全 0）。
- 出现 Win_*：系统 reexec→有序原则在触发。
- 出现 Defer/Opt：L4/预付爆破回退路径存在。

**没做好**

- **代理目标错误:** 在 lazy 形态上学「降 reexec / 升覆盖」会把 ĉ 推向有序，但 OCC 最优是 **不覆盖**。
- 臂集合在 {'Opt', 'Win_1', 'Defer'} 间摆动，**未收敛到稳定「对此 morph 禁止有序」**。
- 缺少（或未生效）「lazy 长链 / near_independent」形态特征门控 → 泛化失败：3356896 上学到的 Win_2 轻覆盖，迁到肥 lazy 变成灾难。
- 若用 PROFILE 主证，Instant 税会污染 ĉ（本分析以 Instant-off 为准）。

### 本块结论

优先刀：**lazy 不进 OrderedAdmit** + **有闸时未闸 OCC pick**；end_block 为辅。不要再加宽有序。

---

## 2. 块 `15274915` — S-mixed

**一句话:** 真 Basic 长脊 + lazy；begin≈95 过预付

| OCC med | SF reuse | × | Soft |
|--------:|---------:|--:|------|
| 5.313 ms | **79.0** ms | **14.9×** | 0 |

- 臂轨迹: `['Opt', 'Opt', 'Opt', 'Full', 'Win_1', 'Win_1', 'Opt']`
- unfenced: `[7, 3, 3, 7, 4, 6, 7]`
- begin 洞: `[95, 95, 95, 95, 95, 95, 95]`
- pick_occ: `[0, 0, 0, 0, 0, 0, 0]`
- end_block: 0.34–1.29 ms
- w_need: `[0, 0, 0, 0, 0, 0, 0]` · refuse: `[433, 78, 99, 84, 91, 102, 90]` · dp: `[0, 0, 0, 0, 0, 0, 0]`

<details><summary>结构摘录（数据）</summary>

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

</details>

### 并行计算：做了什么 / 没做好

**做了什么**

- 仍用多 worker（cores=8）跑块；Soft=0 不靠挂起填核。
- 设计上保留「未闸 OCC pick」接口；独立集不应被有序洞串行化。
- （本块暖机几乎无成功的未闸 OCC pick。）

**没做好**

- **PC 主败:** 暖机 `pick_occ≡0` → 有闸模式污染整块发行；反链宽度（OCC 能吃的独立更新）被 SF 调度壳吞掉。
- begin 洞与 n_tx 完全不成比例时（洞≪n 仍 pick_occ=0），证明失败模式是 **全局模式开关**，不是「洞上的串行前缀」本身。
- 块末 1.29 ms 级串行尾，直接加在 makespan 上，与核数无关。
- begin 洞峰值 95：过预付把可并行前缀压成等待图。
- 相对 OCC：OCC 在短 L、大 W 的 lazy 形态上墙低，说明 **硬件并行度够**；SF 慢不是算力不够，是发行策略把宽度关了。

### 并发控制：做了什么 / 没做好

**做了什么**

- 走 SpecFence 嘴：有臂选择、可能有序覆盖、统计 unfenced/refuse/dp。
- 若干 iter unfenced 维持个位数 → **局部** Detect/覆盖对「可见 abort 列车」有效或冲突本就不爆。
- 本块 dp 多为 0：不是双付账本主导。

**没做好**

- **真假冲突绑在一起:** 真 Basic 脊需要有序，但与 lazy 千级写者叠加时 begin 洞爆炸（~95），CC 过预付。
- 与 OCC 对比：OCC abort 往往远小于 SF 种洞数所暗示的「冲突规模」→ SF 的 Detect 集合 **过近似**。

### 学习：做了什么 / 没做好

**做了什么**

- 跨 iter 臂有变化（见轨迹），说明嘴在动，不是完全死阈值。
- 使用墙相关计量（prepaid/refuse/end 等字段在 tip 上非全 0）。
- 出现 Win_*：系统 reexec→有序原则在触发。
- 出现 Defer/Opt：L4/预付爆破回退路径存在。

**没做好**

- **代理目标错误:** 在 lazy 形态上学「降 reexec / 升覆盖」会把 ĉ 推向有序，但 OCC 最优是 **不覆盖**。
- 臂集合在 {'Opt', 'Win_1', 'Full'} 间摆动，**未收敛到稳定「对此 morph 禁止有序」**。
- 缺少（或未生效）「lazy 长链 / near_independent」形态特征门控 → 泛化失败：3356896 上学到的 Win_2 轻覆盖，迁到肥 lazy 变成灾难。
- 若用 PROFILE 主证，Instant 税会污染 ĉ（本分析以 Instant-off 为准）。

### 本块结论

优先刀：**拆开真 Basic 与 lazy**——只对真脊种洞，lazy 交 OCC；并硬限 begin 洞上限。

---

## 3. 块 `13217637` — S-lazy

**一句话:** L 短、lazy 千级；SF 墙钉死

| OCC med | SF reuse | × | Soft |
|--------:|---------:|--:|------|
| 4.731 ms | **64.1** ms | **13.5×** | 0 |

- 臂轨迹: `['Full', 'Full', 'Full', 'Full', 'Full', 'Full', 'Win_1']`
- unfenced: `[6, 9, 9, 5, 4, 6, 7]`
- begin 洞: `[29, 29, 26, 26, 26, 26, 26]`
- pick_occ: `[0, 0, 0, 0, 0, 0, 1]`
- end_block: 0.28–1.36 ms
- w_need: `[0, 0, 0, 0, 0, 0, 0]` · refuse: `[11, 22, 19, 12, 13, 14, 14]` · dp: `[0, 0, 0, 0, 0, 0, 0]`

<details><summary>结构摘录（数据）</summary>

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

</details>

### 并行计算：做了什么 / 没做好

**做了什么**

- 仍用多 worker（cores=8）跑块；Soft=0 不靠挂起填核。
- 设计上保留「未闸 OCC pick」接口；独立集不应被有序洞串行化。
- 本块偶发 `pick_occ>0`（见轨迹），说明双路径并非从未触发。

**没做好**

- begin 洞与 n_tx 完全不成比例时（洞≪n 仍 pick_occ=0），证明失败模式是 **全局模式开关**，不是「洞上的串行前缀」本身。
- 块末 1.36 ms 级串行尾，直接加在 makespan 上，与核数无关。
- 相对 OCC：OCC 在短 L、大 W 的 lazy 形态上墙低，说明 **硬件并行度够**；SF 慢不是算力不够，是发行策略把宽度关了。

### 并发控制：做了什么 / 没做好

**做了什么**

- 走 SpecFence 嘴：有臂选择、可能有序覆盖、统计 unfenced/refuse/dp。
- 若干 iter unfenced 维持个位数 → **局部** Detect/覆盖对「可见 abort 列车」有效或冲突本就不爆。
- 本块 dp 多为 0：不是双付账本主导。

**没做好**

- **对象错误:** 对 basic_lazy 长链做 OrderedAdmit/Defer，OCC 侧几乎不付等价代价 → CC 在锁一条「不该锁的链」。
- **指标错位:** 优化 unfenced 不能证明对 OCC 竞争；本块可以「CC 计数及格、墙惨败」。
- 与 OCC 对比：OCC abort 往往远小于 SF 种洞数所暗示的「冲突规模」→ SF 的 Detect 集合 **过近似**。

### 学习：做了什么 / 没做好

**做了什么**

- 跨 iter 臂有变化（见轨迹），说明嘴在动，不是完全死阈值。
- 使用墙相关计量（prepaid/refuse/end 等字段在 tip 上非全 0）。
- 出现 Win_*：系统 reexec→有序原则在触发。

**没做好**

- **代理目标错误:** 在 lazy 形态上学「降 reexec / 升覆盖」会把 ĉ 推向有序，但 OCC 最优是 **不覆盖**。
- 缺少（或未生效）「lazy 长链 / near_independent」形态特征门控 → 泛化失败：3356896 上学到的 Win_2 轻覆盖，迁到肥 lazy 变成灾难。
- 若用 PROFILE 主证，Instant 税会污染 ĉ（本分析以 Instant-off 为准）。

### 本块结论

优先刀：**lazy 不进 OrderedAdmit** + **有闸时未闸 OCC pick**；end_block 为辅。不要再加宽有序。

---

## 4. 块 `19807137` — Spine-U

**一句话:** storage 长脊；有序盖不住

| OCC med | SF reuse | × | Soft |
|--------:|---------:|--:|------|
| 17.590 ms | **64.7** ms | **3.7×** | 0 |

- 臂轨迹: `['Opt', 'Full', 'Full', 'Full', 'Full', 'Win_1', 'Win_1']`
- unfenced: `[575, 136, 183, 116, 164, 79, 236]`
- begin 洞: `[38, 38, 38, 38, 38, 38, 38]`
- pick_occ: `[0, 2, 0, 1, 0, 3, 0]`
- end_block: 0.50–1.73 ms
- w_need: `[0, 0, 0, 0, 0, 0, 0]` · refuse: `[51, 214, 292, 198, 220, 243, 226]` · dp: `[0, 0, 0, 0, 0, 0, 0]`

<details><summary>结构摘录（数据）</summary>

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

</details>

### 并行计算：做了什么 / 没做好

**做了什么**

- 仍用多 worker（cores=8）跑块；Soft=0 不靠挂起填核。
- 设计上保留「未闸 OCC pick」接口；独立集不应被有序洞串行化。
- 本块偶发 `pick_occ>0`（见轨迹），说明双路径并非从未触发。

**没做好**

- begin 洞与 n_tx 完全不成比例时（洞≪n 仍 pick_occ=0），证明失败模式是 **全局模式开关**，不是「洞上的串行前缀」本身。
- 块末 1.73 ms 级串行尾，直接加在 makespan 上，与核数无关。
- 相对 OCC：OCC 在短 L、大 W 的 lazy 形态上墙低，说明 **硬件并行度够**；SF 慢不是算力不够，是发行策略把宽度关了。

### 并发控制：做了什么 / 没做好

**做了什么**

- 走 SpecFence 嘴：有臂选择、可能有序覆盖、统计 unfenced/refuse/dp。
- 本块 dp 多为 0：不是双付账本主导。

**没做好**

- **真脊盖不住:** storage 超长 writer 上 Win_1/Full 仍高 unfenced → Detect 强度/臂宽不够，冲突仍在 Resolve 列车。
- 与 OCC 对比：OCC abort 往往远小于 SF 种洞数所暗示的「冲突规模」→ SF 的 Detect 集合 **过近似**。

### 学习：做了什么 / 没做好

**做了什么**

- 跨 iter 臂有变化（见轨迹），说明嘴在动，不是完全死阈值。
- 使用墙相关计量（prepaid/refuse/end 等字段在 tip 上非全 0）。
- 出现 Win_*：系统 reexec→有序原则在触发。
- 出现 Defer/Opt：L4/预付爆破回退路径存在。

**没做好**

- **代理目标错误:** 在 lazy 形态上学「降 reexec / 升覆盖」会把 ĉ 推向有序，但 OCC 最优是 **不覆盖**。
- 臂集合在 {'Opt', 'Win_1', 'Full'} 间摆动，**未收敛到稳定「对此 morph 禁止有序」**。
- 缺少（或未生效）「lazy 长链 / near_independent」形态特征门控 → 泛化失败：3356896 上学到的 Win_2 轻覆盖，迁到肥 lazy 变成灾难。
- 若用 PROFILE 主证，Instant 税会污染 ĉ（本分析以 Instant-off 为准）。

### 本块结论

优先刀：**真 storage 脊的覆盖策略**（足够 w / 分段），同时避免 Full 爆炸；与 S-lazy 分治。

---

## 5. 块 `17666333` — S-lazy

**一句话:** 双 lazy 头 + 轻 WAW

| OCC med | SF reuse | × | Soft |
|--------:|---------:|--:|------|
| 7.374 ms | **35.1** ms | **4.8×** | 0 |

- 臂轨迹: `['Opt', 'Opt', 'Opt', 'Opt', 'Full', 'Win_1', 'Win_1']`
- unfenced: `[33, 7, 24, 9, 7, 18, 13]`
- begin 洞: `[36, 36, 36, 36, 36, 36, 36]`
- pick_occ: `[0, 0, 0, 0, 0, 0, 0]`
- end_block: 0.36–1.98 ms
- w_need: `[0, 0, 0, 0, 0, 0, 0]` · refuse: `[58, 48, 35, 53, 69, 76, 44]` · dp: `[0, 0, 0, 0, 0, 0, 0]`

<details><summary>结构摘录（数据）</summary>

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

</details>

### 并行计算：做了什么 / 没做好

**做了什么**

- 仍用多 worker（cores=8）跑块；Soft=0 不靠挂起填核。
- 设计上保留「未闸 OCC pick」接口；独立集不应被有序洞串行化。
- （本块暖机几乎无成功的未闸 OCC pick。）

**没做好**

- **PC 主败:** 暖机 `pick_occ≡0` → 有闸模式污染整块发行；反链宽度（OCC 能吃的独立更新）被 SF 调度壳吞掉。
- begin 洞与 n_tx 完全不成比例时（洞≪n 仍 pick_occ=0），证明失败模式是 **全局模式开关**，不是「洞上的串行前缀」本身。
- 块末 1.98 ms 级串行尾，直接加在 makespan 上，与核数无关。
- 相对 OCC：OCC 在短 L、大 W 的 lazy 形态上墙低，说明 **硬件并行度够**；SF 慢不是算力不够，是发行策略把宽度关了。

### 并发控制：做了什么 / 没做好

**做了什么**

- 走 SpecFence 嘴：有臂选择、可能有序覆盖、统计 unfenced/refuse/dp。
- 本块 dp 多为 0：不是双付账本主导。

**没做好**

- **对象错误:** 对 basic_lazy 长链做 OrderedAdmit/Defer，OCC 侧几乎不付等价代价 → CC 在锁一条「不该锁的链」。
- **指标错位:** 优化 unfenced 不能证明对 OCC 竞争；本块可以「CC 计数及格、墙惨败」。
- 与 OCC 对比：OCC abort 往往远小于 SF 种洞数所暗示的「冲突规模」→ SF 的 Detect 集合 **过近似**。

### 学习：做了什么 / 没做好

**做了什么**

- 跨 iter 臂有变化（见轨迹），说明嘴在动，不是完全死阈值。
- 使用墙相关计量（prepaid/refuse/end 等字段在 tip 上非全 0）。
- 出现 Win_*：系统 reexec→有序原则在触发。
- 出现 Defer/Opt：L4/预付爆破回退路径存在。

**没做好**

- **代理目标错误:** 在 lazy 形态上学「降 reexec / 升覆盖」会把 ĉ 推向有序，但 OCC 最优是 **不覆盖**。
- 臂集合在 {'Opt', 'Win_1', 'Full'} 间摆动，**未收敛到稳定「对此 morph 禁止有序」**。
- 缺少（或未生效）「lazy 长链 / near_independent」形态特征门控 → 泛化失败：3356896 上学到的 Win_2 轻覆盖，迁到肥 lazy 变成灾难。
- 若用 PROFILE 主证，Instant 税会污染 ĉ（本分析以 Instant-off 为准）。

### 本块结论

优先刀：**lazy 不进 OrderedAdmit** + **有闸时未闸 OCC pick**；end_block 为辅。不要再加宽有序。

---

## 6. 块 `15538827` — S / S-lazy

**一句话:** 洞 79–85；end_block 双峰至 3 ms

| OCC med | SF reuse | × | Soft |
|--------:|---------:|--:|------|
| 5.675 ms | **28.6** ms | **5.0×** | 0 |

- 臂轨迹: `['Opt', 'Opt', 'Opt', 'Full', 'Full', 'Win_1', 'Win_1']`
- unfenced: `[25, 10, 20, 23, 33, 13, 21]`
- begin 洞: `[85, 79, 79, 79, 85, 85, 85]`
- pick_occ: `[0, 0, 0, 0, 0, 0, 0]`
- end_block: 0.31–3.07 ms
- w_need: `[0, 0, 0, 0, 0, 0, 0]` · refuse: `[38, 56, 104, 73, 63, 59, 61]` · dp: `[0, 0, 0, 0, 0, 0, 0]`

<details><summary>结构摘录（数据）</summary>

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
 
```

</details>

### 并行计算：做了什么 / 没做好

**做了什么**

- 仍用多 worker（cores=8）跑块；Soft=0 不靠挂起填核。
- 设计上保留「未闸 OCC pick」接口；独立集不应被有序洞串行化。
- （本块暖机几乎无成功的未闸 OCC pick。）

**没做好**

- **PC 主败:** 暖机 `pick_occ≡0` → 有闸模式污染整块发行；反链宽度（OCC 能吃的独立更新）被 SF 调度壳吞掉。
- begin 洞与 n_tx 完全不成比例时（洞≪n 仍 pick_occ=0），证明失败模式是 **全局模式开关**，不是「洞上的串行前缀」本身。
- 块末 3.07 ms 级串行尾，直接加在 makespan 上，与核数无关。
- begin 洞峰值 85：过预付把可并行前缀压成等待图。
- 相对 OCC：OCC 在短 L、大 W 的 lazy 形态上墙低，说明 **硬件并行度够**；SF 慢不是算力不够，是发行策略把宽度关了。

### 并发控制：做了什么 / 没做好

**做了什么**

- 走 SpecFence 嘴：有臂选择、可能有序覆盖、统计 unfenced/refuse/dp。
- 本块 dp 多为 0：不是双付账本主导。

**没做好**

- **对象错误:** 对 basic_lazy 长链做 OrderedAdmit/Defer，OCC 侧几乎不付等价代价 → CC 在锁一条「不该锁的链」。
- **指标错位:** 优化 unfenced 不能证明对 OCC 竞争；本块可以「CC 计数及格、墙惨败」。
- 与 OCC 对比：OCC abort 往往远小于 SF 种洞数所暗示的「冲突规模」→ SF 的 Detect 集合 **过近似**。

### 学习：做了什么 / 没做好

**做了什么**

- 跨 iter 臂有变化（见轨迹），说明嘴在动，不是完全死阈值。
- 使用墙相关计量（prepaid/refuse/end 等字段在 tip 上非全 0）。
- 出现 Win_*：系统 reexec→有序原则在触发。
- 出现 Defer/Opt：L4/预付爆破回退路径存在。

**没做好**

- **代理目标错误:** 在 lazy 形态上学「降 reexec / 升覆盖」会把 ĉ 推向有序，但 OCC 最优是 **不覆盖**。
- 臂集合在 {'Opt', 'Win_1', 'Full'} 间摆动，**未收敛到稳定「对此 morph 禁止有序」**。
- 缺少（或未生效）「lazy 长链 / near_independent」形态特征门控 → 泛化失败：3356896 上学到的 Win_2 轻覆盖，迁到肥 lazy 变成灾难。
- 若用 PROFILE 主证，Instant 税会污染 ĉ（本分析以 Instant-off 为准）。

### 本块结论

优先刀：**lazy 不进 OrderedAdmit** + **有闸时未闸 OCC pick**；end_block 为辅。不要再加宽有序。

---

## 7. 块 `14334629` — S-lazy

**一句话:** 短 Full + lazy 头；仍整块 SF pick

| OCC med | SF reuse | × | Soft |
|--------:|---------:|--:|------|
| 6.246 ms | **27.1** ms | **4.3×** | 0 |

- 臂轨迹: `['Opt', 'Opt', 'Opt', 'Full', 'Full', 'Full', 'Full']`
- unfenced: `[17, 17, 11, 15, 11, 17, 14]`
- begin 洞: `[63, 63, 63, 63, 63, 63, 63]`
- pick_occ: `[0, 0, 0, 0, 0, 0, 0]`
- end_block: 0.35–3.06 ms
- w_need: `[0, 0, 0, 0, 0, 0, 0]` · refuse: `[33, 36, 15, 39, 34, 23, 23]` · dp: `[0, 0, 0, 0, 0, 0, 0]`

<details><summary>结构摘录（数据）</summary>

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

```

</details>

### 并行计算：做了什么 / 没做好

**做了什么**

- 仍用多 worker（cores=8）跑块；Soft=0 不靠挂起填核。
- 设计上保留「未闸 OCC pick」接口；独立集不应被有序洞串行化。
- （本块暖机几乎无成功的未闸 OCC pick。）

**没做好**

- **PC 主败:** 暖机 `pick_occ≡0` → 有闸模式污染整块发行；反链宽度（OCC 能吃的独立更新）被 SF 调度壳吞掉。
- begin 洞与 n_tx 完全不成比例时（洞≪n 仍 pick_occ=0），证明失败模式是 **全局模式开关**，不是「洞上的串行前缀」本身。
- 块末 3.06 ms 级串行尾，直接加在 makespan 上，与核数无关。
- begin 洞峰值 63：过预付把可并行前缀压成等待图。
- 相对 OCC：OCC 在短 L、大 W 的 lazy 形态上墙低，说明 **硬件并行度够**；SF 慢不是算力不够，是发行策略把宽度关了。

### 并发控制：做了什么 / 没做好

**做了什么**

- 走 SpecFence 嘴：有臂选择、可能有序覆盖、统计 unfenced/refuse/dp。
- 本块 dp 多为 0：不是双付账本主导。

**没做好**

- **对象错误:** 对 basic_lazy 长链做 OrderedAdmit/Defer，OCC 侧几乎不付等价代价 → CC 在锁一条「不该锁的链」。
- **指标错位:** 优化 unfenced 不能证明对 OCC 竞争；本块可以「CC 计数及格、墙惨败」。
- 与 OCC 对比：OCC abort 往往远小于 SF 种洞数所暗示的「冲突规模」→ SF 的 Detect 集合 **过近似**。

### 学习：做了什么 / 没做好

**做了什么**

- 跨 iter 臂有变化（见轨迹），说明嘴在动，不是完全死阈值。
- 使用墙相关计量（prepaid/refuse/end 等字段在 tip 上非全 0）。
- 出现 Defer/Opt：L4/预付爆破回退路径存在。

**没做好**

- **代理目标错误:** 在 lazy 形态上学「降 reexec / 升覆盖」会把 ĉ 推向有序，但 OCC 最优是 **不覆盖**。
- 缺少（或未生效）「lazy 长链 / near_independent」形态特征门控 → 泛化失败：3356896 上学到的 Win_2 轻覆盖，迁到肥 lazy 变成灾难。
- 若用 PROFILE 主证，Instant 税会污染 ĉ（本分析以 Instant-off 为准）。

### 本块结论

优先刀：**lazy 不进 OrderedAdmit** + **有闸时未闸 OCC pick**；end_block 为辅。不要再加宽有序。

---

## 8. 块 `15199017` — S-lazy 典范

**一句话:** 局部 unf≈0、全局 5×：局部赢全局输

| OCC med | SF reuse | × | Soft |
|--------:|---------:|--:|------|
| 4.276 ms | **21.9** ms | **5.1×** | 0 |

- 臂轨迹: `['Win_1', 'Win_1', 'Win_1', 'Win_1', 'Win_1', 'Win_1', 'Win_1']`
- unfenced: `[6, 5, 4, 5, 5, 5, 3]`
- begin 洞: `[44, 44, 44, 44, 44, 44, 44]`
- pick_occ: `[0, 0, 0, 0, 0, 0, 0]`
- end_block: 0.31–0.40 ms
- w_need: `[0, 0, 0, 0, 0, 0, 1]` · refuse: `[35, 49, 35, 38, 22, 25, 39]` · dp: `[0, 0, 0, 0, 0, 0, 0]`

<details><summary>结构摘录（数据）</summary>

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
        2
```

</details>

### 并行计算：做了什么 / 没做好

**做了什么**

- 仍用多 worker（cores=8）跑块；Soft=0 不靠挂起填核。
- 设计上保留「未闸 OCC pick」接口；独立集不应被有序洞串行化。
- （本块暖机几乎无成功的未闸 OCC pick。）

**没做好**

- **PC 主败:** 暖机 `pick_occ≡0` → 有闸模式污染整块发行；反链宽度（OCC 能吃的独立更新）被 SF 调度壳吞掉。
- begin 洞与 n_tx 完全不成比例时（洞≪n 仍 pick_occ=0），证明失败模式是 **全局模式开关**，不是「洞上的串行前缀」本身。
- begin 洞峰值 44：过预付把可并行前缀压成等待图。
- 相对 OCC：OCC 在短 L、大 W 的 lazy 形态上墙低，说明 **硬件并行度够**；SF 慢不是算力不够，是发行策略把宽度关了。

### 并发控制：做了什么 / 没做好

**做了什么**

- 走 SpecFence 嘴：有臂选择、可能有序覆盖、统计 unfenced/refuse/dp。
- 若干 iter unfenced 维持个位数 → **局部** Detect/覆盖对「可见 abort 列车」有效或冲突本就不爆。
- 本块 dp 多为 0：不是双付账本主导。

**没做好**

- **对象错误:** 对 basic_lazy 长链做 OrderedAdmit/Defer，OCC 侧几乎不付等价代价 → CC 在锁一条「不该锁的链」。
- **指标错位:** 优化 unfenced 不能证明对 OCC 竞争；本块可以「CC 计数及格、墙惨败」。
- 与 OCC 对比：OCC abort 往往远小于 SF 种洞数所暗示的「冲突规模」→ SF 的 Detect 集合 **过近似**。

### 学习：做了什么 / 没做好

**做了什么**

- 跨 iter 臂有变化（见轨迹），说明嘴在动，不是完全死阈值。
- 使用墙相关计量（prepaid/refuse/end 等字段在 tip 上非全 0）。
- 出现 Win_*：系统 reexec→有序原则在触发。

**没做好**

- **代理目标错误:** 在 lazy 形态上学「降 reexec / 升覆盖」会把 ĉ 推向有序，但 OCC 最优是 **不覆盖**。
- 缺少（或未生效）「lazy 长链 / near_independent」形态特征门控 → 泛化失败：3356896 上学到的 Win_2 轻覆盖，迁到肥 lazy 变成灾难。
- 若用 PROFILE 主证，Instant 税会污染 ĉ（本分析以 Instant-off 为准）。

### 本块结论

优先刀：**lazy 不进 OrderedAdmit** + **有闸时未闸 OCC pick**；end_block 为辅。不要再加宽有序。

---

## 9. 对照矩阵（K8）

| 块 | 类 | PC 主败 | CC 主败 | 学习主败 | 下一刀优先 |
|----|----|---------|---------|----------|------------|
| 14396881 | S-lazy | 洞少仍 pick_occ=0 | lazy 当锁链 | Defer/Win 对 lazy | lazy 禁有序 + OCC pick |
| 15274915 | S-mixed | 95 洞过预付 | 真脊+lazy 绑死 | 未拆 morph | 拆 lazy / 洞 cap |
| 13217637 | S-lazy | 同左 | lazy 千级 | Win_1 钉死 | lazy 禁有序 |
| 19807137 | Spine-U | 壳+真串行 | 有序盖不住 storage | 臂偏弱 | 真脊覆盖分治 |
| 17666333 | S-lazy | pick_occ=0 | 双 lazy 头 | Win_1 | lazy 禁有序 |
| 15538827 | S | 洞 80+；end 至 3ms | 过预付 | Win_1 | 洞 cap + end_block |
| 14334629 | S-lazy | pick_occ=0 | Full 短+ lazy | Full/Win | lazy 禁有序 |
| 15199017 | S-lazy 典范 | 局部好全局崩 | unf≈0 仍 5× | 代理目标错 | **最干净反例** |

## 10. 与 3356896 薄输的关系

| | 3356896 | K8 肥尾 |
|--|---------|---------|
| 形态 | 短 Basic WAW 脊 | basic_lazy 千级 / 混脊 |
| CC | Win_2 覆盖，unf≈0 | 常 unf 低或中，仍惨 |
| PC | 薄壳 + 少量洞 | **整块离开 OCC pick** |
| 学习 | 轻覆盖局部成功 | 错误对象上「成功」|
| 杠杆 | ~0.2 ms | **数倍～25×** |

**工程含义:** 下一阶段全集 PRIMARY 必须以 **S-lazy PC/CC 对象修正** 为主杠杆；3356896 薄壳优化是次要并行线，不能再当唯一北极星。

## 11. 建议落地顺序（仍是分析结论）

1. **CC 对象:** `basic_lazy`（及 near_independent 头名 lazy）禁止 OrderedAdmit / 不进热 D1 有序候选。  
2. **PC 发行:** 有闸时未闸强制 `next_occ_task`（边约束≠全局模式）。  
3. **PC 尾:** 肥块 `end_block` 再削。  
4. **CC 洞:** n≥512 soft-cap `begin_blocked`。  
5. **学习:** morph 门控 + 奖励改为「相对 OCC 墙」可观测代理（禁止只盯 unfenced）。  
6. **分治:** Spine-U 另线，禁止用 Full(571) 硬刚。

---

## 12. Soft=0

K8 Instant-off / 扫块：Soft=0（见 summary JSON）。
