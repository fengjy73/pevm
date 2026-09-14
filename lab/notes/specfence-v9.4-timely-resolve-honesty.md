# SpecFence v9.4 timely Resolve — Soft=0 honesty

**Date:** 2026-09-14  
**Plant:** this package (PinHold rem checkpoint + R1b covered + Ready refuse + storage true-k)  
**Parent wall:** PR #11 Soft=0 nonempty median **0.7033** / fan 14689597 N=3 **0.4485**; R1 **3/2439**; pin **5158**; refuse **1540**.

## Plant (this tip)

| Weld | Status |
|------|--------|
| PinHold arms rem checkpoint → wake ResumeAtK | **code** (Unfenced snaps; first-access FullRetry — no k=1 livelock). Unit: `arm_pinhold_*` |
| R1b `try_arm_r1b_covered` without cp_k≥8 theater | **code** (strips only, **one** RewindTo). No arm → honest B0, not ForceBind train. Unit: `try_arm_r1b_covered_*` |
| Refuse while producer Ready (not only Executing) | **code** (unit: `refuse_known_consumer_while_producer_ready`) |
| ReadyCanary killed on Ready; DoneUnfenced cert=false | **code** (unit: `ready_producer_is_pinhold_not_canary`) |
| hint-fan + storage InterPrior PE; access-gate Storage true-k | **code** (unit: `hint_fan_seeds_small_accounts_and_storage_prior`) |
| SoftWait Soft | **not armed** (Soft=0 held in plant) |

## Sweep (verdict)

All-blocks Soft=0 JSON is **not in this commit**. Do not claim crush of median ≥0.95 / fan ≥0.90 / R1 ≥50%.

**Bars to beat on the next Soft=0 sweep:** median **0.703**, fan **0.449**, R1 win rate up materially, abort_SF ≤ OCC on fan_out when PE+certs present.

Celebrate neither pin↑ nor refuse↑. The falsifier remains: WaitFor↑ ∧ abort≈OCC ∧ R1≈0.
