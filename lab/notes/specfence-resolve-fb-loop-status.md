# SpecFence resolve — force_bind_reabort ≈ resume loop break

**Date:** 2026-09-07 (Asia/Shanghai)  
**Branch:** `specfence`  
**Base tip:** `12990b8`  
**Authority:** `specfence-resolve-fewer-resumes-status.md`, `specfence-native-resolve-protocol.md`

## Mandate

Break `force_bind_reabort ≈ resume` identity on 597. Escalate resolve on reabort; SoftWait Soft ~0; median wall toward <10ms; tests green; no hang / SoftWait Soft storm.

---

## Diagnosis (597 @ 12990b8)

| Signal (last-iter) | Value | Meaning |
|--------------------|------:|---------|
| resume_count / rewind_to_cp | **~160** | Every Lean fail → SuffixRepair RewindTo |
| force_bind_reabort | **~154** | Almost every repair re-aborts with force_bind still armed |
| SoftWait | **0** | Not Soft storms |
| Fail grain | Bind unfinished Data → ESTIMATE → SuffixRepair+fb → Bind again | **force_bind_reabort loop** |

Bind-no-park + ForceBind prefix did not prevent the next incarnation from failing again.

---

## Cuts landed (evidence-backed combo)

1. **Escalate after 1 `force_bind_reabort`:** clear force_bind + RewindTo; OCC-style **FullRestart** (no protect of prior fb prefix Data). Drop sticky force_bind extend on escalate.
2. **Cap SuffixRepair depth ≥2** → same escalate (belt-and-suspenders).
3. **Before Bind under force_prefix:** if published Data writer not Executed/Validated → spin(64) then **BlockingOther prefer-steal Await** (not SoftWait Soft) → Bind when done. No-Data ESTIMATE path keeps WaitHard→BO after spin.
4. **Keep RebindOnly** when it can apply; SoftWait Soft not restored.

Tried / not landed: protect prior fb Data on escalate (wall↑); longer spin-256 without BO-on-Data (noisy wall↑).

---

## Validation

| Check | Result |
|-------|--------|
| `cargo test -p pevm --lib` | **93 passed** |
| `cargo test -p pevm --test specfence` | **23 passed**, 13 ignored |
| SoftWait 597 median | **0** |
| Hang | **No** |
| seq≡par | green |

### 597 @8 vs `12990b8`

| Metric | 12990b8 N=7 / N=11 | **this N=7 best (t2)** | **this N=7 trip** | **this N=11** |
|--------|-------------------:|----------------------:|------------------:|--------------:|
| wall median | **13.4** / **13.7** | **12.8** | 13.9 / 12.8 / 13.2 | **13.2** |
| wall p90 | 13.6 / 24.9 | **15.1** | | **14.4** |
| wall min | 12.7 / 12.4 | **12.2** | | **12.3** |
| SoftWait med | 0 / 0 | **0** | **0** | **0** |
| OCC median | ~3.7 / ~3.4 | ~3.6 | | |
| resume (last) | **~160** / **~149** | **88** | 99 / 88 / 87 | **102** |
| force_bind_reabort (last) | **~154** / **~145** | **97** | 110 / 97 / 95 | **105** |
| full_restart (last) | low | **97** (=fb escalate) | | **105** |
| rebind_only (last) | 0–4 | **0** | | **0** |

**PARTIAL SUCCESS:** resume / fb_reabort down ~40% vs tip; SoftWait 0; wall flat–slight↓ (best 12.8); **loop broken** — each fb_reabort maps to FullRestart once (`full_restart ≈ fb_reabort`), not another SuffixRepair. Numerical resume≈fb remains for double-fail txs (1 repair + 1 escalate) but identity of infinite SuffixRepair reabort is gone. Stretch **<10 not met**.

---

## Artifacts

- `lab/results/resolve-fb-loop-597.json` — diagnosis summary
- `lab/results/resolve-fb-loop-sf-occ.json` / `*-smoke7.run.log` (t2 best)
- `lab/results/resolve-fb-loop-t{1,2,3}-sf-occ.json` — triplicate
- `lab/results/resolve-fb-loop11-sf-occ.json` / `*-smoke11.run.log`
- `lab/results/resolve-fb-loop-flip.json`

## Code

- `crates/pevm/src/pevm.rs` — escalate on was_force_bind / depth≥2; clear repair depth on success
- `crates/pevm/src/specfence/rem.rs` — `suffix_repair_depth`, `escalate_full_restart`
- `crates/pevm/src/vm.rs` — force_prefix unfinished Data → spin + BO Await then Bind
