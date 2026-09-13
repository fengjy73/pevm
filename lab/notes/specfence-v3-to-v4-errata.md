# Errata: SpecFence architecture v3 (CostGate) → v4 (Learned OCC–PCC Hybrid)

**Date:** 2026-09-13 (Asia/Shanghai, UTC+8)  
**Branch / HEAD at write:** `cursor/specfence-complete-cc-63b0` @ `40903a3`  
**Superseding SoT:** `lab/notes/specfence-complete-architecture-v4-occ-pcc-hybrid.md`  
**Superseded face:** `lab/notes/specfence-complete-architecture-v3.md` (CostGate — historical)

---

## Why CostGate alone is insufficient

User critique (摘要): *还是不够 — 默认 OCC 对，但对 OCC 要做细粒度、学习驱动的冲突发现 / 避免 / 及时解决，才能达到类似 OCC+PCC 混合的效果。*

v3 correctly fixed the **cost-class** mistake (Unfenced must ≡ OCC; Fence-first meta was too expensive). It incorrectly made **Fence admission sparse** — “Fence only if ROI proves.” That:

1. **Silences Avoid** on the fan_out majority when the gate defaults deny — so the system never behaves like a PCC overlay; it is just OCC with optional rare barriers.  
2. **Treats Detect as ROI feedstock**, not as a continuous fine sensor that must always run and drive **timely** actuators.  
3. **Defers Resolve** to validate/reincarnate — correct as the **miss** path, wrong as the **only** path when learning already predicts essential anti-deps.  
4. Therefore **cannot achieve OCC+PCC hybrid effect**: either admit∅ (pure OCC, no hybrid win on contended Regions) or rare Fence (still not continuous learned Detect→Avoid→timely Resolve).

**Verdict:** CostGate is a necessary **cost-class constraint**, not a sufficient **control-plane identity**. v4 keeps OCC default + Unfenced≤OCC ban, and replaces ROI-only Fence-as-face with **always-on fine Detect + learning-driven timely PCC Avoid/Resolve**.

---

## What stays from v3

| Keep | Reason |
|------|--------|
| OCC cost-class baseline | Evidence: META_COLD / quiet need SF≡OCC |
| Ban Unfenced > OCC | Canary/Edge tax forbidden |
| Delete SuffixRepair-as-default | R2 loses to OCC reincarnation |
| Hot Region serial-lane idea | Prefer over fleet WaitFor park |
| Soft/Await / AEC / Storm / bn-hardcode bans | Unchanged |
| Quiet seed never plants H | Unchanged |

## What v4 replaces

| v3 | v4 |
|----|----|
| CostGate admit(R) as product face | Learned PredictedEssential → PCC overlay |
| Default deny ⇒ Detect→silence for Fence | Detect **always**; Avoid when predicted |
| Fence only if ROI | Timely fine PCC when learning says essential |
| Resolve ≈ reincarnation (late) | Timely Avoid/R1/E1 + reincarnation on **miss** |
| Learning = ROI EMA | Learning first-class trains Detect/Avoid/Resolve |

---

## One-line supersession

**v3:** OCC-default / Fence-on-ROI (too sparse — 不够).  
**v4:** OCC-default / **learned fine PCC Always-Detect + timely Avoid/Resolve** (OCC+PCC hybrid).

No coding implied by this errata; land only after confirm of v4 SoT.
