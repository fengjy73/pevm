# SpecFence v9.4 timely Resolve — Soft=0 honesty

**Date:** 2026-09-14  
**Vocabulary:** [`specfence-cc-glossary.md`](specfence-cc-glossary.md)  
**Plant:** `82d9262` / `fbf328a` (WaitForDependency snapped resume k≥8 + one-shot PartialAbortRewind + Ready|Executing refuse)  
**Parent wall:** PR #11 Soft=0 nonempty median **0.7033** / fan 14689597 N=3 **0.4485**; partial_abort **3/2439**; wait_for_dependency **5158**; refuse_admit **1540**.

## Plant (this tip)

| Weld | Status |
|------|--------|
| WaitForDependency arms rem checkpoint → wake ResumeAtK | **code** (OptimisticRead snaps; first-access / k<8 FullAbortReexecute). Unit: `arm_wait_for_dependency_*` |
| PartialAbortRewind `try_arm_partial_abort_rewind` without cp_k≥8 theater | **code** (strips only, **one** RewindTo). No arm → honest full_abort_reexecute. Unit: `try_arm_partial_abort_rewind_*` |
| Refuse while producer Ready or Executing | **code** (unit: `refuse_known_consumer_while_producer_ready`). Dropping Ready-refuse lost 19807137 0.269→0.220 |
| ReadyCanary killed on Ready; DoneOptimisticRead cert=false | **code** (unit: `ready_producer_is_wait_for_dependency_not_canary`) |
| hint-fan + storage InterPrior PE; access-gate Storage true-k | **code** (unit: `hint_fan_seeds_small_accounts_and_storage_prior`) |
| SoftWait Soft | **not armed** (Soft=0 held) |

## Sweep (verdict)

**Did not beat 0.703 / 0.449.** Soft=0 held. partial_abort is no longer theater.

### Focus N=3 @8 (`lab/results/v94-timely-resolve-focus.json` @ k≥8 + Ready-refuse)

| bn | sf_occ | parent | Wait | wait_for_dependency | partial_abort | refuse_admit | park resume_k / full_abort_reexecute | abort SF / OCC |
|---:|-------:|-------:|-----:|--------------------:|--------------:|-------------:|-------------------------------------:|---------------:|
| 14689597 | **0.276** | 0.449 | 200 | 228 | 1/1 | 836 | 12 / 214 | 111 / 41 |
| 19807137 | **0.269** | 0.231 | 1437 | 1481 | **672/673** | 1719 | 16 / 1466 | 1009 / 1066 |
| 19606599 | **0.499** | 0.831 | 112 | 119 | 18/18 | 31409 | 64 / 65 | 76 / 93 |
| 19469097 | **0.445** | 0.508 | 149 | 152 | 34/35 | 6733 | 24 / 132 | 58 / 77 |

Soft=0 on all four. OrderedAdmit 0. Focus median **0.445**.

### All-blocks N=1 @8 (`lab/results/v94-timely-resolve-all-n1.json`)

- nonempty (n=98) median SF/OCC **0.600** (misses 0.703). Mean 7.7 is OCC-slow N=1 noise — do not celebrate.
- Soft=0 (0 blocks with `soft_wait_arms>0`).
- partial_abort **1574 / 1581** (rate **0.996**) — Resolve conversion is real when strips cover.
- 24/98 ≥1.0; 37/98 ≥0.70; 35/98 <0.50.

## What cashed vs what did not

**Cashed:** partial_abort theater is dead (win≈attempt). 19807137 abort_SF **≤** OCC (1009/1066) and N=3 **0.269 > parent 0.231**. WaitForDependency no longer livelocks (empty/k<8 FullAbortReexecute; PartialAbortRewind depth 1). Iter26 seq≡par held.

**Did not cash:** 14689597 still **0.276 ≪ 0.449** — WaitForDependency FullAbortReexecute + extra abort (111 vs 41) + refuse_admit 836. All-blocks median **0.600 ≪ 0.703**. Ready-refuse 31k on 19606599 is idle tax (dropping it lost the 19807137 beat). Tiny ResumeAtK was worse (0.17 / 0.09) so k≥8 FullAbortReexecute is the hang-free product path until cheap jump exists.

Do not celebrate wait_for_dependency↑ / refuse_admit↑. Falsifier on 14689597 is still live: WaitFor↑ ∧ abort>OCC ∧ partial_abort≈0.

**Bars to beat next:** median **0.703**, fan **0.449**, abort_SF ≤ OCC on 14689597 when PE+certs present.

Historical sweep JSON still uses old keys (`r1_win`, `waitfor_pin`, `schedule_refuse`). Live code emits the glossary names.
