# SpecFence resolve push status

**Date:** 2026-09-07 (Asia/Shanghai)  
**Branch:** `specfence`  
**Authority:** `specfence-native-resolve-protocol.md`, `specfence-suffix-repair-resume-fix.md`, `specfence-v5-detect-avoid-resolve.md`  
**Base tip:** `fbbd321` (Lean SuffixRepair resume)

---

## What changed (three bold resolve moves)

### 1. SuffixRepair absolute jump — path armed, live capture deferred

- Lean SuffixRepair still narrow-arms absolute jump when `jump_is_safe` **and** Storage FF prefix (`suffix_repair_jump_env_ok`, no whole-block `SPECFENCE_ENABLE_INSPECT`, honor `SPECFENCE_ABSOLUTE_JUMP=0`, anti-livelock `jump_disabled`).
- Attempted one-shot `needs_live_capture` inspect after `force_bind_reabort` to populate live `jump_snap` (lite EffectBoundary snaps never pass `is_live_capture`).
  - **Result:** cut reaborts but **4× `evm_entries` / wall regression** on 597; opening inspect on Storage SuffixRepair resumes also broke seq≡par (`bayes_storage_conflict`).
- **Decision:** consume `needs_live_capture` without inspect tax; keep jump arming ready for when a hang-free live snap source exists. G7 now exports `absolute_jump_applied` / `absolute_jump_fallback` (still **0** on Lean 597).

### 2. RebindOnly-first (avoid reexec)

- Validation path prefers **RebindOnly** when invalid origins are patchable to current Data/Storage **and** there is no *true* failed-suffix write (`first_k ≥ k_fail`).
- Uncertified writes **before** `k_fail` no longer block RebindOnly (they are not true suffix).
- `rebind_only` still **0** on 597 this run (most fails still have true suffix / Estimate) — path + unit tests are in place.

### 3. Sticky resolve after `force_bind_reabort` ✅ primary win

- On abort while force_bind armed: record sticky ℓ + **`extend_force_bind(invalid)` after** `apply_suffix_repair` (so SuffixRepair’s `set_force_bind(certified)` is not overwritten away).
- Next touch of conflict ℓ: **Bind if Data else SpecRead** (force_prefix), not SpecRead-default reabort loops.
- **Not** EV Wait dampening / Boolean WaitHard — that SoftWait/park-stormed 597 (`wait_park` 497→2662). SoftWait stays scarce.

---

## Validation

| Check | Result |
|-------|--------|
| `cargo test -p pevm --lib` | **91 passed** |
| `cargo test -p pevm --test specfence` | **23 passed**, 13 ignored |
| Hang 597/599? | **No** |
| seq≡par | green |
| SoftWait 597 | **38** ≪428 (≤80) |

### 597 @8 vs `fbbd321`

| Metric | fbbd321 | **resolve-push** | Δ |
|--------|--------:|-----------------:|---|
| SoftWait | 50 | **38** | scarce ✓ |
| wall_ms | 33.0 | **24.2** | **↓ 27%** |
| OCC wall_ms | 3.6 | 3.5 | — |
| occ_aborts | 303 | **72** | **↓ toward OCC 76** |
| force_bind_reabort | 183 | **40** | **↓ 78%** |
| evm_entries | 1018 | 1177 | slight ↑ |
| resume_count | 346 | 100 | fewer SuffixRepair |
| rebind_only | 0 | 0 | path ready |
| absolute_jump_applied | — | **0** | live snap deferred |
| SF TPS | 17111 | **23263** | ↑ |
| SF/OCC | 0.110 | **0.143** | ↑ |

**SUCCESS:** sticky force_bind extend makes resolve meaningfully stronger (wall + aborts + `force_bind_reabort` down); SoftWait scarce; tests green. Jump/RebindOnly remain wired for next push.

---

## Artifacts

- `lab/results/resolve-push-sf-occ.json`
- `lab/results/resolve-push-flip.json`
- `lab/results/resolve-push-smoke.run.log`

## Code

- `crates/pevm/src/pevm.rs` — RebindOnly-first; sticky extend after SuffixRepair
- `crates/pevm/src/vm.rs` — Storage-prefix jump gate; live_prime consume without inspect
- `crates/pevm/src/specfence/rem.rs` — `extend_force_bind`, `has_true_suffix_writes`, `needs_live_capture`
- `crates/pevm/src/specfence/learner.rs` — `note_sticky_resolve` / `is_sticky_resolve`
- `crates/pevm/src/specfence/resolve.rs` — sticky field (EV-neutral; force_bind owns sticky)
- `crates/pevm/examples/specfence_g7_smoke.rs` — export `absolute_jump_*`
