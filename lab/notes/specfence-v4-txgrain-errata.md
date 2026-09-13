# Errata: SpecFence v4 OCC–PCC Hybrid — why **tx-level** hybrid is not enough

**Date:** 2026-09-13 (Asia/Shanghai, UTC+8)  
**Branch / HEAD at write:** `cursor/specfence-complete-cc-63b0` @ `10c7249`  
**Superseding SoT (grain):** `lab/notes/specfence-complete-architecture-v4-finegrain.md` (**v4.1**)  
**Parent (hybrid identity, still valid):** `lab/notes/specfence-complete-architecture-v4-occ-pcc-hybrid.md` (**v4.0**) — OCC default + learned PCC overlay **kept**; **tx-coarse reading of Detect/Avoid/Resolve/learning is not**.

---

## Critique (摘要)

*还要再细一些，不能只是交易级别 — v4 OCC–PCC Hybrid is still too tx-coarse. Push Detect / Avoid / timely Resolve / learning to **access-event / edge / call-frame** SoT, not transaction-level policy.*

v4.0 correctly fixed the **product identity**: OCC cost class default + always-on Detect + learning-driven timely PCC Avoid/Resolve (vs v3 ROI-sparse Fence and v2 Fence-first). It incorrectly left room — in plant shape and in how the SoT can be read — for **transaction-level policy**:

1. **“Tx \(t\) waits” as primary** — SoftWait / sticky Wait / park attached to the incarnation, so one essential observe serializes **all** remaining opcode-seconds in that tx (and often peers).  
2. **Whole-tx `ForcePrefix` bool as π** — collapses many edges into one flag; invents Fence tax on cold accesses; fights Unfenced≤OCC **per access**.  
3. **Edge key flatten \((\ell,\mathrm{reader})\) without \(k\)/depth** — loses call-frame and access ordinal; cannot express “inner CALL Fence, outer Unfenced” or “only \(k{=}6\) star SLOAD is essential.”  
4. **Learning / first-wave as per-tx sticky Wait** — AvoidBroadcast arms the rest of the tx (or all future reads of \(\ell\)) instead of **PredictedEssential\((\ell,k,\mathrm{morph})\)** / access class.  
5. **Resolve as whole-tx reincarnation / SuffixRepair ladder** — discards **certified access/frame prefix**; timely Resolve must be RebindThis / PrefixSkip first.  
6. Therefore a “hybrid” that still decides at **tx grain** cannot get OCC width on cold accesses **inside** hot txs, and still loses wall to WAIT_PARK / meta / full redo — the fan_out signature (597 tx203-class: one hot \(k{=}6\) SLOAD among 7 effects) is exactly the counterexample.

**Verdict:** OCC–PCC hybrid is a necessary **identity**; **access-event / typed-edge / call-frame** is the necessary **decision grain**. v4.1 keeps the hybrid, pushes the grain.

---

## What stays from v4.0

| Keep | Reason |
|------|--------|
| OCC cost-class default when ¬PredictedEssential | Quiet / META_COLD / cold accesses need SF≡OCC |
| Always-on Detect + timely PCC overlay (not ROI-only) | Hybrid effect vs v3 |
| Ban Unfenced > OCC | Canary/Edge tax forbidden |
| Delete SuffixRepair-as-default | R2 loses to OCC reincarnation |
| Hot serial-lane > fleet WaitFor park | 6196166 lesson |
| Soft/Await Soft / AEC / Storm / bn-hardcode bans | Unchanged |
| Quiet seed never plants H | Unchanged |

## What v4.1 replaces (tx-coarse → fine)

| v4.0 tx-coarse reading / plant shape | v4.1 fine grain |
|--------------------------------------|-----------------|
| “tx / Region set waits” | Access-event \(a=(t,\mathrm{inc},k,\mathrm{depth},\ell,\mathrm{mode})\) + typed edge |
| Sticky Wait / SoftWait-on-tx | WaitFor/Bind **for this \(a\) only** |
| ForcePrefix bool as π | **Banned** as π (metrics→0) |
| Edge SoT \((\ell,\mathrm{reader})\) | Edge SoT keeps \(k\)/depth / \(a\) |
| PredictedEssential sticky per tx/ℓ | PredictedEssential\((\ell,k,\mathrm{morph})\) / access class |
| Resolve = incarnation ladder / whole-tx redo | RebindThis / CertifiedPrefixSkip / residual; B0 only if identity lost |
| First-wave broadcast per tx | First-wave broadcast **per access class** |
| PCC overlay “on Region” vaguely | PCC **per-Region-access**; OCC for other accesses in same tx |

---

## Minimal counterexample (597 / tx203)

On `14689597`, consumer **tx203** has ~7 DB effects; the essential program RAW is essentially **one** access: \(k{=}6\) SLOAD of star \(\ell^\star\) from writer 38 (gross-work depth ≈0.94, opcode depth ≈0.77). OCC@8 still reincarnated this consumer (inc 4 in sample).

- **Tx-grain hybrid:** “tx203 is essential / ForcePrefix / sticky Wait” ⇒ Fence tax on unrelated accesses + park/makespan or whole-tx redo.  
- **Access-grain hybrid:** PCC Bind/WaitFor **only** on \(a=(203,\mathrm{inc},6,\mathrm{depth},\ell^\star,\mathrm{R})\); other accesses in tx203 stay OCC; Resolve prefers RebindThis / PrefixSkip from \(k{=}6\).

If the SoT cannot say that sentence, it is still too tx-coarse.

---

## One-line supersession

**v4.0:** OCC-default / learned fine PCC Detect+Avoid+Resolve — **hybrid identity right; grain still too tx-readable.**  
**v4.1:** Same hybrid at **access-event / typed-edge / call-frame** SoT — *primary π unit = \(a\), never “tx \(t\) waits.”*

No coding implied by this errata; land only after confirm of v4.1 SoT.
