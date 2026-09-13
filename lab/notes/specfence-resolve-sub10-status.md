# SpecFence resolve — sub-10 wall push status

**Date:** 2026-09-07 (Asia/Shanghai)  
**Branch:** `specfence`  
**Base tip:** `83412fe`  
**Authority:** `specfence-resolve-mw-strip-status.md`

## Mandate

Push 597 wall median **below 10ms** (stretch <8). SoftWait ≪428; SpecFence-native SuffixRepair on conflict; tests green; no hang.

---

## Re-profile residual (83412fe)

| Bucket | ~ms (CPU sum) | Note |
|--------|-------------:|------|
| maybe_wait | **~41** | rem journal + Bind `is_done` park |
| validate + SuffixRepair | **~55** | Instant-on profile |
| BO park idle | **~19** | Bind unfinished `!is_done` |
| SoftWait Soft idle | **0** | already scarce |
| Wall median | **~14.1** | vs OCC ~3.6–4.0 |

Root of residual gap vs OCC: **Bind `!is_done` BlockingOther park** + rem lock tax + validate meta — not SoftWait Soft.

---

## Structural cuts (landed)

1. **Bind-on-Data without `is_done` park** — OCC-like consume of published MV Data; writer abort → ESTIMATE → SuffixRepair/RebindOnly (no SoftWait Soft).
2. **WaitHard → BlockingOther steal** — repair Await no longer arms FenceGraph SoftWait Soft (keeps SoftWait 0 under Bind-no-park abort pressure).
3. **rem `UnsafeCell<PartialRetryState>`** — single-executor invariant; drop per-access Mutex; `note_access_certified_checkpoint` one-lock Bind journal+certify+lite EffectBoundary.
4. **Lock-free `done_flags`** for `scheduler.is_done` (prior/BO paths still use it).
5. **Skip `has_force_bind` DashMap on incarnation 0**; validate `checkpoint_opportunity` O(1).

Tried / not landed as default: long spin/yield-before-park (CPU/schedule noise); SpecRead journal skip (FullRestart wall↑); lean abort Bayes/HotSet strip (no wall win).

---

## Validation

| Check | Result |
|-------|--------|
| `cargo test -p pevm --lib` | **92 passed** |
| `cargo test -p pevm --test specfence` | **23 passed**, 13 ignored |
| SoftWait 597 median | **0** ≪428 |
| Hang | **No** |
| seq≡par | green |

### 597 @8 vs `83412fe`

| Metric | 83412fe N=7 / N=11 | **this N=7** | **this N=11** |
|--------|-------------------:|-------------:|--------------:|
| wall median | **14.1** / **14.7** | **14.0** | **13.7** |
| wall p90 | 15.4 / 17.9 | **14.7** | **14.2** |
| wall min | 13.1 / 12.9 | **13.2** | **12.8** |
| SoftWait med | 0 / 0 | **0** | **0** |
| OCC median | ~3.6 / ~4.0 | ~3.7 | ~3.8 |
| BO idle (profile) | ~19 | **~1.6** | — |
| maybe_wait (profile) | ~41 | **~30** | — |

**PARTIAL:** SoftWait 0; BO idle collapsed; maybe_wait down; wall N=11 **materially below tip** (13.7 vs 14.7); stretch **<15 met**; **<10 not met**. Residual ~4× vs OCC is validate/SuffixRepair + rem/EVM tax on critical path after park kill.

---

## Artifacts

- `lab/results/resolve-sub10-597.json` — diagnosis summary
- `lab/results/resolve-sub10-sf-occ.json` / `*-smoke7.run.log` / `*-smoke11.run.log`
- `lab/results/resolve-sub10-prof-sf-occ.json` / `*-prof-smoke7.run.log`

## Code

- `crates/pevm/src/vm.rs` — Bind-no-park; WaitHard→BO; rem journal coalesce
- `crates/pevm/src/specfence/rem.rs` — UnsafeCell rem; `note_access_certified_checkpoint`
- `crates/pevm/src/scheduler.rs` — `done_flags` lock-free `is_done`
- `crates/pevm/src/pevm.rs` — validate opportunity O(1)
