# SpecFence resolve — SoftWait Soft + maybe_wait meta cut

**Date:** 2026-09-07 (Asia/Shanghai)  
**Branch:** `specfence`  
**Base tip:** `97071cc`  
**Authority:** `specfence-resolve-reprofile-status.md`

## Mandate

Cut residual wall after BO kill: SoftWait Soft idle + maybe_wait meta (~91ms). SoftWait arms ~125 ≪428. Do not restore ESTIMATE Blocking storms.

---

## SoftWait Soft profile (597 @8)

| Signal | tip `97071cc` | Note |
|--------|--------------:|------|
| SoftWait arms median | **125** | ≪428 scarce already |
| wake_ok : wake_reabort | **~20 : 48** | SoftWait Soft mostly **useless** |
| SoftWait Soft idle | **~18.8ms** | Dominated residual park after BO→0 |
| maybe_wait meta | **~91ms** | ~80% of SF handler |

Root: **Bind-on-Data while producer still Executing** SoftWait Soft-armed (mid-publish WS). Secondary: unfinished-writer short-circuit WaitHard'd **HotSet alone** (EV raises Wait cost with fanout — SoftWait wrong direction).

Tried / reverted: Bind-certify-without-park and SpecRead-through-unfinished Bind (abort-storm / re-sticky SoftWait); WaitHard→BlockingOther for sticky-no-Data (wall↑).

---

## Cuts (landed)

1. **Bind-on-Data before** sticky / HotSet / prior / learner DashMap (collapse hot Bind meta).
2. **Bind unfinished → BlockingOther steal-prefer** — dependency park without FenceGraph SoftWait Soft arm (Soft Soft wake_reabort ≫ useful; steal-convert like BO kill).
3. **EV-aligned cheap Await:** sticky SoftWait Soft; program-prior WaitHard without π; **HotSet-alone → SpecRead** (no SoftWait).
4. Skip Bayes/learner bind credits on **writer_done** Bind (publish already credited).

Not restored: ESTIMATE Wait storms, SpecRead-through-writer, Bind-certify-without-park SoftWait-strip.

---

## Validation

| Check | Result |
|-------|--------|
| `cargo test -p pevm --lib` | **92 passed** |
| `cargo test -p pevm --test specfence` | **23 passed**, 13 ignored |
| SoftWait 597 median | **0–1** ≪428 |
| Hang | **No** |
| Abort storm | **No** (stable N=11) |

### 597 @8 vs `97071cc`

| Metric | 97071cc N=7 / N=11 | **this N=7** | **this N=11** |
|--------|-------------------:|-------------:|--------------:|
| wall median | **22.1** / **22.6** | **21.3** | **22.2** |
| wall p90 | 23.4 / 24.7 | **21.6** | **24.7** |
| wall min | 21.7 / 21.3 | **21.0** | **20.5** |
| SoftWait med | 132 / 125 | **1** | **1** |
| OCC median | ~3.5 | ~3.5 | ~3.7 |
| maybe_wait (profile) | ~91 | **~78** | — |
| Soft Soft idle | ~18.8 | **~0.5** | — |
| BO idle | ~0.7 | **~7** | (steal residual) |

**SUCCESS (partial):** SoftWait Soft **scarce** (0–1); meta **down** (~91→~78); wall N=7 **materially below 22** (21.3 vs 22.1). Stretch &lt;15 **not** met — residual BO steal idle + maybe_wait + validate.

---

## Artifacts

- `lab/results/resolve-sw-meta-597.json` — diagnosis summary
- `lab/results/resolve-sw-meta-sf-occ.json` / `*-smoke7.run.log` / `*-smoke11.run.log`
- `lab/results/resolve-sw-meta-prof-sf-occ.json` / `*-prof-smoke7.run.log`
- `lab/results/resolve-sw-meta-flip.json`

## Code

- `crates/pevm/src/vm.rs` — Bind-on-Data meta strip; Bind unfinished BlockingOther; EV-aligned SoftWait Soft / HotSet SpecRead
