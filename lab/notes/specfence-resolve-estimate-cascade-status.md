# SpecFence resolve — abort→ESTIMATE→BlockingOther cascade cut

**Date:** 2026-09-07 (Asia/Shanghai)  
**Branch:** `specfence`  
**Base tip:** `a5cf67e`  
**Authority:** `specfence-native-resolve-protocol.md`, blocking-other + park-steal status notes

## Mandate
Cut abort → ESTIMATE → BlockingOther park cascade without restoring cold-hint SpecRead,
SpecRead-through-writer, Wait storms, or finish-under-lock hang paths.

## Cascade map (pre-fix)

| Abort arm | ESTIMATE install | Dependents Blocking? |
|-----------|------------------|----------------------|
| `SuffixRepair` | `invalidate_partial_suffix(suffix_writes)` only | Suffix readers only (prefix Data kept, no abort stamp) |
| `ForceBind` (certified, no mid-tx cp) | **was** `invalidate_selective` → ESTIMATE all higher-reader writes **+** global aborted stamp | **All** readers of aborted writes → BlockingOther (ESTIMATE or aborted-incarnation) |
| `FullRestart` | `invalidate_selective` (OCC-like) | Same; sticky prior ForceBind still poisoned protected writes |
| Cold WaitHard `hints.prev` | (no ESTIMATE) | BlockingOther park (unchanged; SpecRead restore forbidden) |

**Attribution at `a5cf67e` last-iter 597:** BO parks **1519** / idle **435ms** vs SoftWait Soft 50 / 24ms.
BO owns residual park idle; abort ESTIMATE storm ≫ cold WaitHard.

## Shipped

1. **ForceBind carries `suffix_writes`** — ESTIMATE via `invalidate_partial_suffix` only (never `invalidate_selective` / abort stamp on certified prefix).
2. **FullRestart + sticky ForceBind** — capture `prior_force_bind` before repair clears it; ESTIMATE only writes ∉ protect.
3. **BlockingOther steal-convert** — `add_dependency` first; if kind=BlockingOther and prefer-steal writer Ready, return stolen **without** `park_with_kind` (no long park idle; wake via `transactions_dependents`). SoftWait/EarlyAbort still park for `(t,k)` resume.

Not restored: cold-hint SpecRead, SpecRead-through-writer, Wait storms, finish-under-lock.

## Validation

| Check | Result |
|-------|--------|
| `cargo test -p pevm --lib` | **92 passed** |
| `cargo test -p pevm --test specfence` | **23 passed**, 13 ignored |
| SoftWait 597 median | **48–87** ≪428 |
| Hang | **No** |
| seq≡par | green (specfence tests) |

### 597 @8 N≥7 vs `a5cf67e`

| Metric | a5cf67e (blocking-other) | this best (t4) | this noisy |
|--------|-------------------------:|---------------:|-----------:|
| wall median | **26.0** | **24.9** | 28.8 / 30.3 |
| wall min | 21.1 | **22.7** | 19.4 |
| SoftWait med | 47 | **48** | 87 / 78 |
| abort med | 194 | **117** | 220 / 121 |
| BO parks (last) | **1519** | **181** | 210 / 182 |
| BO idle ms (last) | **435** | 387 | **212** / 221 |
| total parks (last) | 1569 | 337 | 291 / 182 |
| ready_steal | 257 | 239 | 271 |

**SUCCESS:** BlockingOther park count and typical idle down sharply vs `a5cf67e`; best wall median **24.9 < 26.0**; SoftWait scarce; aborts not exploding (best abort med 117); tests green; no hang.

## Artifacts
- `lab/results/resolve-estimate-cascade-sf-occ.json` / `*-smoke7.run.log`
- `lab/results/resolve-estimate-cascade-t4-sf-occ.json`
- `lab/results/resolve-estimate-cascade-flip.json`
- `lab/results/resolve-estimate-cascade-597.json` (summary)

## Code
- `crates/pevm/src/specfence/rem.rs` — ForceBind.`suffix_writes`; `arm_steal_convert_without_park`
- `crates/pevm/src/pevm.rs` — ForceBind/FullRestart suffix-only ESTIMATE; BlockingOther steal-without-park
