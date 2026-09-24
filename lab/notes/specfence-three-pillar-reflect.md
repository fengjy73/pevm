# SpecFence three-pillar reflect — STOP for human confirm

**Date:** 2026-09-08 (Asia/Shanghai)  
**Commit:** (this drop on `specfence` from `1058c89`)  
**Smoke:** `lab/results/three-pillar-sf-occ.json`  
**Authority:** pause-rethink + abc-unified-protocol — one coherent A∧B∧C drop, then stop.

---

## Detect / Avoid / Resolve after the drop

| Stage | Verdict |
|-------|---------|
| **Detect** | Still OK (MV + validation + sticky). 597 SF abort med **87** (tip ~164) — less over-abort noise, closer to OCC 65. |
| **Avoid** | **Fence-shaped again as Await@a** (BO-until-done + Validated spin on storm top-k ℓ). SoftWait Soft **still 0** (no Soft 1.0 storm). Quiet 598: await_a=0. 597: await_a=17 / wake_ok=2 (productive Bind-when-ready exists; wake_ok≪arms — EV still thin). |
| **Resolve** | **aj=6 on 597** (tip aj≈0) via tip≡FF max_steps 8192 + best deferred tip — SuffixRepair can skip opcodes under fan-out. FullRestart/fb still present; stretch wall &lt;10 **unmet**. |

**One-line:** Protocol shape moved off pure OCC-lite→FullRestart: storm hot ℓ Awaits at `a`, then Bind; Resolve can absolute-jump on 597. Makespan ratio ≈ tip; absolute wall not closed.

---

## Region / Fence / intra / inter

| Axis | After drop |
|------|------------|
| **Region** | ℓ = MemoryLocation; event `a=(t,k,ℓ,m)` now **drives** Await@a arm (`armed_at_k` + await_at_a counters). |
| **Fence** | SoftWait Soft dormant; Await@a uses BlockingOther prefer-steal (not FenceGraph Soft). Serial-barrier clique retained. |
| **Intra** | choose_action / learner still hot-candidate only; Await@a gated on live_fanout≥8 under Storm. |
| **Inter** | Quiet\|Storm from morph prior + flip; eng_sw≥1 on cores; **no** SoftWait bitmap copy t−1→t. 598 stays near-quiet (await_a=0). |

---

## Honest vs tip baseline (`pause-rethink-tip`)

| Metric | Tip | Three-pillar | Read |
|--------|----:|-------------:|------|
| mean SF/OCC TPS | ~0.455 | **0.449** | flat |
| 597 SF/OCC | 0.35 | **0.36** | flat |
| 597 SF wall med | 12.5 | 18.0 | abs↑; OCC also 4.0→6.1 (~1.5× host) |
| 597 SoftWait Soft | 0 | **0** | hold |
| 597 aj | ≈0 | **6** | resolve win |
| 597 await_at_a | hollow | **17 / wake_ok 2** | avoid win (partial) |
| 597 abort med | ~164 | **87** | detect/repair quieter |
| 598 wall | 2.1 | 2.9 | no SoftWait tax; Quiet OK |
| Stretch 597 &lt;10 | unmet | **unmet** | still open |

Do **not** claim wall victory. Claim: **native avoidance+resolve arms fire without SoftWait Soft regression**, ratio holds, Lean seq≡par green.

---

## What still fails

1. **597 wall &lt;10** — park+repair still dominate; wake_ok≪await_a arms.  
2. **Await@a EV** — many arms, few wake_ok; may still pay BO idle without Bind success.  
3. **599** — high abort both sides; await_a=0 (mixed morph, less live_fanout≥8 program); SF ~2.2× OCC.  
4. **RebindOnly** still rare on 597 (0 last iter).  
5. Host noise — compare **ratios**, not raw ms, across tips.

---

## Hard bans held

No SoftWait Soft 1.0, WaitHard ladders, account Wait, mass SNAP, inspect live_prime under concurrency, OCC-as-architecture.

---

## STOP

**Await human confirm before more edits.**  
Next work (only if approved): tighten Await@a EV (wake_ok↑ without SoftWait Soft), or cut remaining FullRestart opcode-seconds on 597 — not micro-gate Iter-31-style loops.
