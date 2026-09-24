# SpecFence SuffixRepair Lean resume fix

**Date:** 2026-09-07 (Asia/Shanghai)  
**Branch:** `specfence`  
**Authority:** `lab/notes/specfence-native-resolve-protocol.md`  
**Bug tip:** `c13ee14` (SuffixRepair-first Lean resolve; resume path still `!lean`-gated)

---

## Bug

In `Vm::execute` (~c13ee14):

```rust
let lean = ... engagement.begin_tx(...); // true on SpecFence Lean default
let rewind_resume = SpecFence && !lean && partial_retry.is_rewind_resume(...);
```

Native resolve armed Lean SuffixRepair / SoftWait-wake **RewindTo**
(`is_rewind_resume=true`), but **`!lean` forced `rewind_resume=false`** on the
default path. Symptoms on G7 597:

| Metric | Dig Lean | c13ee14 native | Meaning |
|--------|---------:|---------------:|---------|
| wall_ms | 36.1 | 46.4 | worse |
| evm_entries | 2416 | 2672 | worse |
| resume_count | 0 | 0 | fake head reexec |
| rewind_to_cp | 24 | 451 | SuffixRepair armed |
| journal_ff_hits | — | 2009 | `try_ff_*` keyed off table |

So Lean paid full `Handler::run` from head + `record_evm_entry`; journal FF could
hit, but resume accounting / PC-skip never engaged. CallEntry push was also
`!lean`-gated → weaker mid-tx cps for SuffixRepair quality.

---

## Fix (SpecFence-native, not OCC++)

### 1. Lean SuffixRepair takes the resume path

- `rewind_resume = SpecFence && is_rewind_resume` — **drop `!lean`**.
- Prefer `record_resume` over `record_evm_entry`.
- `try_ff_*` unchanged (still table `is_rewind_resume`).

### 2. Hang-free prefix skip (narrow)

- When Lean + RewindTo + `ff_continuation` passes **`jump_is_safe`** (same gates
  as `try_arm_safe_absolute_jump`): open **inspect_run for this incarnation only**
  and arm jump via `try_arm_safe_absolute_jump_gated(..., suffix_repair_jump_env_ok())`.
- Does **not** require whole-block `SPECFENCE_ENABLE_INSPECT`.
- Honors explicit `SPECFENCE_ABSOLUTE_JUMP=0`.
- If jump unsafe (typical Lean lite EffectBoundary snaps): **Handler::run +
  journal FF only** (current hang-free SoftWait-wake subset).

### 3. Checkpoints on Lean first run

- Push `CallEntry` on SpecFence execute regardless of lean (still skip on
  rewind_resume). Mid-tx EffectBoundary (SpecRead/Bind) + end write cps unchanged.

### 4. Hang / seq≡par guards (critical)

Lean journal-FF-only resume must **not**:

- **Seed FF read origins** before run — stale FF origin vs post-wake MV Data →
  `InconsistentRead` SoftWait livelock (caught by m2/p4/p2 integration tests).
- **Residual-republish** certified prefix writes — pastes stale MvMemory Data when
  Handler::run already re-executed stores → **seq≠par** (`p2_full_retry`).

Both stay enabled only when `suffix_jump || research_inspect` (PC-skip path).

### 5. π

Unchanged: Await-on-tie for known Running writer; SoftWait scarce.

---

## Validation

| Check | Result |
|-------|--------|
| `cargo test -p pevm --lib` | **88 passed** |
| `cargo test -p pevm --test specfence` | **23 passed**, 13 ignored |
| G7 smoke | `SPECFENCE_G7_TAG=suffix-repair-resume` |
| Hang 597/599? | **No** |
| seq≡par | `ok=true` all cores |

### 597 @8 vs baselines

| Metric | Dig Lean | c13ee14 | **This fix** |
|--------|---------:|--------:|-------------:|
| SoftWait | 41 | 41 | **50** (≪428) |
| wall_ms | 36.1 | 46.4 | **33.0** (beats dig) |
| evm_entries | 2416 | 2672 | **1018** |
| resume_count | 0 | 0 | **346** |
| rewind_to_cp | 24 | 451 | **342** |
| journal_ff_hits | — | 2009 | **1570** |
| force_bind_reabort | 184 | 180 | **183** |
| full_restart | 378 | 0 | **0** |

**SUCCESS:** Lean SuffixRepair actually resumes (`resume_count>0`); hang-free;
wall and `evm_entries` down vs c13ee14 **and** dig Lean wall.

---

## Artifacts

- `lab/results/suffix-repair-resume-sf-occ.json`
- `lab/results/suffix-repair-resume-flip.json`
- `lab/results/suffix-repair-resume-smoke.run.log`

## Code

- `crates/pevm/src/vm.rs` — resume gate, CallEntry, narrow jump, seed/residual gates
- `crates/pevm/src/specfence/boundary.rs` — `absolute_jump_eligible`,
  `try_arm_safe_absolute_jump_gated`, `suffix_repair_jump_env_ok`
- `crates/pevm/src/specfence/rem.rs` — docs
