# S-lazy PC/CC 对象修正 → 全集 PRIMARY 主杠杆（整包落地）

**基线:** PR #34 `cursor/specfence-prepaid-losers-b5de`（代码 tip；文档 PR #35 不改行为）  
**依据:** `lab/notes/specfence-pr34-k8-pc-cc-learn-analysis.md`  
**用户确认:** 下一阶段全集 PRIMARY 以 **S-lazy PC/CC 对象修正** 为主杠杆  
**Soft=0；select_arm 唯一嘴；无分期；PC/CC 分析透镜一体脊**

---

## 0. 问题（再陈述）

肥块 S-lazy：OCC 把 **basic_lazy 数百～千写者**当廉价重叠；SF 把它当 OrderedAdmit 热 ℓ → 种洞 → **`pick_occ≈0`**（整块离开 OCC pick）→ SF 壳 + 肥 `end_block` → **4–27×**。  
局部 unfenced 低不能当赢。3356896 薄输降为次要线。

---

## 1. 整包设计

### CC 对象（主刀）

| ID | 内容 |
|----|------|
| **C1** | **`basic_lazy`（及等价 lazy 连续写链）永不种 OrderedAdmit**；不进热 D1 有序候选 / 不因该 ℓ 系统 reexec 升 Win |
| **C2** | 真 **Basic / storage** 脊仍走现有 reexec→CC 轻覆盖；与 lazy **拆开**（S-mixed：只闸真脊） |
| **C3** | 禁止用 Full 硬刚超长 storage（Spine-U 另线，本包不靠 Full(571)） |

### PC 发行（主刀）

| ID | 内容 |
|----|------|
| **P1** | **有闸 ≠ 全局模式：** 未闸 tx **必须**可走 `next_occ_task`（边约束只挡 `is_gated`） |
| **P2** | n≥512（或等价肥块）：`begin_blocked` **soft-cap**，防止 95 洞过预付 |
| **P3** | 肥块 `end_block` 再削（lazy 已见 / D1 稳定则跳冗余 merge） |

### 学习（配套）

| ID | 内容 |
|----|------|
| **L1** | morph 门控：lazy / near_independent 头名 **禁止** 有序臂进入候选 |
| **L2** | 奖励代理对齐「相对 OCC 墙」可观测信号；**禁止**只盯 unfenced 在 lazy 上升窗 |
| **L3** | 热 sticky「禁有序」决策；冷探不得在 lazy ℓ 上试 Win/Full |

### 保留

iter11 Done-on-success；禁 mid-plant；Soft=0；轻覆盖真脊；Instant idle ↛ ĉ；3356896 不回退双付。

---

## 2. 验收

1. **K8 S-lazy 代表块**（至少 14396881、15199017、13217637）：SF/OCC 倍数相对 PR34 **大幅下降**；暖机 `pick_occ` 明显 >0 或 begin 洞不再由 lazy 主导  
2. **全集扫块** Soft=0：SF≤OCC 比例上升；max 倍数下降；median 不恶化  
3. **3356896** 不显著回退（仍可薄输，但不得变肥尾）  
4. 真 Basic/storage 脊块不因 C1 漏成 PR24 级列车  
5. Soft=0；iter11；erc20_independent  

## 3. 一整 PR，无分期
