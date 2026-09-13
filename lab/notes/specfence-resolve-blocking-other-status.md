# SpecFence resolve — subtype-2 park idle cut

**Date:** 2026-09-07 (Asia/Shanghai)  
**Branch:** `specfence`  
**Base tip:** `2e141f2`  
**Authority:** park-steal + profile-strip status notes

## Mandate
Cut subtype-2 (ESTIMATE / cold) park idle without SoftWait strips / Wait storms / SpecRead-through-writer.

## Call sites
- vm basic/storage ESTIMATE -> park + set_pending_park_location
- vm basic/storage aborted incarnation -> park + tag
- vm maybe_wait_specfence cold WaitHard hints.prev -> set_pending_park_location
- vm nonce / code_hash abort -> park (unwrap_or subtype-2)
- pevm handler parks with pending kind or subtype-2 default

Why SF > OCC: OCC only ESTIMATE/abort/nonce; SF also SoftWait WaitHard + cold hints + EarlyAbort; abort cascades mint more ESTIMATE. Subtype-2 owns residual park idle at tip (~149ms of ~187ms).

## Shipped
1. ESTIMATE/abort pending park tagging (promote kept)
2. prefer-steal writer after park (`next_task_steal_after_park_prefer`)

Tried/reverted: cold-hint SpecRead; finish handoff under lock; Validation woken Execution; no-promote-on-ESTIMATE; post-finish steal-on-None.

## Validation
| Check | Result |
|-------|--------|
| lib tests | 92 passed |
| specfence tests | 23 passed, 13 ignored |
| SoftWait 597 median | 47 << 428 |
| Hang | No |

### 597 @8 vs tip
| Metric | tip recorded | this best N=7 | triplicate |
|--------|-------------:|--------------:|------------|
| wall median | 30.0 | **26.0** | 29.6 / 28.2 / 26.0 |
| wall min | 26.6 | **21.1** | |
| SoftWait med | 52 | **47** | 46-49 |

## Artifacts
- lab/results/resolve-blocking-other-597.json
- lab/results/resolve-blocking-other-sf-occ.json / smoke7.run.log
- lab/results/resolve-blocking-other-flip.json

## Code
- crates/pevm/src/vm.rs, scheduler.rs, pevm.rs, specfence/rem.rs
