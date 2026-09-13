# SpecFence P4 — optional `(t,k)` park continuation status

**Date:** 2026-09-07 (Asia/Shanghai)  
**Branch:** `specfence`  
**Authority:** `specfence-region-fence-adaptive-architecture.md` §2.3 / §8 P4  
**Parent tip:** `8c875c4` (P0/P1/P2 status)
**Tip:** `43323b8739778e750fe466471fc0aeec24d7ab94` (`43323b8`)

---

## Goal (honest slice)

Evolve WaitHard park so SoftWait can **record** and (when safe) **resume from** armed effect ordinal `k`, not only whole-tx restart.

Full mid-tx live Interpreter park remains **unsafe** (M1k/M1l hang lessons). This milestone ships the **data plane + API** plus a **hang-free safe subset**.

---

## What shipped

### Data plane
| Piece | Change |
|-------|--------|
| SoftWait `armed_at_k` | Already on `FenceGraph::arm_soft`; now uses **per-tx** `PartialRetryTable::current_k` (not global rem hint) |
| `PendingPark { location, armed_at_k }` | Thread-local pending WaitHard carries `k` |
| `ParkedWait.armed_at_k` | WavePark park entry stores SoftWait `k` |
| `ParkResumeIntent` | Wake restores intent `(waiter, armed_at_k, location)` |
| FenceGraph | `soft_wait_k`, `wake_on_publish_arms` (arms retain `k`) |

### Safe subset resume
`PartialRetryTable::try_arm_park_resume_at_k(t, k)`:

1. If `k == 0` **or** no checkpoint with `0 < cp.k < k` **or** empty certified prefix → **`ParkResumeKind::FullRetry`** (tx-grain head reexec — M2 behaviour).
2. Else arm existing **RewindTo + journal FF + force-bind** at that checkpoint → **`ParkResumeKind::ResumeAtK`**.

Absolute PC jump is **not** newly enabled; it stays behind M1e/M1l safety / `SPECFENCE_ENABLE_INSPECT`.

### Wiring
- `vm.maybe_wait` WaitHard → `arm_soft(..., k)` + `set_pending_park(ℓ, k)`
- `pevm` Blocking → `wave.park(t, writer, ℓ, k)`
- Writer finish → `wake_*_intents` stores resume intent
- Next `try_execute` → `vm.try_apply_park_resume` before `execute` (journal still parked incarnation)

### Metrics
- `park_resume_at_k` — wakes that armed RewindTo/FF
- `park_resume_full_retry` — wakes that fell back to tx-grain FullRetry

---

## What is still tx-grain FullRetry

- SoftWait with `armed_at_k == 0` (no journaled observes yet — common off HotSet / lean)
- Only synthetic CallEntry checkpoint at `k=0`
- Empty certified/journal prefix before SoftWait observe
- Default production lean path often never builds mid-tx cps → wake intent recorded but arms FullRetry
- Live Interpreter continuation / true `(t,k)` task grain (rayon steal of mid-effect frames) — **out of scope**

---

## Coordination with P3 EarlyAbort

P3 may also touch resolve/rem. P4 only uses FenceGraph SoftWait + WavePark park/wake + `PartialRetryTable` RewindTo APIs — no competing WaitHard spin path. EarlyAbort still returns via rem repair; SoftWait park remains the WaitHard worker-free path.

---

## Constraints respected

- seq≡par (integration tests green)
- No default inspect tax (`SPECFENCE_ENABLE_INSPECT` still required for jump research)
- SoftWait SoT = FenceGraph
- Beneficiary / lazy never SoftWait (unchanged)
- P0–P2 metrics/tests unbroken

---

## Tests

```
cargo test -p pevm --lib -- --test-threads=1
# includes rem::p4_tk_park_tests (4) + dag SoftWait k tests

cargo test -p pevm --test specfence --test raw_transfers --test small_blocks --test mixed -- --test-threads=1
# specfence: + specfence_p4_tk_park_seq_eq_par_and_metrics
```

Unit coverage:
- park stores `armed_at_k`; wake restores intent
- pending park carries `k`
- `try_arm_park_resume` → FullRetry when unsafe
- `try_arm_park_resume` → ResumeAtK when cp before `k`

---

## Blockers / next

1. **Per-tx `k` often 0** until HotSet journals the Observe — optional: note_access on WaitHard arm for off-HotSet observes (careful vs PartialRetry semantics).
2. **LeanOCC** skips mid-tx checkpoints → ResumeAtK rare in production default.
3. True mid-effect park still needs hang-free Interpreter serialize+FF (M1l residual) — not this PR.
4. Do **not** touch `pevm-specfence-server`.

