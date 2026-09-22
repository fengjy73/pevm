# Focus pair optimize v2

**Date:** 2026-09-22
**Tip:** `190b926` on `cursor/specfence-sf-ps-true-spine-d6e8`
**Baseline tip+note:** `8f27955` / `lab/notes/specfence-cc-pc-v3-focus-pair-opt-v1.md`
**Blocks:** `3356896` and `15274915` only. No mixed-49.
**Harness:** Instant-off, Soft=0, 8 cores, `SPECFENCE_COMPARE_CHECK=1`. Primary wall is the reuse median.
**specfence-lab:** `repos/fengjy73/specfence-lab` returns HTTP 404. This note is the record on the pevm branch.

Lib release tests: 416 passed. `complete_arch_edge_pi_seq_eq_par_softwait0` passed. Both blocks `seq=par`, `occ_picks=0`, `soft_wait_arms=0`, `explore=0`, no hang on completed N≥3 Instant-off runs.

## What landed (vs opt-v1)

1. **Ungated WaitOnce consult.** `consult_ungated_wait_once` runs on basic/storage before the Opt MV walk. Crit-loc and WaitOnce arms are consulted on the ungated path (AccessArm was previously skipped by `optimistic_skip_gate`).
2. **Thin shell (3356896, n≤176):** consult only — no Block, no RewindTo, no rem checkpoints. Short-spine park / Rewind raised the wall; 15-writer hold and one-shot re-read stay discarded.
3. **Large block (15274915):** hang-free early `PartialAbortRewind` (Indep, no live_capture) when a mid-tx checkpoint exists before `fail_k`. Crit-loc / WaitOnce pushes that checkpoint. Park only when the nearest pred is live.
4. **Sticky ≥32 hold** kept from opt-v1 (plant ungated nearest preds, head-first LIFO). Quiet snapshots must not drop a ≥32 chain.
5. **`full_from_0` cut:** basic FF hits re-snap into rem `value_snap`. A second fail after Rewind/ff_head no longer empties the prefix keep. Early-WAW WaitOnce with known `k>0` survives morph flip (same-k Opt retired across reuse).

## Criterion C

| Evidence | 3356896 | 15274915 |
|:---|:---|:---|
| WaitOnce on ungated early-WAW | Consulted (`is_wait_once` / crit hash). Block counter stays 0 by design — thin Skip. | `wait_once` ~50–80 per reuse iter; parks when pred live |
| `resolve_rewind` | 0 by design (RewindTo tax > FullReplay on thin) | ~48–80 per reuse iter; real fail_k resume via checkpoint + hang-free Rewind |
| `full_from_0` vs opt-v1 | 0 (prefix keep) | reuse **0–5** (opt-v1 was **32–80**) |

Fail_k resume call-graph (large): ungated basic/storage → `consult_ungated_wait_once` → `push_checkpoint_at_k(access_k-1)` → Opt read may FullReplay → `try_early_waw_rewind` finds `last_checkpoint_before(fail_k)` → `PartialAbortRewind` + Indep requeue → next incarnation RewindTo / ff_head.

## Same-host numbers vs remasure

Census remasure on this host: **3356896 ~1.43**, **15274915 ~2.37**. Quieter note samples were 1.28 / 2.34. Compare ratios inside one run.

### 3356896

Chain stays under 32 — no hold. Thin consult+Skip, no Rewind.

| | opt-v1 tip calm N=3 | this tip calm N=5 | this tip under remasure N=5 |
|:---|---:|---:|---:|
| OCC median ms | 1.093 | 0.959 | 0.984 |
| SF reuse median ms | 1.615 | 1.408 | **1.330** |
| primary SF/OCC | 1.48 | 1.47 | **1.35** |
| FullReplay reuse | 21, 26 | 8–20 | (calm) |
| full_from_0 | 0 | 0 | 0 |
| resolve_rewind | 0 | 0 | 0 |
| explore / occ_picks / soft | 0 / 0 / 0 | 0 / 0 / 0 | 0 / 0 / 0 |

Calm primary **1.35** is under remasure 1.43 (not always under quieter 1.28). Neighboring N=5 runs on the same binary land 1.47–1.76 when one reuse iter spikes — same host noise class as opt-v1's unlucky 2.45. Absolute calm SF ~1.33–1.41 is under remasure SF absolute (~1.49).

### 15274915

N=5 reuse. Long hold + head-first sticky. Head first-start ~0.59–0.89 ms when chain holds (opt-v1 ~0.60).

| | opt-v1 tip N=7 | this tip N=5 |
|:---|---:|---:|
| OCC median ms | 5.057 | 5.350 |
| SF reuse median ms | 11.089 | **8.441** |
| primary SF/OCC | 2.19 | **1.58** |
| head first-start | ~0.60 ms | tx 129 ~0.59–0.65 ms |
| FullReplay reuse | 111–192 | 27–78 |
| full_from_0 reuse | **32–80** | **0–3** |
| WaitOnce reuse | 9–30 | 75–78 |
| resolve_rewind | 0 | **79–80** |
| explore / occ_picks / soft | 0 / 0 / 0 | 0 / 0 / 0 |

Primary clearly under remasure 2.37 and under quieter 2.34. `full_from_0` fell an order of magnitude. Chain-head early start held.

## Discarded (still)

- Gated nearest-pred / mark_gated hold
- 15-writer hold on 3356896
- One-shot InconsistentRead re-read
- Thin RewindTo / thin WaitOnce Block / thin rem checkpoints
- Two early Rewinds before FullReplay escalate (wall↑ on 15274915)

## Still open

- **3356896** stays Opt-then-FullReplay at fail_k 5/6; thin path cannot afford park or Rewind. Ratio stays remasure-band on calm samples.
- Occasional rset_w collapse spikes (~2–6 ms) on thin; not SoftWait / not hang.
