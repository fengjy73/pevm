# SpecFence resolve — fewer SuffixRepair resumes status

**Date:** 2026-09-07 (Asia/Shanghai)  
**Branch:** `specfence`  
**Base tip:** `c1c2d20`  
**Authority:** `specfence-resolve-validate-cost-status.md`, `specfence-native-resolve-protocol.md`

## Mandate

Cut ~176 SuffixRepair resumes (not faster plan). More RebindOnly / in-place resolve; break force_bind_reabort loops; SoftWait ~0; stretch median wall &lt;10ms; tests green.

---

## Diagnosis (597 @ c1c2d20)

| Signal (last-iter) | Value | Meaning |
|--------------------|------:|---------|
| resume_count / rewind_to_cp | **176** | Every Lean fail → SuffixRepair RewindTo |
| force_bind_reabort | **175** | Almost every repair re-aborts with force_bind still armed |
| rebind_only | **0–1** | In-place resolve almost never wins |
| SoftWait | **0** | Not Soft storms |
| true_suffix | dominant | Fan-out txs write after first bad read → RebindOnly gated |
| value_stable | rare | Real value changes; Lazy Basic cannot safely match (seq≢par when tried) |
| Estimate @ validate | common | `try_rebind` refuses Estimate → SuffixRepair |

**Fail grain:** Bind-on-Data (no `is_done`) → writer abort/ESTIMATE → consumer validate fail → SuffixRepair + force_bind → reexec Bind unfinished Data → **force_bind_reabort loop**. RebindOnly blocked by true_suffix + non-stable / Estimate / multi-origin (lazy).

**Tried / not landed:** BO park on unfinished published Data (sticky/force) — cut some reaborts but **BO idle regressed wall**; optimistic Lazy value_stable — **broke seq≡par**; incarnation-0 ESTIMATE→BO — wall↑.

---

## Cuts landed

1. **Yield-spin before Bind** on unfinished Data for `force_prefix` / sticky / hot_inc0+prior (no SoftWait Soft; no BO park on published Data).
2. **force_prefix + ESTIMATE:** spin for Data/done → Bind if Data appears; else WaitHard→BO if still unfinished (not SpecRead-through-ESTIMATE).
3. **Sticky Await** allowed under force_prefix when no Data (break SpecRead→reabort).
4. **Value-stable widen:** snap match **or** `prior_read_value_stable` (prior MvMemory Basic/Storage == current); value-stable path **allows multi-origin → single Data** rebind.
5. Keep Bind-on-Data common path; SoftWait Soft not restored.

---

## Validation

| Check | Result |
|-------|--------|
| `cargo test -p pevm --lib` | **92 passed** |
| `cargo test -p pevm --test specfence` | **23 passed**, 13 ignored |
| SoftWait 597 median | **0** |
| Hang | **No** |
| seq≡par | green |

### 597 @8 vs `c1c2d20`

| Metric | c1c2d20 N=7 / N=11 | **this N=7** | **this N=11** |
|--------|-------------------:|-------------:|--------------:|
| wall median | **13.6** / **15.6** | **13.4** | **13.7** |
| wall p90 | 14.5 / 20.3 | **13.6** | **24.9** |
| wall min | 12.8 / 13.3 | **12.7** | **12.4** |
| SoftWait med | 0 / 0 | **0** | **0** |
| OCC median | ~3.5 / ~4.1 | ~3.7 | ~3.4 |
| resume (last) | **~176** | **~160** | **~149** |
| rewind_to_cp (last) | ~176 | **~160** | **~148** |
| force_bind_reabort (last) | ~175 | **~154** | **~145** |
| rebind_only (last) | 0–1 | **0** | **4** |

**PARTIAL:** resume / fb_reabort down ~10–15% vs tip; SoftWait 0; N=7 wall flat/slight↓; N=11 wall **15.6→13.7**; RebindOnly still scarce on 597 (true_suffix + real value deltas); stretch **&lt;10 not met**. Residual remains SuffixRepair/reexec under Bind-no-park abort cascades — BO-on-Data parks trade wall for resumes.

---

## Artifacts

- `lab/results/resolve-fewer-resumes-597.json` — diagnosis summary
- `lab/results/resolve-fewer-resumes-sf-occ.json` / `*-smoke7.run.log`
- `lab/results/resolve-fewer-resumes11-sf-occ.json` / `*-smoke11.run.log`
- `lab/results/resolve-fewer-resumes-flip.json`

## Code

- `crates/pevm/src/vm.rs` — spin-before-Bind; force_prefix ESTIMATE spin→Bind/BO; sticky Await w/ force_prefix
- `crates/pevm/src/pevm.rs` — estimate_cleared + prior_read value_stable; value-stable multi-origin rebind
- `crates/pevm/src/mv_memory.rs` — `prior_read_value_stable`; `try_rebind_invalid_reads_value_stable`
