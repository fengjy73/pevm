# SpecFence A+B+C Iter 11 status

**Date:** 2026-09-08 (Asia/Shanghai)  
**Branch:** `specfence`  
**Base tip:** `28948dd` (Iter10)  
**Authority:** `specfence-abc-unified-protocol.md`, `specfence-abc-iter10-status.md`, `specfence-global-cc-evm-rethink.md`

## Mandate

Make **multi-SSTORE last tip** Handler abs jump ≡ sequential under pevm MV
(`write_replays_at_tip` / plant gas / memory restore), then optionally enable Lean
capture+jump if hang-free and wall↓. If falsified, find alternate resume opcode cut
without abs jump. SoftWait Soft~0; stock SSTORE on mass path; no Lean inspect/live_prime.

## Root cause (diagnosed this iter)

1. **Plant pre-`sload` warmed storage** before stock SSTORE → EIP-2929 cold access
   (−2100 gas) → **seq≠par** on capture path (diag: `Δcum_gas=2100`, `aj=0`, `hs>0`).
2. **Iter9 claimed `k < k_fail` jump-snap select but code still used `k ≤ cp.k`** and
   preferred exact `cp.k` — early tips beat later multi-SSTORE tips between cp and fail.
3. **Plant `note_write_replay` did not pin `first_k`** (Iter10 stripped finalize
   first_k-from-gas) → mid-abort before finalize Write `note_access` left
   `fk=MAX` → `build_continuation` dropped tip write_replays → `jump_is_safe` refused.
4. Even after (1)–(3), **Lean abs jump still does not prove `aj>0 ∧ seq≡par`** on
   fixtures: SuffixRepair→capture→second-abort→jump chain rarely closes; when
   `SPECFENCE_ABSOLUTE_JUMP=1` forced capture, jump_is_safe / eligibility still left
   `aj=0`. Prior Iter9 force-allow multi remained seq≠par on pevm fixtures.

## Attack landed (production = `abc-iter11`)

| Fix | Where |
|-----|--------|
| **No-warm plant original** via `sload_skip_cold_load` (else ZERO) + stock SSTORE | `boundary.rs` |
| **Jump snap: max `sstore_index` among `k < k_fail`** | `rem.rs` `build_continuation` |
| **Plant-only `first_k` pin** (gas>0 notes only; finalize gas=0 untouched) | `rem.rs` `note_write_replay` |
| Last-tip gates (memory, min tip gas, cont wr match, distinct slots) | `boundary.rs` `jump_is_safe` |
| Multi-SSTORE still **refused** (`sstore_index!=1`) until proven | `boundary.rs` |
| **Jump/capture production OFF**; stock SSTORE unless plant install wanted | `vm.rs` / Iter10 |

### Measured / rejected this iter

| Trial | Result |
|-------|--------|
| Capture with pre-sload plant | **seq≠par** Δgas=2100 — **root cause found** |
| No-warm plant + last-tip gates + `k<k_fail` + plant first_k | seq≡par when plant runs; **aj=0** (jump not actuated) |
| `SPECFENCE_ABSOLUTE_JUMP=1` trial enable capture/jump | hs>0 possible; **aj=0**; multi still unsafe to enable |
| Production jump/capture OFF (`abc-iter11`) | Soft=0; **597 N=5 med 12.7** |

## Multi-block table

### Primary: N=5 (`abc-iter11`)
| Block | SF wall med | OCC wall med | SoftWait Soft | hsstore | aj | notes |
|------:|------------:|-------------:|--------------:|--------:|---:|-------|
| **597** | **12.7** | 3.5 | **0** | 0 | 0 | ≤ Iter10 13.3 / Iter8d 13.4 |
| **599** | **20.1** | 9.7 | **0** | 0 | 0 | ~ Iter10 20.7 |
| **097** | **12.4** | 5.5 | **0** | 0 | 0 | ~ Iter10 12.4 |
| **598** | **2.1** | 1.1 | **0** | 0 | 0 | quiet OK |

### Secondary: N=10 (`abc-iter11-n10`)
| Block | SF wall med | SoftWait | aj | vs Iter10 N10 |
|------:|------------:|---------:|---:|---------------|
| **597** | **14.2** | **0** | 0 | ~ Iter10 13.8 (noise) |
| **599** | **20.6** | **0** | 0 | ~ Iter10 21.7 |
| **097** | **12.4** | **0** | 0 | ~ Iter10 12.1 |
| **598** | **2.3** | **0** | 0 | quiet OK |

`absolute_jump_applied=0`, `handler_sstore_capture=0`. SoftWait Soft **0**. No hang.

Tests: `cargo test -p pevm --lib` **97 ok**; `--test specfence` **25 ok / 13 ignored**.

## Iter 11 diagnosis (required answers)

### 1. detect / avoid / resolve — which hole now?
**Resolve still primary for <10.** Abs-jump as SuffixRepair opcode cut is **falsified for enablement** this iter (cannot prove aj>0∧seq≡par on Lean multi-SSTORE). Plant gas correctness fixed (necessary for any future capture). Avoid OK (SoftWait Soft=0). Detect OK.

### 2. Region / Fence / intra / inter
| Axis | Verdict |
|------|---------|
| **Region** | ℓ / `a` OK; tip embed + `k<k_fail` select corrected. |
| **Fence** | Abs-jump fence plumbing improved but **OFF**; SoftWait Soft dormant. |
| **Intra** | Mass path stock SSTORE; plant install gated; no-warm plant when TLS on. |
| **Inter** | Quiet\|Storm unchanged; must not drive Lean plant/jump today. |

### 3. vs Iter10 / plateau
- N=5: **12.7** ≤ Iter10 13.3 (slight win / noise).
- N=10: **14.2** ~ Iter10 13.8.
- SoftWait Soft **0**. Stretch <10 unmet; aj=0.
- Multi-SSTORE Handler abs jump **falsified for production enable**.

### 4. Cause for Iter 12 (named)
**Named cause:** Opcode-seconds on successful SuffixRepair remain because **Handler abs jump cannot be enabled** (multi-SSTORE last tip not proven aj>0∧seq≡par under pevm MV on Lean; capture→jump chain does not close hang-free). Need an **alternate resume opcode cut without abs jump**:
1. Longer sticky BO Await / serial-barrier so 2nd SuffixRepair sees Validated writers more often (cut re-exec seconds) — prior widen attempts mixed; need new evidence.
2. Or hang-free **opcode-skip credit only** (no PC restore) when certified Storage FF origin-stable covers the write-prefix (measure resume inspector_steps / wall).
3. Do **not** re-enable capture-without-jump for wall, live_prime inspect, or pre-sload plant.
4. Keep production plant install **off** unless a measured jump trial with proven seq≡par.

## Artifacts
- `lab/results/abc-iter11-sf-occ.json`, `abc-iter11-flip.json`, `abc-iter11.run.log` (N=5)
- `lab/results/abc-iter11-n10-sf-occ.json`, `abc-iter11-n10-flip.json`, `abc-iter11-n10.run.log`

## Code touched
- `boundary.rs` — no-warm plant; last-tip gates; multi still refused
- `rem.rs` — `k < k_fail` snap select; plant-only first_k
- `vm.rs` — Iter11 production jump/capture OFF
- `mod.rs` — Iter11 blurb + export `absolute_jump_env_enabled`
- `tests/specfence.rs` — Iter11 falsification smoke (jump OFF, seq≡par)
