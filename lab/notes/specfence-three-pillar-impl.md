# SpecFence three-pillar impl (one coherent drop)

**Date:** 2026-09-08 (Asia/Shanghai)  
**Branch:** `specfence`  
**Base tip:** `1058c89` (ABC Iter-30)  
**Stance:** SpecFence-native CC — OCC is wall/TPS baseline only.  
**Smoke:** `lab/results/three-pillar-sf-occ.json` (+ flip `three-pillar-flip.json`)  
**Tag:** `SPECFENCE_G7_TAG=three-pillar` N=5 @8 cores

---

## Defaults (production-on; env = dig escape only)

| Knob | Default | Escape |
|------|---------|--------|
| Await@a (storm+program+live_fanout≥8) | **ON** | `SPECFENCE_DISABLE_AWAIT_AT_A=1` |
| SoftWait Soft | dormant (~0) | `SPECFENCE_DISABLE_SOFTWAIT=1` (already dig) |
| ResumePath Bind tips / nested apply | ON (Iter24–30) | `SPECFENCE_NESTED_BIND=0`, SNAP/JUMP dig |
| tip≡FF / storage max_steps | **8192** (was 2048) | — |
| Quiet vs Storm morph | inter prior + live flip | — |

---

## Pillar 1 — Access-grain Await@a (NOT SoftWait Soft 1.0)

**Where:** `crates/pevm/src/vm.rs` `maybe_wait_specfence`

- Storm ∧ program ∧ `live_fanout_hot` (≥8) ∧ unfinished writer → **Await@a**:
  - brief yield, then **BlockingOther** park (steal-friendly dependency-requeue) until writer done
  - Validated yield-spin before Bind; if writer restarts → re-BO
  - metrics: `await_at_a_arms` / `await_at_a_wake_ok` / `await_at_a_wake_reabort`
- Quiet: OCC-lite only (`force_prefix` / sticky / `prior_inc0`) — no live_fanout Await
- SoftWait Soft arms stay **0** (FenceGraph Soft not restored)
- Hollow Iter17 yield-spin→Bind-no-park on hot unfinished **removed** for storm top-k

**Falsifiers (not reintroduced):** SoftWait Soft storms, WaitHard ladders, account Wait, Heat/Bayes-bool π.

---

## Pillar 2 — Resolve ≠ FullRestart

**Where:** `boundary.rs` / `rem.rs` (arm gates); keep Iter12–30 serial-barrier / SuffixRepair / ResumePath

- tip≡FF / storage / non-empty `tip_sloads` **max_steps 2048→8192** (597 tips at PC 2–6k were `steps_over`)
- deferred Bind tip rank: prefer in-cap ∧ tip≡FF over deepest steps_over (one attach/resume, no mass SNAP)
- Lean-safe nested apply default-on kept (Iter30)
- Refuse unsafe jumps rather than hang; SoftWait Soft=0

**Success signals this drop:** 597 **aj=6** (tip baseline aj≈0); 599/097 aj>0; SoftWait Soft=0.

---

## Pillar 3 — Learning-driven morph mode

**Where:** `engagement.rs` Quiet|Storm; `pevm.rs` inter morph + top-ℓ; `vm.rs` Await gated on `is_storm()`

- quiet → OCC-lite discovery (minimal meta; Await@a off)
- storm → Await-ready on top-k hot ℓ only (`live_fanout_hot`)
- inter morph prior + mid-block `maybe_flip_mode` (598→599 style); never sticky SoftWait bitmap t−1→t
- smoke reports `engagement_switches` / `await_a` / `await_ok`

---

## G7 smoke table (N=5 median @8; SoftWait Soft=0)

| Block | SF wall med | OCC wall med | SF/OCC TPS | SF abort med | OCC abort med | Soft | await_a* | await_ok* | aj* | eng_sw* | park_ms* |
|------:|------------:|-------------:|-----------:|-------------:|--------------:|-----:|---------:|----------:|----:|--------:|---------:|
| **14689597** | **18.0** | **6.1** | **0.36** | **87** | **65** | 0 | 17 | 2 | **6** | 1 | 5.9 |
| **19606599** | **27.5** | **12.3** | **0.49** | **218** | **81** | 0 | 0 | 0 | 9 | 1 | 9.8 |
| **19469097** | **16.4** | **7.1** | **0.43** | **217** | **93** | 0 | 13 | 4 | 3 | 3 | 5.5 |
| **19606598** | **2.9** | **1.5** | **0.52** | **14** | **8** | 0 | 0 | 0 | 0 | 1 | 0.0 |

\*last-iter counters (not median). mean SF/OCC TPS = **0.449**.

Flip smoke 598→599: SoftWait Soft=0 both; flip_count advances.

### vs tip baseline (`pause-rethink-tip`, Soft=0)

| | Tip 597 | Three-pillar 597 |
|--|--------:|-----------------:|
| SF wall med | 12.5 | 18.0 |
| OCC wall med | 4.0 | 6.1 |
| SF/OCC TPS | 0.35 | 0.36 |
| SF abort med | ~164 | **87** |
| SoftWait Soft | 0 | **0** |
| aj | ≈0 | **6** |
| Await@a | n/a (hollow) | **17 arms / 2 wake_ok** |

Host OCC ~1.5× slower than tip sample; **ratio flat (~0.45 mean)**. Absolute wall not &lt;10. Protocol shape moved: Await@a actuates, aj on storm, SoftWait Soft=0, Quiet 598 await_a=0.

Lean `cargo test -p pevm --test specfence -- lean`: **3 passed** (incl. Iter30 nested default-on).

---

## Code touch list

- `crates/pevm/src/vm.rs` — Await@a prefer_await + wake mark
- `crates/pevm/src/specfence/{engagement,metrics,rem,boundary,mod}.rs`
- `crates/pevm/src/pevm.rs` — await_at_a wake_ok/reabort
- `crates/pevm/examples/specfence_g7_smoke.rs` — await_a / eng_sw print+JSON

**STOP after this drop** — see `specfence-three-pillar-reflect.md`.
