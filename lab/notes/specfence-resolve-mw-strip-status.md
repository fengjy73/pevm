# SpecFence resolve — maybe_wait meta structural strip

**Date:** 2026-09-07 (Asia/Shanghai)  
**Branch:** `specfence`  
**Base tip:** `c794c67`  
**Authority:** `specfence-resolve-sw-meta-status.md`, `specfence-native-resolve-protocol.md`

## Mandate

Make maybe_wait nearly free for common SpecRead/Bind-on-Data — single MV read like OCC. Full π only for sticky force_bind / program-prior Await / true conflict repair. SoftWait ≪428; no SoftWait Soft storms; SpecFence-native SuffixRepair on conflict. Stretch median wall <15 (better <10).

---

## Profile where maybe_wait went (c794c67)

| Bucket | ~ms (CPU sum) | Note |
|--------|-------------:|------|
| maybe_wait total | **~78** | ~80% of SF handler |
| Bind-on-Data residual | prior `predicts_write` + rem triple-lock + dag | after sticky/HotSet already skipped |
| Cold SpecRead | residual O(n) + sticky + HotSet + prior + full π gate | before OCC-fast return |
| SoftWait Soft idle | ~0.5 | already scarce |

---

## Structural cuts (landed)

1. **Common path = one `last_data_before`** — no `last_writer`+`residual` pair up front; no sticky/HotSet/prior/learner/Bayes/`choose_action`.
2. **`bind_on_data_lite`:** Bind metrics + BlockingOther if unfinished; done → `note_certified_with_effect_boundary` (one rem lock). Skip dag/Bayes/learner/`predicts_write` except M3 prior credit on Bind success.
3. **First incarnation / no force_bind:**
   - cold (no MV writer) → OCC SpecRead (no DashMap meta; no EffectBoundary plant)
   - ESTIMATE → SpecRead
   - unfinished live writer + process prior → **BlockingOther** steal (M3 / program-prior Await without SoftWait Soft)
4. **Full π** only on repair reincarnation (sticky / HotSet / program-prior / `choose_resolve`).

Not restored: SoftWait Soft storms, SpecRead-through-writer without park, is_lazy repair fallthrough (broke M2 seq≡par).

---

## Validation

| Check | Result |
|-------|--------|
| `cargo test -p pevm --lib` | **92 passed** |
| `cargo test -p pevm --test specfence` | **23 passed**, 13 ignored |
| SoftWait 597 median | **0** ≪428 |
| Hang | **No** |
| seq≡par | green (M2/M3) |

### 597 @8 vs `c794c67`

| Metric | c794c67 N=7 / N=11 | **this N=7** | **this N=11** |
|--------|-------------------:|-------------:|--------------:|
| wall median | **21.3** / **22.2** | **14.1** | **14.7** |
| wall p90 | 21.6 / 24.7 | **15.4** | **17.9** |
| wall min | 21.0 / 20.5 | **13.1** | **12.9** |
| SoftWait med | 1 / 1 | **0** | **0** |
| OCC median | ~3.5 | ~3.6 | ~4.0 |
| maybe_wait (profile) | ~78 | **~41** | — |

**SUCCESS:** maybe_wait **materially down** (~78→~41); wall median **materially down** (21–22→14–15); SoftWait scarce; stretch **<15 met**; <10 not. Residual: BO steal idle + validate Instant/tax + rem `note_access` mutex.

---

## Artifacts

- `lab/results/resolve-mw-strip-597.json` — diagnosis summary
- `lab/results/resolve-mw-strip-sf-occ.json` / `*-smoke7.run.log` / `*-smoke11.run.log`
- `lab/results/resolve-mw-strip-prof-sf-occ.json` / `*-prof-smoke7.run.log`
- `lab/results/resolve-mw-strip11-sf-occ.json`

## Code

- `crates/pevm/src/vm.rs` — OCC-lite `maybe_wait_specfence` + `bind_on_data_lite`
- `crates/pevm/src/specfence/rem.rs` — `note_certified_with_effect_boundary`
- `crates/pevm/src/specfence/boundary.rs` — `arm_pending_effect_cp_only`
