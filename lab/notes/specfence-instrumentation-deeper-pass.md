# SpecFence — deeper effect-RAW instrumentation pass

**Date:** 2026-09-07 00:16 UTC+08:00 (Asia/Shanghai)  
**Status:** INSTRUMENTATION + ANALYSIS only — control law v3 remains FROZEN (design); no `choose_action` / HotSet replacement  
**Evidence:** `lab/results/effect-raw-deeper.{json,csv}` + per-block `effect-raw-deeper-b*.json`  
**Base tip before this commit:** `fd61ed1` (fd61ed1 or later)  
**Flags:** opt-in `set_finegrain_journal(true)`; production `Handler::run` default unchanged

---

## Framing

- Plant-observed only. External RAW tables are **not** targets.
- Primary concurrency object remains **location last-writer RAW**.
- Account-grain observes, producer readiness, OCC@8 timing, and WAW-pair proxies are **diagnostic** inputs that may refine the v3 **note**, not production code yet.

## What was added (opt-in)

1. **Account-grain structured edges** (`AccountGrainObserve`) + counters: SLOAD/BALANCE/EXT* when slot cold but account hot; `would_wait` / `would_bind` if policy were account-keyed.
2. **Producer readiness at discovery** on every `RawEffectEdge` + first program-cross: `validated|executed|data|estimate|running|aborting` via live MvMemory+Scheduler attach.
3. **OCC@8 vs OCC@1** discovery timing: gross-work depth, incarnation, abort×discovery correlation.
4. **WAW pairs**: consecutive multi-writer pairs; fraction with **no intervening RAW reader** (HotLocal writer-count Wait proxy).

---

## Headline table (OCC@1 primary RAW; OCC@8 readiness/WAW)

| Block | loc RAW | acct-grain | OCC8 acct would_wait | OCC8 edge ready_done / waitish | gw p50 | WAW pairs no-intervening-RAW | OCC8 aborts (w/ prior disc) |
|-------|--------:|-----------:|---------------------:|-------------------------------:|-------:|-----------------------------:|----------------------------:|
| 14689597 | 647 | 1073 (1044s/29e) | 3858 (0.94) | 0.71 / 0.29 | 0.943 | 144/144 (1.00) | 63 (54) |
| 19606598 | 44 | 83 (81s/2e) | 11 (0.12) | 1.00 / 0.00 | 0.539 | 13/13 (1.00) | 12 (5) |
| 19606599 | 584 | 909 (806s/103e) | 694 (0.44) | 0.87 / 0.13 | 0.381 | 163/177 (0.92) | 89 (31) |
| 19469096 | 232 | 311 (298s/13e) | 481 (0.73) | 0.85 / 0.15 | 0.925 | 169/172 (0.98) | 118 (85) |
| 19469097 | 410 | 567 (524s/43e) | 472 (0.52) | 0.91 / 0.09 | 0.611 | 168/181 (0.93) | 86 (56) |

### Account-grain vs slot RAW

- On **14689597**, account-grain observes **1073** vs location RAW **647** (matches prior diag). Mostly SLOAD (1044) + some EXT* (29); BALANCE≈0 (Basic IS account grain).
- **OCC@1:** account producer always done → `would_bind=100%`, `would_wait=0` (serial artifact).
- **OCC@8:** account-keyed Wait would fire on **~12–94%** of account-grain observes (597: **0.94**, 19469096: **0.73**, 599: **0.44**). That would **change Wait/Bind EV** vs slot-grain (many observes have no slot RAW at all).
- **Implication:** keep primary edges location-correct; do **not** promote account-grain to control-law RAW. Account-grain is a **false-positive Wait amplifier** under parallel if HotSet/account-keyed.

### OCC@8 vs OCC@1 discovery timing + producer readiness

- OCC@1: every RAW edge sees producer `validated` (edge_ready_done=1.0) — expected under serial G*.
- OCC@8 edge-level (includes aborted incarnations): **waitish (running|estimate|aborting) ≈ 0–29%**; done (validated|executed|data) ≈ **71–100%**.
- **First-cross last-incarnation** readiness overstates “producer ready” vs edge-level (e.g. 597 fc_done=1.00 while edge waitish≈0.29) — policy must use **discovery-time** readiness, not final incarnation.
- Abort correlation: most OCC@8 aborts occur on consumers that **already** discovered a program cross; mean gw at abort remains **high** on fan-out/spine (597≈0.75, 19469096≈0.89) and **moderate** on 599≈0.39 — consistent with gross-work Wait/Bind, not EarlyAbort@1%.
- Gross-work p50 unchanged vs prior freeze (597 **0.94**, 599 **0.38**, 097 **0.61**, 096 **0.93**).

### WAW-only / HotLocal writer-count proxy

- `multi_writer_no_readers = 0` on all sampled blocks: every multi-writer location has **some** reader somewhere.
- But **consecutive WAW pairs without intervening RAW** are the norm: **144/144 (1.00)** on 597, **169/172 (0.98)** on **19469096**, **163/177 (0.92)** on 599.
- Pure location-level “no RAW at all” (`waw_only_multi_writer_locs`) is **0** — writer-count HotSet still correlates with some RAW presence.
- **Spurious Wait sense for v3:** WaitHard keyed **only on writer count** would still park on WAW steps that are **not** RAW discoveries between writers. Reinforces frozen v3: **schedule/steal over WaitHard on long WAW spines**, HotSet = tracking cache only.

---

## Does v3 control law need amendment? (design only)

| Claim in frozen v3 | Still holds? | Note |
|--------------------|--------------|------|
| Gross-work Wait/Bind on fan-out / late `d` | **Yes** | gw p50 unchanged; abort mean gw still high on 597/096 |
| EarlyAbort minority (heavy `d≈0.1`), not block-wide 1% | **Yes** | No ≪1% class; abort×discovery not early-work dominated |
| Bind when producer Data published | **Yes** | OCC@8: majority edges already Data/Validated; waitish minority needs Wait/SpecRead |
| SpecRead on handler / short-lag | **Yes** | unchanged |
| WAW spine: schedule/steal ≫ WaitHard on writer-count | **Strengthened** | ≥92% of WAW pairs lack intervening RAW |
| Account-grain as primary RAW | **No — reject** | Inflates vs location RAW; OCC@8 would_wait very high → EV noise |

### Recommended design-note amendments (not code)

1. **Producer readiness is timing-mode dependent:** evaluate `producer_ready` at the discovering incarnation (edge-local), never from final-incarnation snapshot alone.
2. **Account-grain:** explicit non-goal for `choose_action`; may appear in diag / learning features as a **negative** prior (account-hot ≠ slot RAW).
3. **HotLocal writer-count:** treat high `waw_pairs_no_intervening_raw` as evidence against Wait-on-count; morphology prior for spine blocks should bias schedule/steal.
4. Otherwise **v3 stays frozen** for the next `choose_action` implementation.

---

## Pointers

- Frozen law: `lab/notes/specfence-adaptive-control-law-v3-frozen.md`
- Prior first principles: `lab/notes/specfence-evm-cc-first-principles-from-effect-raw.md`
- Runner: `cargo run -p pevm --release --config 'profile.release.lto=false' --example specfence_effect_raw_deeper`

