# Focus pair optimize v3

**Date:** 2026-09-22
**Tip:** `c62f5bd` on `cursor/specfence-sf-ps-true-spine-d6e8` (code ≡ opt-v2 `190b926` after revert)
**Baseline tip+note:** `190b926` / `lab/notes/specfence-cc-pc-v3-focus-pair-opt-v2.md`
**Blocks:** `3356896` and `15274915` only. No mixed-49.
**Harness:** Instant-off, Soft=0, 8 cores, `SPECFENCE_COMPARE_CHECK=1`.
**specfence-lab:** still HTTP 404. This note is the record on the pevm branch.

Lib release: 416 passed. `complete_arch_edge_pi_seq_eq_par_softwait0` passed. Soft=0 Instant-off N≥5 both blocks: `seq=par`, `occ_picks=0`, `soft_wait_arms=0`, `explore=0`, no hang.

## Verdict

No net land. Every thin Avoid that cut Opt→FullReplay either reopened a discarded tax (park / Rewind / 15-hold / Estimate Block / tip-hold) or failed to make **3356896** primary **stably** under remasure 1.43 without risking **15274915** v2 wins. Code tip is restored to opt-v2 behavior. This note records the verified call-graph and the discarded attempts (including temporary commits `9e12cd2` / `1a09199`, reverted).

## Why thin still Opt→FullReplay at fail_k 5/6

Ungated path (`skip_ungated_tx_path_tax`, `optimistic_skip_gate`):

1. `consult_ungated_wait_once` sees WaitOnce / crit — thin returns without Block or rem checkpoint.
2. `maybe_wait` is a no-op on ungated.
3. Writer often installs Data only at `MvMemory::record` end — no Estimate tip → Opt reads Storage pre-state at k=5/6.
4. `validate_to_plan` Opt → `try_early_waw_rewind` returns None on thin → **FullReplay**.
5. Learn `note_early_waw` arms WaitOnce for reuse; IntraPatch Win is vetoed on thin majority; `explore=0`. AccessArm changes; execute plane does not.

## Hypotheses verified

### 1. Ungated publish-order after FullReplay (no mark_gated)

`note_ungated_wait_on` to nearest unfinished tip + `mark_wait` on requeue so `release_owner` cannot Indep Opt-mill past the pred. Opt resume kept.

- **Broad plant** (all invalid ℓ + break_replay_mill): emptied antichain — `rset_w` ~25 vs ~75, wall spikes 2–6 ms.
- **Narrow** (EffectiveWAW only, `inc≥1`, thin-only mark_wait): some calm 3356896 primaries ~1.21–1.42; neighboring N=5 still 1.5–1.7. On this host 15274915 reuse median swung to ~1.8–2.3 (above v2 ~1.58) when sticky/head noise hit.
- **Landed then reverted** (`9e12cd2`, `1a09199` → `c62f5bd`).

### 2. Thin Estimate Block when WaitOnce already armed

Allow `decide`→Block only on a live Estimate tip for WaitOnce locs (consult still skips not-started preds).

- Raised 3356896 primary into the 1.45–1.70 band. **Discarded.**

### 3. Tip-only short hold on thin reuse (≤3 pairs, WaitOnce loc)

`plant_nearest_preds` on last 4 writers of short WaitOnce spines at begin — not the full 15-hold.

- Primary mostly above 1.43. **Discarded.**

### 4. Still discarded (from v1/v2; not re-tried)

Gated nearest-pred / mark_gated hold; full 15-writer begin hold; one-shot InconsistentRead re-read; thin RewindTo; thin rem checkpoints; two early Rewinds.

## Same-host numbers (this session, Instant-off N=5)

Remasure targets: **3356896 ~1.43**, **15274915 ~2.37**. opt-v2 tip calm: **~1.35** / **~1.58**.

| Round | 3356896 primary | 15274915 primary |
|:---:|---:|---:|
| restored v2 #1 | 1.86 | 1.61 |
| restored v2 #2 | 1.44 | 2.12 |
| restored v2 #3 | 1.64 | 1.76 |

Host OCC medians are often ~0.85–1.0 (faster than opt-v2 note’s ~1.09), so the same absolute SF (~1.3–1.6 ms) prints a higher ratio. Absolute calm SF still sits near remasure SF absolute; ratio is not stably under 1.43. 15274915 head-first ~0.6 ms and `resolve_rewind`≫0 / `full_from_0` low remain when the ≥32 sticky hold sticks (v2 behavior).

## Criterion status

| | Status |
|:---|:---|
| A Soft=0 Instant-off N≥5 both, seq=par, occ_picks=0 | **Met** on restored tip |
| B 3356896 clearly+stably under 1.43 | **Not met** — thin Opt→FullReplay remains; noise 1.4–1.9 |
| B 15274915 ≤1.58, not worse than v2 | **Hold v2** — no regressing land kept |
| C this note + PR #45 | **Met** |

## Open

A thin Avoid that orders the doomed early basic **after** the peer’s Data without park tax, Rewind tax, or antichain drain. Publish-order waits move the race to start-time; on n=176 that either under-helps (inc≥1 only) or over-serializes (broader plant). Learn already arms WaitOnce; the execute plane must consume it without the discarded taxes.
