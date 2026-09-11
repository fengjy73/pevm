# SpecFence V5-P3 — research plant graduation — status

**Date:** 2026-09-07 (Asia/Shanghai)  
**Branch:** `specfence`  
**Parent tip:** `e52eff0` (V5-P2 θ)  
**Authority:** `lab/notes/specfence-v5-first-principles-clean-slate.md` §7 V5-P3  
**Iron law:** `lab/notes/specfence-first-principles-bottleneck.md`

## Goal

Inspect / absolute jump stay **opt-in**. Graduate **only** hang-free plant pieces that improve SF/OCC on block **14689597** @8 without hangs / SoftWait storms.

## Step A — A/B measure

Harness:

```text
cargo run -p pevm --release --config 'profile.release.lto=false' --example specfence_g7_smoke
```

| Arm | Env | 597 SoftWait | 597 SF/OCC | Hang? |
|-----|-----|-------------:|-----------:|-------|
| **A Lean** (default) | `SPECFENCE_ENABLE_INSPECT` unset | **23** (recheck 39) | **0.127** (recheck 0.093) | no |
| **B Inspect** | `SPECFENCE_ENABLE_INSPECT=1` | — | — | **YES** — timeout 240s on flip block 19606599 after 19606598 completed |
| P2 baseline (ref) | Lean | 30 | 0.166 | no |

Artifacts:

- `lab/results/v5-p3-ab-summary.json`
- `lab/results/v5-p3-ab-lean-sf-occ.json` / `v5-p3-ab-lean.run.log`
- `lab/results/v5-p3-ab-lean-recheck-sf-occ.json`
- `lab/results/v5-p3-ab-inspect-hang.json` / `v5-p3-ab-inspect.run.log`

**SoftWait on Lean stays scarce** (23–39 ≪ G7 428; ~20–50 band). SF/OCC noise vs P2 is load/abort variance — do not chase SoftWait up.

### Verdict from A/B

1. **Full inspect must NOT graduate** — hangs before SF/OCC@8 on the G7 harness.
2. No hang-free RewindTo+FF **win** proven on 597 (inspect never finishes 597).
3. Therefore **graduate nothing behavioral** — plant stays research-only.

## Step B — What was / was not graduated

| Candidate | Decision | Why |
|-----------|----------|-----|
| Full `inspect_run` / absolute PC jump | **No** | Hang timeout under `SPECFENCE_ENABLE_INSPECT=1` |
| Valued CallOutcome SC / multi-SSTORE/LOG jump | **No** | Forbidden mythology; hang history |
| Lean abort → RewindTo+FF (`apply_lean_abort_repair` arm RewindTo) | **No** | No proven hang-free SF/OCC win on 597 |
| SoftWait-wake RewindTo+FF (P4 `try_arm_park_resume_at_k`) | **Already Lean** | Journal FF + force-bind only; no absolute jump — unchanged |
| Fanout→WaitHard | **Never** | Forbidden |

### Code clarity (no behavior change on Lean)

- Separated APIs in `rem.rs`:
  - **Lean:** `apply_lean_abort_repair` — force-bind + clear RewindTo
  - **Research:** `research_apply_abort_repair` — RewindTo+FF+force-bind (inspect path only)
- `pevm.rs` research abort arm uses the research wrapper; Lean path unchanged.
- `vm.rs` comments corrected: Lean does **not** take inspect `rewind_resume`; SoftWait may still replay journal FF via `set_tx`.
- G7 smoke now records dig hooks: `evm_entries`, `rewind_to_cp`, `journal_ff_*`, `park_resume_*`, `full_restart`, `partial_retry_count`.

## Tests

```text
cargo test -p pevm --lib
cargo test -p pevm --test specfence
```

(Must pass; includes `research_apply_abort_repair_arms_rewind_lean_does_not`.)

## Forbidden (unchanged)

Restoring `fanout_hint → WaitHard`, Boolean Wait ladders, Heat/account sticky Wait, or default-on inspect to chase SF/OCC.

## Next

Bottleneck dig prep: `lab/notes/specfence-v5-bottleneck-dig-plan.md`.
