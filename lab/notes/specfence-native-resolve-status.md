# SpecFence-native resolve status

**Date:** 2026-09-07 (Asia/Shanghai)  
**Branch:** `specfence`  
**Authority:** `lab/notes/specfence-native-resolve-protocol.md`  
**Tip before:** `48f5ee7` (protocol note) / `8a08122` (detect/avoid/resolve dig)

---

## What changed

### A. SuffixRepair-first Lean abort (`rem.rs` + `pevm.rs`)

- New default Lean resolve: [`PartialRetryTable::apply_suffix_repair`]
  - When `plan_partial_retry` yields a certified prefix **and** a hang-free mid-tx
    checkpoint (`0 < cp.k < k_fail`, same SoftWait-wake subset as
    `try_arm_park_resume_at_k`): **arm RewindTo + journal FF + force-bind**.
  - Else certified but no mid-tx cp → ForceBind + head reexec (legacy fallback).
  - Else → FullRestart (last resort).
- `apply_lean_abort_repair` is a thin alias of `apply_suffix_repair`.
- `LeanAbortRepair::SuffixRepair { certified, suffix_writes, reexec_cost }` added;
  after SuffixRepair, `is_rewind_resume` is **true** so `set_tx` / `try_ff_*` apply.
- Absolute PC jump / valued CallOutcome stay research-only (`SPECFENCE_ENABLE_INSPECT`).
- Lean validation path (`pevm.rs`): SuffixRepair → `invalidate_partial_suffix` +
  `record_rewind_to_cp`; ForceBind/FullRestart → selective + `record_full_restart`.
- Unit tests flipped: expect RewindTo armed when mid-tx checkpoint exists.

### B. π identity (`resolve.rs`)

- Removed protocol identity “ties → SpecRead (OCC-like default)”.
- Tie law: if writer **known Running/unfinished** and `EV_Wait ≈ EV_Spec` →
  **Await (WaitHard)**; SpecRead remains for writer absent/unknown discovery.
- Meta tax still biases tiny Wait wins → Spec (no SoftWait storm).
- No Boolean fanout→WaitHard ladders.

### C. Docs

- `specfence/mod.rs`, `rem.rs` headers, `vm.rs` comments updated to SpecFence-native
  SuffixRepair / Await vocabulary (not OCC++).

---

## Validation

| Check | Result |
|-------|--------|
| `cargo test -p pevm --lib` | **88 passed** |
| `cargo test -p pevm --test specfence` | **23 passed**, 13 ignored (M1* research) |
| G7 smoke Lean default | `SPECFENCE_G7_TAG=native-resolve-lean` → `lab/results/native-resolve-lean-*.json` |
| Hang on 597/599? | **No** (absolute jump off) |
| seq≡par | green (integration + smoke `ok=true`) |

### 597 @8 vs dig Lean baseline

| Metric | Dig Lean | Native SuffixRepair | Note |
|--------|---------:|--------------------:|------|
| SoftWait | 41 | **41** | ≪428 ✓ |
| SF wall_ms | 36.1 | 46.4 | not yet improved |
| OCC TPS / SF TPS | ~ / 15622 | 141493 / 12158 | SF/OCC **0.086** (dig ~0.153) |
| occ_aborts | 378 | 425 | more aborts this run |
| evm_entries | 2416 | 2672 | not yet improved |
| force_bind_reabort | 184 | **180** | slight drop |
| full_restart | 378 | **0** | SuffixRepair replaced head restart |
| rewind_to_cp | 24 | **451** | abort SuffixRepair + SoftWait wake |
| journal_ff_hits | (n/a) | **2009** | Lean FF serving |
| resume_count | 0 | 0 | inspect/PC-jump path only (correct) |
| park_resume_at_k | 24 | 26 | SoftWait wake hang-free |

**Iron law:** SoftWait scarce ✓; SuffixRepair is the default resolve verb
(`full_restart=0`); π not OCC-shaped ✓; absolute jump off ✓. Wall / `evm_entries`
not yet below dig — hang-free FF skips MV work for certified prefix but still
pays prefix opcode execution without absolute jump (intentional; inspect stays off).

---

## Bisect note (if hang)

Did **not** hang. If a future path hangs with SuffixRepair: keep journal FF +
force-bind, disable inspect/PC jump (`SPECFENCE_ENABLE_INSPECT` unset), document.
Absolute jump remains research-only.

---

## Artifacts

- `lab/results/native-resolve-lean-sf-occ.json`
- `lab/results/native-resolve-lean-flip.json`
- `lab/results/native-resolve-lean-smoke.run.log`
