# SpecFence v6 — cost-aware π (cut over-WaitHard)

**Date:** 2026-09-04 (Asia/Shanghai)  
**Branch:** `specfence`  
**Commit:** (see git)  
**Non-goal:** wave ready-queue; pevm-specfence-server untouched; OCC/PCC unchanged.

## Decision rule

Replace sticky / low-τ WaitHard bias with expected-cost comparison:

```
cost_wait ≈ 0.0 if writer Executed/Validated else 1.0
cost_spec ≈ 1.0 + P_conflict * C_retry     // C_retry = 3.0
margin    = 0.40                           // wait must be clearly cheaper

if Bind ready:                         Bind
elif writer_known && cost_wait < cost_spec * margin: WaitHard
elif P_conflict >= τ_very_high (0.75): WaitHard   // safety valve
else:                                  SpecRead
```

Also:
- Sticky Wait is revoke-only (`P < τ_revoke=0.20`); it no longer forces WaitHard.
- EarlyVal escalate SpecRead→Wait/Bind only at `P ≥ 0.75` (was 0.35).
- Metrics: `cost_chose_{wait,spec,bind}`, `mean_p_at_{wait,spec}`.

## Smoke @8 (vs v5 smoke / v5 full)

Blocks 19807137, 19434587, 15199017; cores 1,8; repeats 1.

| block | v5 smoke WH | v6 WH | ΔWH | v5 smoke SF TPS | v6 SF TPS | v5 smoke SF/OCC | v6 SF/OCC | wait_adm v5→v6 |
|-------|-------------|-------|-----|-----------------|-----------|-----------------|-----------|----------------|
| 19807137 | 12589 | 8369 | −33% | 5583 | 6481 | 0.220 | 0.141* | 2614→938 |
| 19434587 | 3667 | 2848 | −22% | 11430 | 12356 | 0.314 | 0.327 | 329→380 |
| 15199017 | 904 | 181 | −80% | 63007 | 61025 | 0.349 | 0.277* | 644→634 |

\*OCC TPS jumped a lot this run on 19807137/15199017 (machine noise); absolute SF TPS is the fairer within-box compare to v5 smoke.

Sum wait_hard: **17160 → 11398 (−33.6%)** — below the ≥50% gate.  
Mean SF/OCC: **0.294 → 0.248** — not >0.45.

Cost-model sanity (v6 hot block): `mean_p_at_wait≈0.90`, `mean_p_at_spec≈0.10` — Wait reserved for high-P; Spec on cold.

vs v5 **full** mean @8 (19807137): WH 10799→8369; wait_adm 1262→938; SF TPS 6879→6481 (noisy single-repeat).

## Gate decision

Criteria for full 7-block v6: mean SF/OCC@8 >0.45 **or** wait_hard drop ≥50% with TPS up.  
**Neither met → stop after smoke.** No `figures-v6`.

Honest read: cost-aware π + sticky demotion cut WaitHard and wait_admissions on the hottest block and move π mass toward SpecRead at low P, but residual WaitHard from Bind-blocking / safety-valve / writer-done paths still taxes the hot block; SF still trails OCC ~3–7× at 8 cores. Next lever is wave ready-queue (P2 remainder), not further τ-only retuning.

## Tests

```
cargo test -p pevm --release --config 'profile.release.lto=false' --test specfence -- --test-threads=1
```
16/16 green + resolve/bayes unit tests for high-P+writer-done→WaitHard / moderate-P+writer-running→SpecRead.

## Artifacts

- `lab/results/mainnet-sweep-v6-smoke.json` (+ `.csv`)
- `lab/notes/mainnet-sweep-v6-smoke.log`
