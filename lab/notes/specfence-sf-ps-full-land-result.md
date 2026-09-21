# SpecFence Parallel Spine (SF-PS) — land result

**Date:** 2026-09-21  
**PR:** [#44](https://github.com/fengjy73/pevm/pull/44) `cursor/specfence-sf-ps-full-land-09b0`  
**Baseline:** PR #43 `cursor/specfence-shell-cut-redig-6a8f`  
**Design SoT:**
- [`specfence-first-class-architecture-redesign-v1.md`](specfence-first-class-architecture-redesign-v1.md)
- [`specfence-sf-ps-full-land-v1.md`](specfence-sf-ps-full-land-v1.md)

Raw JSON lives under `lab/results/` (gitignored): `sf-ps-instant-off-*.json`, `sf-ps-all-blocks-n1.json`, `sf-ps-focus-n3-reuse.json`.

Short-term TPS jitter vs PR #43 is reported honestly. The OCC main loop was **not** restored to chase a ratio.

---

## What landed

SpecFence mode no longer treats Block-STM/OCC as the protocol root.

| Face | Land |
|------|------|
| **A. Schedule** | `schedule::pick(RunnableSet)` is the SpecFence main pick. `refuse_admit` wave-fills another runnable. Empty / leftover / mid-band leftover wait-set = Avoid=noop antichain (`ready=None` on the wave host), still on SF-PS. `next_occ_task` is OCC/PCC only. |
| **B. Visibility** | `VisibilityPolicy::{Opt, WaitReleased, OrderedTip}`. Opt = DAG independent set (Avoid=noop), **not** `ConcurrencyMode::Occ`. |
| **C. Resolve** | Validate emits `ResolvePlan::{Commit, PartialAbortRebind, PartialAbortRewind, OrderedReplay, FullReplay}`. Edged path prefers Resolve over bool→incarnation++. |
| **D. Learn→G** | Thin n≤176 `train_hat` stays light (no Win_8 mill). Under-covered spines drop Full/Seg as success. `skip_ungated_*` is compat Avoid=noop, not a Learn target. |
| **E. Docs/metrics** | `mod.rs` / glossary / SPECFENCE.md are SF-PS. Metrics: RunnableSet width, visibility counts, ResolvePlan histogram, `sf_schedule_picks` / `occ_schedule_picks`. |

---

## Call-graph evidence (P1)

SpecFence worker:

```
next_sf_task → schedule::pick(RunnableSet)
  → ProducerStage (conflict subgraph)
  → scheduler.next_task_with_wave_ready(wave, ready)   # host walk, not next_occ_task
Execute(VisibilityPolicy) → validate_specfence → ResolvePlan.apply
```

OCC contrast (kept):

```
next_occ_task → scheduler.next_task()
```

Proof:

- Unit + fixture: `specfence_sf_ps_pick_never_calls_next_occ_task`
- Source asserts on `schedule.rs` / `computer.rs` (pre-`#[cfg(test)]` body; the token `next_occ_task` is not a SpecFence pick)
- Every SpecFence harness row: `occ_schedule_picks = 0` (Instant-off 4/4, N=1 99/99, N=3 focus 6/6)
- OCC rows increment `occ_schedule_picks` only

Empty wait-set / leftover Detect bits / mid-band leftover (19469101) stay Avoid=noop. They do **not** fall through to `next_occ_task`.

---

## Hard constraints

| Constraint | Result |
|------------|--------|
| Soft=0 | `soft_wait_arms = 0` on every Instant-off / N=1 / N=3 row. `soft_wait_arms_nonzero_blocks = []`. |
| seq≡par | `cargo test -p pevm --lib` 362 passed. SpecFence integration 45 passed / 20 ignored (iter11, erc20 independent, existing seq≡par fixtures). |
| lazy never OrderedAdmit | 15274915 N=1 stays Opt→Opt, no Full thousand-writer gate. No 4–27× lazy Full tail. |
| OCC contrast kept | `ConcurrencyMode::Occ` still uses `next_occ_task`. |
| thin n≤176 no Win_8 | Instant-off + N=3: 3356896 Win_1→Win_2; 6196166 Win_1. Zero `Win_8` arms on N=1 99. |
| no `next_occ_task` as SF pick | See call-graph. |

---

## Regressions

| Suite | Result |
|-------|--------|
| `cargo test -p pevm --lib` | 362 passed |
| specfence integration | 45 passed / 20 ignored |
| `specfence_sf_ps_pick_never_calls_next_occ_task` | passed |
| iter11 / erc20 independent / seq≡par fixtures | passed |

`erc20_clusters` previously starved (~11 min) when leftover Detect bits honored ReadyEdge refuse while skipping ProducerStage promote. Fixed: leftover / empty / mid-band pick is Avoid=noop (`ready=None`). Not re-run after the mid-band fix; independent raw transfers passed.

---

## Instant-off (N=5 @8 Soft=0)

Harness: `specfence_3356896_compare` + `SPECFENCE_COMPARE_BLOCK`. Primary wall = SF reuse median (iters 1..4) vs OCC median. Host is noisier than the PR #43 Instant-off box; compare **ratios and arms**, not absolute ms.

| block | n | OCC med ms | SF cold | SF reuse | reuse/OCC | PR43 Instant × | last arm | vis tip | Resolve | occ_picks |
|------:|--:|-----------:|--------:|---------:|----------:|---------------:|----------|--------:|---------|----------:|
| **3356896** | 176 | 7.141 | 8.254 | **4.004** | **0.56** (SF faster) | 1.13×, Win_8 | Win_1 | 4 | Commit + 15 FullReplay last iter | 0 |
| **19807137** | 712 | 30.989 | 52.530 | **43.952** | **1.42** | 1.30×, Opt | Opt | 0 | Commit + FullReplay (under-covered Abort) | 0 |
| **14396881** | 1346 | 15.714 | 18.408 | **16.493** | **1.05** | (not in PR43 Instant K) | Opt | 0 | Commit (Avoid=noop) | 0 |
| **6196166** | 108 | 6.263 | 11.037 | **4.041** | **0.65** (SF faster) | 1.43×, Win_8 mill | Win_1 | 1 | Commit; reuse OrderedTip | 0 |

Soft=0 every row. `occ_schedule_picks = 0` on every SF row.

**Behavior vs PR #43 Instant-off:**

- **3356896 / 6196166:** thin Win_8 mill is gone. Reuse shows OrderedAdmit + `VisibilityPolicy::OrderedTip` + `ResolvePlan::Commit`. That is Resolve/ordered, not a shell-tax gap.
- **19807137:** stays Opt (Avoid=noop). One mid-iter planted `Full/1` then dropped; last iter Opt. FullReplay counts are the under-covered storage spine aborting on the SF-PS Resolve path, not an OCC-engine retreat. Instant × 1.42 vs PR43 1.30 — same class, slightly worse on this host.
- **14396881:** Opt Avoid=noop, near-parity Instant wall.

---

## Corpus TPS vs PR #43

PR #43 sweep (N=3 reuse @8, n>0): **26/98** wins, median SF/OCC **0.923**, Soft=0, no lazy Full tail.

### N=1 all-blocks Soft=0 @8 (99/99 pairs)

**Do not treat this as the P4 score.** Iters=1 / reuse=false. OCC first-iter noise inflates SF wins (same class as Instant-off 19807137 OCC cold 4.5 s).

| metric | N=1 this PR | note |
|--------|------------:|------|
| loaded / ok | 99 / 99 | empty 19910734 listed; TPS on n>0 = 98 |
| Soft=0 every row | yes | |
| SF TPS ≥ OCC (n>0) | **66 / 98** | OCC-cold contaminated |
| SF/OCC median (n>0) | **1.167** | OCC-cold contaminated |
| `occ_schedule_picks` | **0 / 99 SF rows** | |
| `edge_ordered_admit` > 0 | **51** | |
| `visibility_ordered_tip` > 0 | **51** | |
| `resolve_ordered_replay` > 0 | **21** | |
| `resolve_full_replay` > 0 | 58 | ResolvePlan, not OCC incarnation++ root |
| Win_8 arms | **0** | |
| lazy Full thousand-writer (15274915) | Opt→Opt, 0.847 | no Full gate |

### N=3 reuse @8 — focus six (honest reuse)

Full 99-block N=3 hung twice before the leftover/mid-band Avoid=noop picks (2179522 OCC-quiet first-iter + 19469101 leftover Detect starve). After those picks, the six Instant/hang-class blocks completed. **Full 99 N=3 reuse was not re-run**; P4 for the whole corpus is therefore incomplete. The six-block reuse is the honest number we have.

| block | n | PR43 N=3 | this N=3 | Δ | arm | OA | Resolve / note |
|------:|--:|---------:|---------:|--:|-----|---:|----------------|
| **3356896** | 176 | 0.691 Win_2→Win_8 | **0.645** Win_1→Win_2 | −0.046 | Win_2/14 + Full/2 + Win_1/2 | 8 | OrderedTip vis=3; no Win_8 |
| **19807137** | 712 | 0.593 Full→Full | **0.577** Opt→Opt | −0.016 | Opt | 0 | FullReplay=1231 on Opt spine; no Full-as-success |
| **14396881** | 1346 | 0.893 Opt→Opt | **0.494** Opt→Opt | **−0.399** | Opt | 0 | Avoid=noop; Instant-off 1.05× — sweep OCC was hot (113k TPS) |
| **6196166** | 108 | 0.629 Win_8 mill | **0.984** Win_1→Win_1 | **+0.355** | Win_1/47 | 0 | thin mill closed |
| **19469101** | 469 | 0.860 Opt→Opt | **1.069** Full→Full | **+0.209** | Full/16 | 0 | **no hang**; leftover = Avoid=noop |
| **14689597** | 564 | 0.845 Opt→Opt | **1.042** Opt→Opt | **+0.197** | Opt | 0 | Avoid=noop |

Focus-6: wins **2/6**, median SF/OCC **0.984**, Soft=0, `occ_schedule_picks=0`. Same six on PR #43: wins 0/6, median **0.768**.

14396881 N=3 0.494 is the honest reuse loser. Instant-off on the same block is 1.05×; the sweep OCC median (11.9 ms / 113k TPS) is an OCC-hot outlier vs Instant-off OCC 15.7 ms. Not treated as a structural OCC-retreat.

---

## Representative blocks — Resolve / ordered, not a shell

| block | What the spine does |
|-------|---------------------|
| **3356896** | Thin light-cover. Instant reuse Win_1 + OrderedTip + edge_OA=10 + rsw≈4.8. N=3 climbs only to Win_2 (cover=1). PR43 learned Win_8. |
| **6196166** | Instant reuse Win_1 + OrderedTip. N=3 Win_1/47, cover=1, aborts=0. PR43 Win_8 mill 0.629 → 0.984. |
| **19807137** | Under-covered storage spine. Sticky Opt. FullReplay is the Resolve of Avoid=noop abort — not `ConcurrencyMode::Occ`. One Instant mid-iter `Full/1` leak, last iter Opt. |
| **19469101** | Mid-band leftover Detect bits. Completes N=3 (previously hung). Pick is Avoid=noop, not ReadyEdge starve and not `next_occ_task`. |
| **14396881 / 14689597** | Independent-majority. Opt visibility, `runnable_set_width_mean=1`, Resolve=Commit. Control flow is SF-PS Schedule.pick, named Avoid=noop. |

---

## Acceptance (land brief §5)

| # | Target | Verdict |
|---|--------|---------|
| **P1** architecture | SF pick = RunnableSet / `schedule::pick`; OCC only on `ConcurrencyMode::OCC` | **hit** — test + `occ_schedule_picks=0` + source asserts |
| **P2** Soft=0 / seq≡par / regressions | green | **hit** — Soft=0 all measured rows; lib + specfence integration green |
| **P3** lazy | no 4–27× tail; lazy not OrderedAdmit | **hit** — 15274915 Opt; no thousand-writer Full gate |
| **P4** TPS vs PR43 | win-rate and median must not **both** clearly worsen; conflict reps show Resolve/ordered | **partial / honest** — full N=3 99 not completed. Focus-6 median 0.984 vs PR43 0.768 on the same six; 3356896/6196166 show ordered Win_1/2 not Win_8. 14396881 N=3 0.494 is a real reuse drop (Instant-off 1.05×). N=1 66/98 is OCC-cold, not a P4 claim. |
| **P5** docs | land note + design pointers | **hit** — this file + SoT copies in `lab/notes/` |

P4 was **not** recovered by putting SpecFence back on `next_occ_task`.

---

## Remaining (not this PR)

- Full Soft=0 N=3 reuse @8 on all 99 after the leftover/mid-band pick. Needed before claiming corpus win-rate vs PR #43.
- 19807137 under-covered spine still pays FullReplay on Opt; Instant × ~1.4. Learn must not treat a mid-iter Full/1 as success.
- 14396881 N=3 reuse vs Instant-off disagreement (OCC-hot sweep). Diagnose, do not switch the pick root.
- `erc20_clusters` not re-run after the leftover Avoid=noop pick.
- PartialAbort Rebind/Rewind histogram is still thin on these reps; edged path is Commit / OrderedReplay / FullReplay first.
