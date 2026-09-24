# SpecFence V5 — bottleneck dig plan (post-P3)

**Date:** 2026-09-07 (Asia/Shanghai)  
**Trigger:** V5-P3 no-graduate (inspect hangs; plant stays research).  
**Iron law:** only win by cutting **interpreter-seconds on the critical path** or **false-wait idle** — not SoftWait count, not labels.

Target block for dig: **14689597** @8 cores (also spot-check 599/097/598).  
P2/P3 Lean ref: SoftWait ~20–50, SF/OCC ~0.09–0.17 (noisy); OCC is the makespan baseline.

---

## 1. First-principles questions

### Q1 — Where are interpreter-seconds spent on 597?

Decompose SpecFence wall / useful work into:

| Bucket | Meaning | How to see |
|--------|---------|------------|
| First-run EVM | Successful first incarnation | `evm_entries` − reexec-ish counters |
| Abort reexec EVM | Failed incarnation → head restart | `occ_aborts`, `full_restart`, Lean ForceBind still re-enters interpreter |
| Research rewind credit | Mid-tx resume (opt-in only) | `rewind_to_cp`, `resume_count`, `journal_ff_*` (0 on Lean abort today) |
| Wait idle | SoftWait / Blocking park | SoftWait arms, `park_resume_*`, wake latency EMAs |
| Meta / CC tax | Bayes/HotSet/Fence on hot path | `meta_tax_ratio`, bind vs spec counts |

**Hypothesis (from bottleneck note R1):** Lean PartialRetry is “partial” in MV/Bind labels, not in CPU — abort ≈ full tx EVM again. Dig must **prove** fraction of SF makespan that is abort-reexec vs first-run vs idle.

### Q2 — SoftWait: useful vs harmful?

- Useful: waiter would have aborted with high P; producer finishes soon; wake → progress without storm.
- Harmful: artificial chain lengthens \(T_{\mathrm{crit}}\); cores idle while other ready txs exist.

Dig: correlate SoftWait arm→wake latency (`note_wait_latency`) with whether subsequent validation succeeds without abort; compare SF makespan with SoftWait forced-off (SpecRead-only) vs default on 597 **without** restoring WaitHard ladders.

### Q3 — Bind hit rate vs abort after force-bind?

- `bind_hits` vs `spec_read_count` vs `occ_aborts` on Lean.
- After `apply_lean_abort_repair` ForceBind, does the next incarnation abort again on the same locations?
- Gap: force-bind without mid-tx skip still pays full interpreter — Bind quality alone cannot beat OCC if reexec count ≈ OCC aborts.

### Q4 — Gap vs OCC makespan decomposition

\[
\mathrm{SF/OCC} = \frac{T_{\mathrm{OCC}}}{T_{\mathrm{SF}}}
\approx \frac{T_{\mathrm{crit}}+T_{\mathrm{waste}}^{\mathrm{OCC}}}{T_{\mathrm{crit}}+T_{\mathrm{waste}}^{\mathrm{SF}}+T_{\mathrm{meta}}}
\]

Need: same-block OCC vs SF wall; abort counts both sides; estimate \(T_{\mathrm{meta}}\) from meta_ops/useful; estimate idle from SoftWait latency × arms (upper bound).

---

## 2. Metrics already present vs need adding

### Already exist (`SpecFenceMetrics` / learner / smoke)

| Metric | Use |
|--------|-----|
| `evm_entries` | Interpreter session starts |
| `occ_aborts`, `full_restart`, `partial_retry_count`, `rebind_only` | Abort taxonomy |
| `rewind_to_cp`, `resume_count`, `journal_ff_*`, `pc_resume_*` | Plant / FF (research) |
| `park_resume_at_k`, `park_resume_full_retry` | SoftWait wake resume class |
| `soft_wait_arms`, `wait_hard_count` | Wait volume |
| `bind_hits`, `spec_read_count` | π mix |
| `lean_mode_txs` | Engagement path |
| LiveLearner `E_wait_time`, `E_cascade`, `E_reexec`, `E_idle_steal`, `meta_tax` | EV θ |

G7 smoke (V5-P3) now dumps dig hooks into `g7-sf-occ-smoke.json` rows.

### Likely need adding (dig phase — not P3)

| Gap | Proposal |
|-----|----------|
| Abort-reexec vs first success EVM time | Per-tx or block ns: `evm_ns_first` / `evm_ns_reexec` (Instant around `Vm::execute`) |
| Force-bind then re-abort | Counter `force_bind_reabort` when abort with force_prefix armed |
| SoftWait useful | Counter wake→validate-ok without abort vs wake→reabort |
| OCC-comparable waste | Export OCC abort + execute incarnation counts in same smoke row |
| Crit-path proxy | Optional: commit-index progress timestamps / steals while SoftWait armed |

Do **not** add Wait ladders or default inspect to “fix” the dig.

---

## 3. Dig protocol (next phase)

1. **Baseline dump:** one quiet Lean G7 smoke; record dig JSON fields for 597 OCC + SF.  
2. **Time split:** add lightweight `evm_ns_*` if wall variance dominates counter noise.  
3. **SoftWait A/B:** Lean default vs SoftWait disabled (π Spec-only) — SoftWait must stay ≪428 if re-enabled.  
4. **Bind quality:** histogram aborts after ForceBind; identify hot locations.  
5. **Write-up:** `lab/notes/specfence-v5-bottleneck-dig-status.md` with makespan decomposition table.  
6. **Only then** revisit hang-free mid-tx resume (lite EffectBoundary / journal-FF abort arm) if dig shows abort-reexec dominates **and** a Lean-safe subset can cut interpreter-seconds without inspect.

---

## 4. Non-goals

- Graduating inspect/jump without hang-free 597 proof.  
- Raising SF/OCC by WaitHard storms / fanout→Wait.  
- Renaming modules before dig evidence (v5 shovel rename is secondary).

## Related

- `lab/notes/specfence-v5-p3-plant-status.md`  
- `lab/notes/specfence-v5-p2-theta-status.md`  
- `lab/notes/specfence-first-principles-bottleneck.md`  
- `lab/results/v5-p3-ab-summary.json`
