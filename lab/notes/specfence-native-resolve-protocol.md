# SpecFence-native resolve protocol (not OCC++)

**Date:** 2026-09-07  
**Status:** AUTHORITATIVE for resolve redesign — SpecFence is its **own** CC protocol.  
**OCC role:** sequential≡parallel baseline number only. **Not** the architectural template.

---

## 1. Break with OCC

OCC = speculate → validate → **abort whole tx** → reexec from head (+ ESTIMATE).

SpecFence must **not** be “OCC with ForceBind labels.” Dig proved that path: more aborts, same cost grain, meta tax → lose.

SpecFence thesis:

> Concurrency object = `MemoryLocation` ℓ; event = access `(t,k,ℓ,m)`.  
> **Resolve** means restore a serializable continuation for the **suffix after the first conflicting access**, not restart the transaction as the default verb.

---

## 2. Protocol verbs (native)

| Verb | When | Effect |
|------|------|--------|
| **Bind** | Producer version known (Data) or prior says bindable | Install origin; continue consumer — **no abort** |
| **Await** (SoftWait) | Producer running / known unfinished; EV_Await < EV_reexec | Park at `k`; wake → **resume at/after k** (not head) |
| **SuffixRepair** | Validation / EarlyVal fail at first bad `k` | Invalidate suffix writes; **RewindTo certified checkpoint ≤ k**; journal FF; force-bind prefix; **never prefer FullRestart** |
| **FullRestart** | No checkpoint / control-flow broken | Last resort only |
| **SpecRead** | Writer absent / unknown; discovery | Allowed, but not the identity of the protocol |

**Forbidden identity:** “default SpecRead + abort≈OCC.” That is OCC wearing a SpecFence hat.

---

## 3. Resolve law (iron)

1. **Abort is not the product.** Prefer Bind/Await so validation never fails.  
2. When fail is inevitable: **SuffixRepair** is the default resolve; FullRestart is exceptional.  
3. Resume after Await/SuffixRepair must be **hang-free**: journal FF + RewindTo like SoftWait wake (`try_arm_park_resume_at_k`); absolute jump / valued CallOutcome stay research-only until hang-free proof.  
4. Metrics of success: wall / `evm_entries` / `force_bind_reabort` / Absolute TPS — **not** “look more like OCC.”

---

## 4. Detection & avoidance roles (supporting)

- Detect: surface first conflicting `k` and producer readiness — feed Bind/Await/SuffixRepair.  
- Avoid: Await when EV says so; do not storm SoftWait.  
- **Resolve owns the win condition.**

---

## 5. Implementation mandate (大刀阔斧)

1. Replace Lean `apply_lean_abort_repair` “clear RewindTo + head restart” with **SuffixRepair-first** (same hang-free arm as park resume).  
2. π: when writer known unfinished, **Await/Bind compete as first-class**; drop “ties → SpecRead (OCC-like)” as protocol identity — ties → Prefer Await if producer Running, else SpecRead.  
3. Validation path: pass first-fail `k` into SuffixRepair; stop treating every fail as OCC FullRestart.  
4. Keep seq≡par TCB. SoftWait scarce OK if Bind/SuffixRepair work.  
5. Do not graduate `SPECFENCE_ENABLE_INSPECT` absolute jump.

