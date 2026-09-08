# SpecFence abort cheapening + Bind hit quality — status

**Date:** 2026-09-07 12:05 CST (Asia/Shanghai)  
**Branch:** `specfence`  
**Parent tip:** `eb2695e` (AEC)  
**Authority:** AEC stays (argmin EV); no Boolean Wait ladders / fanout→WaitHard.

---

## 1. Problem (597 @8 AEC)

| Metric | G7 ladders | AEC baseline | This tip |
|--------|----------:|-------------:|---------:|
| SoftWait arms | 428 | 28 | **20** |
| WaitHard | 2828 | 503 | **20** |
| SF aborts | 81 | 325 | **82** |
| SF TPS | ~22k | ~16k | **~24.7k** |
| OCC TPS | — | ~155k | ~154k |
| **SF/OCC** | ≈0.17 | **≈0.104** | **≈0.161** |
| Bind hits | — | — | 824 |

Success: SF/OCC **0.161 > 0.104**; SoftWait stays scarce (≪ G7 428).

---

## 2. What landed

### A. Cheapen abort path (hang-free, LeanOCC)

- Lean abort: classify `plan_partial_retry` → **`set_force_bind(certified)`** + selective invalidate (no inspect / no PC jump).
- `force_prefix`: **Bind when Data ready, else SpecRead** — never WaitHard without Data (SoftWait livelock on 599).
- Research-inspect path still prefers RewindTo+journal FF when `cp.k > 0`; `note_reexec_cost` feeds learner.
- RebindOnly unchanged on LeanOCC.
- **Not restored:** Boolean Wait ladders / fanout→WaitHard / D_WAIT gates.

### B. Raise Bind hits (AEC EV)

- Bind when Data + (`writer_done` ∨ `prior_ws` ∨ `placeholder` ∨ high P ∨ **`posterior_bind_success ≥ τ_s`**).
- Unresolved published Data enters EV_Bind≈scaled wake cost vs Spec (argmin); high fanout still Spec (no SoftWait storm).
- When WŜ predicts bindable-soon, bump `EV_Spec` by bind posterior (not Boolean Wait).

### C. Calibrate EV_Spec reexec

```text
EV_Spec = P_abort * (W_remain + β·E_cascade + 0.5·E_reexec)
```

- `LiveLearner::note_reexec_cost` EMA from RebindOnly≈0.1 / force-bind FullRestart≈1.2 / RewindTo≈0.6 / bare FullRestart≈2.0+.
- Prices Spec on fan-out so Wait can win **only when EV says wake is cheap**.

### D. Constraints kept

- seq≡par TCB; AEC argmin; ties→SpecRead; EarlyAbort only with known d; Lean no default inspect tax; Learning ∉ TCB.

---

## 3. Hang-free lessons (this PR)

1. **Bind-on-every-Data** SoftWait-armed when `!writer_done` → livelock on 599. Fixed via quality gates + EV Bind.
2. **force_prefix→WaitHard** without Data → SoftWait livelock. Fixed → SpecRead.
3. **CallEntry on every lean execute** + aggressive RewindTo arming → hang class. Lean stays OCC-fast; force-bind only.

---

## 4. Other smoke blocks (this tip @8)

| Block | SoftWait | WaitHard | SF/OCC |
|------:|--------:|---------:|-------:|
| 14689597 | 20 | 20 | **0.161** |
| 19606599 | 13 | 13 | 0.302 |
| 19469097 | 77 | 77 | 0.288 |
| 19606598 | 3 | 3 | 0.317 |
| **mean** | | | **≈0.267** |

AEC baseline mean ≈0.250; G7 mean ≈0.269.

---

## 5. Files

- `crates/pevm/src/specfence/resolve.rs` — Bind quality + EV_Spec·E_reexec + tests
- `crates/pevm/src/specfence/learner.rs` — `note_reexec_cost` / `e_reexec`
- `crates/pevm/src/specfence/mod.rs` — wire `e_reexec` into PolicyCtx
- `crates/pevm/src/specfence/rem.rs` — abort cheapening unit tests
- `crates/pevm/src/pevm.rs` — lean force-bind + reexec samples; inspect RewindTo notes cost
- `crates/pevm/src/vm.rs` — hang-free force_prefix Bind/SpecRead
- `lab/notes/specfence-abort-cheapening-status.md` — this note
- `lab/results/abort-cheapening-smoke.run.log`, `g7-sf-occ-smoke.json`

---

## 6. Validation

- Unit: Bind EV / cheaper repair / reexec-priced Wait — **green**
- `cargo test -p pevm --lib` — **76 green**
- Integrations: `small_blocks`, `raw_transfers`, `mixed`, lean/r1 SpecFence — **green**
- Smoke 597/599/097/598 @8 hang-free

---

## 7. Blockers / next

1. 597 SF/OCC 0.161 still below G7/v8 (~0.17/0.32) — further gains from mid-tx RewindTo+FF **without** inspect tax (lite EffectBoundary on lean) once hang-free.
2. Absolute PC jump / CallOutcome SC remain research-gated (`SPECFENCE_ENABLE_INSPECT`).
3. Do **not** restore D_WAIT / fanout_hint→WaitHard to chase ratio.
