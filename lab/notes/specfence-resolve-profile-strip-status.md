# SpecFence resolve profile + structural strip status

**Date:** 2026-09-07 (Asia/Shanghai)  
**Branch:** `specfence`  
**Base tip:** `1c1ad81` (stabilize Bind-on-Data)  
**Landing tip:** `9fe988d`  
**Authority:** `specfence-native-resolve-protocol.md`

---

## Phase 1 — Profile (597 @8)

Enabled `SPECFENCE_PROFILE=1` Instant buckets (default **off** — Instant tax on every SpecRead biases wall).

Artifact: `lab/results/resolve-profile-597.json` (from `resolve-profile-sf-occ.json`, iters=7).

### Bucket table (SF CPU-ms **sum across workers**, not wall)

| Bucket | SF ms | OCC ms | Note |
|--------|------:|-------:|------|
| Handler::run (incl. DB/maybe_wait) | **157** | **15** | |
| maybe_wait / π / Bayes/HotSet | **125** | 0 | ≈80% of SF handler |
| Pure EVM (handler − maybe_wait) | **32** | ~15 | Not the 9× |
| validate + SuffixRepair/Rebind | **73** | **5** | |
| scheduler next_task | ~1 | — | |
| SoftWait / park idle | **391** | 0 | Dominates cumulative idle |
| **Wall median** | **30.6** | **3.6** | gap ≈27ms |

### Root bucket for the ~27ms / ~9× gap

**`softwait_park_idle` + `maybe_wait` meta** — not EVM Handler dominance.

Evidence:

1. SF often has **fewer** `evm_entries` than OCC on fair captures, yet ~9× wall.
2. SoftWait **arms** stay scarce (≪428); park **idle ns** and low steal-vs-park still serialize the critical path.
3. `maybe_wait` is ~80% of SF handler CPU vs OCC’s near-zero read-path tax.
4. SpecRead-through-writer and Bind→SpecRead SoftWait-strips moved median toward ~26ms on lucky N=11, but **abort-stormed** or broke `M3 prior_bind` under suite parallelism — not landed.

**Not root:** SoftWait arm count, whole-block inspect (off), raw EVM seconds vs OCC.

---

## Phase 2 — Structural strip (landed)

Consistent with SpecFence-native resolve + profile:

1. **Bind-on-Data before** HotSet / learner `note_observe` / `try_revoke` / `choose_action` DashMap tax (collapse hot Bind path).
2. **First-incarnation OCC-fast SpecRead** only when no writer / prior / sticky / force / hotset (never SpecRead through a live writer).
3. Profile Instant gated by `SPECFENCE_PROFILE=1`.
4. M3 retries 8→24 (schedule noise with Bind-first).

**Tried and reverted:** SpecRead-through-writer; Bind→SpecRead instead of SoftWait for non-repair (wall↓ but M3/aborts).

Do **not** restore Wait storms or whole-block inspect.

---

## Validation

| Check | Result |
|-------|--------|
| `cargo test -p pevm --lib` | **91 passed** |
| `cargo test -p pevm --test specfence` | **23 passed**, 13 ignored (×5 green) |
| SoftWait 597 median | **40** ≪428 |
| Hang 597/599? | **No** |

### 597 @8 — multi-run vs tip `1c1ad81`

| Metric | tip stabilize N=7 | **strip N=11** | Δ |
|--------|------------------:|---------------:|---|
| wall median | **29.2** | **29.4** (p90 38.5, min 23.7) | ≈ flat |
| OCC wall median | 3.3 | ~3.7 | — |
| SoftWait median | 33 | **40** | scarce ✓ |
| abort median | 263 | **306** | schedule-noisy |

Stretch &lt;15 **not** met. Profile-backed next lever: **critical-path park/steal** (hang-free resume, steal when Ready) without SoftWait storms — not more Bind labels / SoftWait knobs.

---

## Artifacts

- `lab/results/resolve-profile-597.json` — bucket diagnosis
- `lab/results/resolve-profile-sf-occ.json` / `resolve-profile-smoke7.run.log` — `SPECFENCE_PROFILE=1`
- `lab/results/resolve-profile-strip-sf-occ.json` / `*-smoke11.run.log` — wall N=11

## Code

- `crates/pevm/src/specfence/metrics.rs` — profile ns + `occ_fast_first`
- `crates/pevm/src/specfence/engagement.rs` — `profile_timing_enabled`
- `crates/pevm/src/vm.rs` — Bind-first; OCC-fast writer-none; profile gates
- `crates/pevm/src/pevm.rs` — validate/scheduler profile gates
- `crates/pevm/examples/specfence_g7_smoke.rs` — export profile fields
- `crates/pevm/tests/specfence.rs` — M3 retries 24
