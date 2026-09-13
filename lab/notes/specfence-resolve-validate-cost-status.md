# SpecFence resolve — validate / SuffixRepair cost status

**Date:** 2026-09-07 (Asia/Shanghai)  
**Branch:** `specfence`  
**Base tip:** `1dddbce`  
**Authority:** `specfence-resolve-sub10-status.md`, `specfence-native-resolve-protocol.md`

## Mandate

Cut validate / SuffixRepair residual after park kills. Goal: median wall &lt;10ms on 597 N≥7; SoftWait ~0; tests green.

---

## Profile: what dominates validate (~40ms @ 1dddbce)

| Bucket | Role |
|--------|------|
| Read-set walk (`collect_invalid_reads` / `origin_still_valid`) | Baseline OCC cost (~3–4ms CPU on OCC) |
| Fail path: `plan_partial_retry` + `apply_suffix_repair` + `invalidate_partial_suffix` | **Dominant** SpecFence tax (~186–220 SuffixRepair resumes) |
| Fail path: bayes / hotset / promote / learner per invalid ℓ | Meta on every abort |
| Success path: O(\|reads\|) bayes success + checkpoint atomics | Was redundant vs abort learning under Bind-no-park |
| RebindOnly | Rare (0–3): `true_suffix` + Estimate window blocks value-stable patch |
| MvMemory `last_locations` locks | Double-fetch read/write sets + re-plan on abort |

**Not** SoftWait Soft (0) or BO idle (~1–2). Residual wall ~4× OCC is SuffixRepair/reexec + validate meta after park kill.

Tried / not landed: Estimate validation-deferral (requeue while ODR-stable under ESTIMATE) → wall↑ / churn; multi-origin Basic snap + RebindOnly collapse → regress; lean-first strip of hotset/co_access → abort storm.

---

## Cuts landed

1. **Fail-path plan reuse** — one `plan_partial_retry` + cached write/read sets → `apply_suffix_repair_planned` (no double plan / re-lock).
2. **`value_stable_match`** — compare snap in place (no `FfValue` clone) for RebindOnly gate.
3. **`try_rebind`** — verify origins before mutate; skip full `collect_invalid_reads` re-walk (still refuses multi-origin / Estimate).
4. **Success-path validate strip** — O(1) checkpoint opportunity; **Wait revoke only** (drop O(\|reads\|) bayes success storm; abort-path bayes still learns).
5. Defer `read_locations` until fail/success need (no eager fetch before invalid walk).

---

## Validation

| Check | Result |
|-------|--------|
| `cargo test -p pevm --lib` | **92 passed** |
| `cargo test -p pevm --test specfence` | **23 passed**, 13 ignored |
| SoftWait 597 median | **0** |
| Hang | **No** |
| Abort storm | **No** (aborts ~215–236 med) |
| seq≡par | green |

### 597 @8 vs `1dddbce`

| Metric | 1dddbce N=7 / N=11 | **this N=7** | **this N=11** |
|--------|-------------------:|-------------:|--------------:|
| wall median | **14.0** / **13.7** | **13.6** | **15.6** |
| wall p90 | 14.7 / 14.2 | **14.5** | **20.3** |
| wall min | 13.2 / 12.8 | **12.8** | **13.3** |
| SoftWait med | 0 / 0 | **0** | **0** |
| OCC median | ~3.7 / ~3.8 | ~3.5 | ~4.1 |
| validate (profile ms) | **~40** | **~34.5** | — |
| maybe_wait (profile) | ~30.5 | **~23.7** | — |
| rebind_only (last) | 3 | **1** | **0** |
| resume / rewind_to_cp | ~186 | **~176** | **~174** |

**PARTIAL:** validate CPU ↓ (~40→34.5); SoftWait 0; N=7 wall **13.6 vs 14.0** (small↓); N=11 noisy / not improved; RebindOnly still rare; stretch **&lt;10 not met**. Residual remains SuffixRepair/reexec (~176 resumes) vs OCC FullRestart grain.

---

## Artifacts

- `lab/results/resolve-validate-597.json` — diagnosis summary
- `lab/results/resolve-validate-sf-occ.json` / `*-smoke7.run.log`
- `lab/results/resolve-validate11-sf-occ.json` / `*-smoke11.run.log`
- `lab/results/resolve-validate-prof-sf-occ.json` / `*-prof-smoke7.run.log`

## Code

- `crates/pevm/src/pevm.rs` — plan cache; success Wait-revoke-only; `value_stable_match`
- `crates/pevm/src/mv_memory.rs` — faster `try_rebind` (verify-before-apply)
- `crates/pevm/src/specfence/rem.rs` — `value_stable_match`; `apply_suffix_repair_planned`
