# 盯 SF TPS&lt;OCC 的 ~70 块（中档真脊 + Opt 路径税）→ 整包落地

**基线:** PR #39 `cursor/specfence-endblock-spine-tps-c471` @ `efe87f2`  
**证据:** 99 块 Soft=0：SF TPS≥OCC 仅 **28/98**；70 块 TPS 输。输家臂：Opt 36 / Win_* 24 / Full 9；wait-set 中位 5.5、上限 8；最差 TPS 比 0.25–0.50 含大块近独立与中档真脊。  
**术语:** OrderedAdmit wait-set / ungated OCC task selection / cover_window / under-covered conflict spine / lazy-update chain / over-admission OrderedAdmit  
**Soft=0；无分期**

---

## 0. 聚类（PR39 TPS 输家）

| 簇 | 特征 | 代表 | 主税 |
|----|------|------|------|
| **T1 Opt 路径税** | 臂 Opt、仍 2–4× | 14396881, 13217637, 14029313 | SpecFence 调度/validate/壳 ≫ OCC，即使几乎无 OrderedAdmit |
| **T2 有序 wait-set 封顶仍贵** | wait-set=8 + Win/Full，×≈2–2.7 | 19860366, 19716145, 8889776, 19807137 | 轻量有序预付仍贵于 OCC abort；或 under-covered spine 空转 |
| **T3 中档真脊** | n≈176–511，真 Basic/storage | 19716145, 19638737, 16146267 | Detect 预付 + 残留路径税 |

用户指定优先 **中档真脊**；同包一并砍 T1（否则 Opt 输家仍占一半）。

---

## 1. 整包

| ID | 内容 |
|----|------|
| **M1** | 中档真脊：ĉ 更狠 — 有序预付墙 ≱ OCC abort 则 **整脊 sticky OptimisticRead**；wait-set=8 仍输则继续降 cover_window / 撤有序 |
| **M2** | Opt/近独立：`skip_ungated_path_tax` 扩大到「wait-set 空或仅短链」的中档块，validate/execute 贴 OCC |
| **M3** | under-covered conflict spine：禁止 Win_1 空转；强制 OptimisticRead 直到有证据 cover 更便宜 |
| **M4** | 修完后重跑 **99 块 Soft=0 TPS vs OCC**，目标抬高 SF TPS≥OCC 计数与 TPS 比中位 |
| **M5** | 保留：lazy-update 永不 OrderedAdmit；Done-on-success；Soft=0；专业术语 |

## 2. 验收

1. 原 70 输家里中档真脊代表（19716145、19638737、19860366、16146267 等）TPS 比上升  
2. 全集 SF TPS≥OCC 明显高于 28/98；TPS 比中位上升  
3. 墙 max 不回到 4–27× lazy-update 肥尾  
4. Soft=0；iter11；erc20  

## 3. 一整 PR
