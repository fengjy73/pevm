# SpecFence resolve — prevent first force_bind_reabort

**Date:** 2026-09-08 (Asia/Shanghai)  
**Branch:** `specfence`  
**Base tip:** `9836720`  
**Authority:** `specfence-resolve-fb-loop-status.md`, `specfence-native-resolve-protocol.md`

## Mandate

Prevent **first** force_bind abort / first SuffixRepair fail. Before SpecRead/Bind-no-park on incarn0 hot/prior/sticky unfinished writers, prefer BlockingOther prefer-steal Await until Data+Executed. SoftWait Soft = 0; keep escalate-after-1-reabort; keep RebindOnly. Stretch median wall &lt;10ms; tests green; no hang.

---

## Diagnosis (597 @ 9836720)

| Signal | Meaning |
|--------|---------|
| First fail grain | Incarnation-0 (and sticky) **Bind-no-park on unfinished published Data** — reading past unfinished writer → validate fail → SuffixRepair + force_bind |
| force_prefix path | Already spin64 + **BO prefer-steal Await** then Bind (post-fail only) |
| sticky / hot_inc0 | Only yield-spin then Bind-no-park — **gap before first fail** |
| Loop status | Broken at tip (`full_restart ≈ fb_reabort`); still pay full EVM on escalate — need fewer first aborts |

Wrong Bind-no-park / reading past unfinished writer is the first-abort grain. SoftWait Soft storms are not.

---

## Cuts landed

1. **Unfinished published Data:** `prefer_await = force_prefix \|\| sticky \|\| prior_inc0` → spin64 + **BlockingOther prefer-steal Await** until `is_done`, then Bind. Extends force_prefix BO to **incarnation-0 prior** and sticky (block-wide), before SpecRead/Bind-no-park.
2. **No-Data unfinished:** prior **or** hot **or** sticky → BO Await before SpecRead (incarn0).
3. **Keep** ESTIMATE→SpecRead on incarn0 (tried ESTIMATE→BO: wall↑ / fb flat).
4. **Keep** SoftWait Soft = 0; escalate-after-1-reabort; RebindOnly.

Tried / not landed: all-incarn0 unfinished Data BO (fb↓ but wall↑ vs prior-gated); incarn0 ESTIMATE spin+BO (wall↑).

---

## Validation

| Check | Result |
|-------|--------|
| `cargo test -p pevm --lib` | **93 passed** |
| `cargo test -p pevm --test specfence` | **23 passed**, 13 ignored |
| SoftWait 597 median | **0** |
| Hang | **No** |
| seq≡par | green |

### 597 @8 vs `9836720` (same-machine baseline)

Tip commit recorded N=7 best wall **12.8**; **same-machine tip re-run** median **14.0** (box noise). Compare against same-machine baseline.

| Metric | tip recorded | **same-machine tip** | **this N=7 best (t2)** | **this N=7 trip** | **this N=11** |
|--------|-------------:|---------------------:|-----------------------:|------------------:|--------------:|
| wall median | **12.8** | **14.0** | **13.2** | 13.5 / 13.2 / 14.2 | **13.8** |
| wall p90 | 15.1 | 14.2 | **13.4** | | **14.8** |
| wall min | 12.2 | 12.6 | **12.0** | | **12.3** |
| SoftWait med | 0 | **0** | **0** | **0** | **0** |
| OCC median | ~3.7 | ~3.4 | ~3.7 | | |
| resume (last) | **88** | **91** | **84** | 85 / 84 / **77** | **101** |
| force_bind_reabort (last) | **97** | **99** | **96** | 93 / 96 / **76** | **100** |
| full_restart (last) | **97** | **99** | **96** | 93 / 96 / **76** | **100** |
| occ_aborts med | 223 | **210** | **204** | 201 / 204 / 204 | **200** |
| rebind_only (last) | 0 | 1 | 0–1 | | 0 |

**PARTIAL SUCCESS:** vs same-machine tip — resume / fb_reabort / full_restart down; abort_med down; wall best **13.2** (vs baseline **14.0**); SoftWait 0; escalate identity preserved. Stretch **&lt;10 not met**. Residual first fails remain SpecRead/ESTIMATE and Executed-not-yet-Validated writer aborts.

---

## Artifacts

- `lab/results/resolve-prevent-first-597.json` — diagnosis summary
- `lab/results/resolve-prevent-first-sf-occ.json` / `*-smoke7.run.log` (t2 best)
- `lab/results/resolve-prevent-first-t{1,2,3}-sf-occ.json` — triplicate
- `lab/results/resolve-prevent-first-baseline-sf-occ.json` — same-machine tip
- `lab/results/resolve-prevent-first11-sf-occ.json` / `*-smoke.run.log`
- `lab/results/resolve-prevent-first-flip.json`

## Code

- `crates/pevm/src/vm.rs` — unfinished Data prefer_await (force_prefix \| sticky \| prior_inc0) → BO Await; incarn0 no-Data prior/hot/sticky → BO
