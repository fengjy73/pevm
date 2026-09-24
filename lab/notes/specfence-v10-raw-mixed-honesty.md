# SpecFence v10 — Soft=0 honesty (RAW_fan_out + mixed 52)

**Date:** 2026-09-14  
**Plant:** `cursor/specfence-v10-raw-mixed-e645`  
**Vocabulary:** [`specfence-cc-glossary.md`](specfence-cc-glossary.md)  
**Impl:** [`specfence-v10-raw-mixed-impl.md`](specfence-v10-raw-mixed-impl.md)

## Verdict

Architecture landed on one spine. Soft=0. seq≡par held on product specfence tests.  
**Did not beat OCC on the fan anchor. Did not beat the 0.703 / 0.449 walls** (0.703 was 98-block; this sweep is the curated 52).

| | v10 | Prior bar |
|--|-----|-----------|
| 52-id N=1 median SF/OCC | **0.589** | 0.703 (different corpus) |
| 14689597 N=3 | **0.262** | **0.449** |
| Soft=0 | held | held |
| `park_resume_full_abort_reexecute` | **0** (52) | 214 on timely-Resolve 14689597 |
| `partial_abort` win/attempt | **59/59** | not theater when strips cover |

14689597 N=3: WaitFor **6**, refuse **low**, abort **189 vs OCC 50**.  
Falsifier **still live:** abort>OCC on the fan. Wait→rem-full-abort **as default is dead** (`park_resume_full_abort_reexecute=0`). The remaining tax is ESTIMATE `wait_for_full_abort` + first-wave storage RAW that hint-fan cannot name.

Do not celebrate `wait_for_dependency` or `refuse_admit` counts.
