# SpecFence from CC first principles: detect / avoid / resolve

**Date:** 2026-09-07 (Asia/Shanghai)  
**Evidence:** `lab/notes/specfence-v5-bottleneck-dig-status.md`, 597 @8 Lean vs SoftWait-off  
**Question:** 没有检测到冲突，还是没有避免冲突，还是没有解决冲突？

---

## 0. Definitions (classical CC)

On a fixed commit order \(T_0,\ldots,T_{n-1}\) and locations \(\ell\):

| Stage | Meaning in SpecFence / Block-STM |
|-------|----------------------------------|
| **Detect** | Discover that consumer access \(a_c\) conflicts with a producer on \(\ell\) (RAW/WAW/…): MV miss, ESTIMATE, validation fail, Bayes/HotSet hint, effect-edge. |
| **Avoid** | Prevent concurrent conflicting work *before* waste: SoftWait / WaitHard / EarlyAbort park / Bind-to-known-version / schedule after writer. |
| **Resolve** | After conflict is live (dirty read, abort, ESTIMATE): repair serializability — abort+reexec, ForceBind prefix, selective invalidate, RewindTo, ESTIMATE cascade. |

OCC (PEVM Block-STM): **weak avoid**, **late detect** (validate), **resolve = abort+reexec+ESTIMATE**.  
SpecFence thesis: **earlier detect + selective avoid + finer resolve** should beat OCC makespan.

Iron law: only stages that cut **interpreter-seconds on \(T_{\mathrm{crit}}\)** or **false idle** win.

---

## 1. Verdict (597 Lean @8)

| Stage | Status | One-line |
|-------|--------|----------|
| **Detect** | Not the hole; if anything **over-eager / wrong-grain** | SF validation aborts **378 vs OCC 80** (~4.7×). Conflicts are found; more incarnations than OCC. |
| **Avoid** | **Under-used and not the SF/OCC hole** | SoftWait **41 ≪ 428**; SoftWait-off → SF/OCC **0.137** (worse/flat vs **0.153**). Missing Wait is not why we lose. |
| **Resolve** | **Primary failure** | Lean abort = ForceBind + **head** FullRestart; `resume_count=0`; **~49%** `force_bind_reabort`. Labels ≠ cheaper repair. |

**Bottom line:** SpecFence is not “blind.” It is not mainly “forgot to Wait.” It **detects (and aborts) more**, **avoids little**, and **resolves at OCC’s cost grain while paying more aborts + meta** — so makespan loses.

---

## 2. Detect — what the numbers say

- seq≡par TCB still holds → silent missed conflicts that corrupt commit are not the story.
- SF aborts ≫ OCC on 597 → discovery/admission creates **more** fail events, not fewer.
- Bind/SpecRead mix (~18% Bind) + SoftWait scarce → most paths still **speculate** (OCC-like detect-at-validate), plus extra EarlyAbort/Blocking parks (`wait_park_count` ≫ steals).
- Hypothesis: “detect” is often **wrong time / wrong action trigger** (cut incarnation early, ESTIMATE noise) rather than “never saw RAW.”

So: **not undetected conflict**; **detection does not reduce first-pass wrong work enough**, and sometimes **amplifies** abort storms vs lean OCC.

---

## 3. Avoid — what the numbers say

- Classical avoidance = serialize conflicting readers behind writers (locks / SoftWait).
- Dig SoftWait A/B: turning avoidance **off** does **not** close the gap; SoftWait wake useful rate only ~37% (13 ok / 22 reabort).
- Historical G7 Wait storms (SoftWait 428) raised avoidance and still lost — avoidance-without-cheap-resume lengthens \(T_{\mathrm{crit}}\).

So: **failure mode is not “we failed to detect so we failed to avoid.”** Avoidance is scarce by design (AEC); the remaining gap is elsewhere. Extra EarlyAbort park idle is **harmful micro-avoidance**, secondary.

---

## 4. Resolve — why this is the main CC failure

When conflict is already known (validation abort / EarlyAbort):

| Intended finer resolve | Lean reality |
|------------------------|--------------|
| PartialRetry | Count ≈ aborts; still **full interpreter** from tx head |
| ForceBind certified prefix | MV/π labeling; **`force_bind_reabort ≈ 49%`** |
| RewindTo / journal FF | SoftWait-wake only; **abort path clears RewindTo** (`resume_count=0`) |
| Inspect mid-tx plant | V5-P3: **hangs** — must not graduate |

OCC resolve: abort + reexec, ~80 aborts, ~922 reexec entries, wall ~5.5 ms.  
SF resolve: ~378 aborts, ~1852 reexec entries, wall ~36 ms — **same mechanical family as OCC, worse multiplicity**, plus meta on every location read.

**Resolve failure = cost grain ≠ concurrency grain.** Region/access detect is fine-grained; repair is still **tx-head EVM**. That is the CC redesign target.

Nosoft: SF `evm_entries` can be **&lt;** OCC yet wall still ~7× → resolve/meta path tax compounds even when entry counts look OK.

---

## 5. Reframe the product question

Wrong question: “Did we miss conflicts?”  
Right questions:

1. **Detect→action map:** On a known/probable conflict, should we Spec / Wait / EarlyAbort / Bind? (AEC tries; still too many aborts vs OCC.)
2. **Resolve cost:** When we abort, can repair cost \(\ll\) full tx EVM **hang-free**? (Today: no on Lean abort.)
3. **Avoid budget:** Wait only when EV says wake ≪ reexec; SoftWait already scarce — do not storm.

---

## 6. Implications (no Wait ladders / no inspect graduate)

1. Treat **resolve** as the first-class redesign: Lean-safe mid-tx resume **only** where ForceBind-reabort clusters and hang-free proof exists; otherwise match OCC’s cheap abort path and **stop paying meta**.
2. Treat **detect** as calibration of **when to cut**, not “add more sensors.” Goal: SF abort count → toward OCC, not above.
3. Treat **avoid** as already correctly scarce; fix **park→steal** (false idle), not SoftWait volume.

