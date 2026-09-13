# SpecFence A+B+C Iter 8 status

**Date:** 2026-09-08 (Asia/Shanghai)  
**Branch:** `specfence`  
**Base tip:** `291580d` (Iter7)  
**Authority:** `specfence-abc-unified-protocol.md`, `specfence-abc-iter7-status.md`, `specfence-global-cc-evm-rethink.md`

## Mandate

Cut remaining **successful SuffixRepair opcode-seconds** on 597 (wall clearly below ~13 toward &lt;10). SoftWait Soft~0; no Lean inspect/live_prime; no empty-memory abs jump; keep Iter5–7 head-FF / RebindOnly / extra SuffixRepair / 2nd-repair BO Await.

## Attack chosen (evidence-backed)

### Landed (production = `abc-iter8d`)
1. **Hang-free Handler memory snap plumbing** — `sstore_plant_capture_eth` clones memory when ≤8KiB under `plant_tls` (WaitHard demoted). Cap avoids large-clone hang family.
2. **Handler `run_exec_loop` PENDING_RESUME apply** — absolute jump apply without `inspect_run` / `initialize_interp`.
3. **Production jump/capture OFF** — gates retained (`suffix_jump_eligible` + memory-lite); arming left false after falsification (below). SoftWait Soft=0. Iter5–7 resolve path unchanged.

### Measured / rejected this iter
| Trial | Result |
|-------|--------|
| Broad memory-lite Handler jump (non-empty memory) | **seq≠par** on bayes/m2/p2/p4 fixtures — **falsified** |
| Capture window ON (`needs_capture` plant TLS) without jump (`abc-iter8c`) | **hsstore>0 hang-free** (597 N=5 hs≈1324) but wall **14.0↑** vs Iter7 13.2 (tax, aj=0 — ERC-20 bytecode ≫256) |
| Tiny≤256 + memory + write_replays jump | tests green; **aj=0** on 597/599 (no eligible snaps) |
| Certified-prefix-only RebindOnly + spin 72 (`abc-iter8`) | 597 N=5 wall **16.5** p90 **72** — **falsified** |
| First-repair Estimate BO park (`abc-iter8b`) | sra↑ (57 vs 24); wall **14.9↑** — **falsified** |

## Multi-block table

### Primary: N=5 (`abc-iter8d`)
| Block | SF wall med | OCC wall med | SoftWait Soft | SF aborts med | full_restart (last) | fb_reabort (last) | sra (last) | resume (last) | vs Iter7 N=5 |
|------:|------------:|-------------:|--------------:|--------------:|--------------------:|------------------:|-----------:|--------------:|--------------|
| **597** | **13.4** | ~3.7 | **0** | 141 | **39** (vs 45) | **80** (vs 90) | 25 | **108** (vs 138) | ≈ wall; resume/fb/fr↓ |
| **599** | 19.2 | ~10.6 | **0** | — | 52 | 83 | 17 | 97 | ≈/↓ wall |
| **097** | 11.8 | ~6.2 | **0** | — | 56 | 91 | 17 | 99 | ↓ vs 12.4 |
| **598** | 2.2 | ~1.3 | **0** | 14 | 3 | 4 | 0 | 6 | quiet OK |

### Secondary: N=10 (`abc-iter8d-n10`)
| Block | SF wall med | SoftWait | aborts med | full_restart (last) | fb_reabort (last) | sra | vs Iter7 N=10 |
|------:|------------:|---------:|-----------:|--------------------:|------------------:|----:|---------------|
| **597** | **13.4** | **0** | 158 | **33** (vs 38) | **70** (vs 90) | 18 | **↓ wall; fr/fb↓** |
| **599** | 20.9 | **0** | 221 | 57 | 109 | 33 | ≈/slight↑ |
| **097** | 12.8 | **0** | 210 | 59 | 98 | 19 | ↓ vs 13.6 |
| **598** | 2.3 | **0** | 16 | 2 | 4 | 0 | quiet OK |

`absolute_jump_applied=0`. SoftWait Soft **0**. No hang on 597/599.  
Hang-free snap proof (non-production): `abc-iter8c` hsstore≫0, aj=0, no hang.

Tests: `cargo test -p pevm --lib` **97 ok**; `--test specfence` **23 ok / 13 ignored**.

## Iter 8 diagnosis (required answers)

### 1. detect / avoid / resolve — which hole now?
**Resolve still primary for &lt;10**, but shape clarified. Hang-free **non-empty memory snap on Handler path is real** (hsstore under capture_window; WaitHard demote; no inspect_run hang). Applying that snap as abs jump still **breaks seq≡par** on multi-tx fixtures (restore ≠ sequential certified prefix under pevm MV) — so production cannot spend the snap yet. Capture without jump is pure tax (wall↑). RebindOnly remains scarce on 597 true_suffix value-changing RAW (rb≈0–2). N=10 wall **13.4↓** vs Iter7 13.9 with jump/capture OFF (schedule/noise + Iter7 sticky path); stretch &lt;10 unmet.  
**Avoid** OK (SoftWait Soft=0; Iter7 2nd-repair BO kept; first-repair Estimate park falsified).  
**Detect** OK.

### 2. Region / Fence / intra / inter
| Axis | Verdict |
|------|---------|
| **Region** | ℓ / `a` OK; memory snap attaches at post-SSTORE `k` when plant_tls on. |
| **Fence** | BO Await at 2nd repair kept; SoftWait Soft dormant; abs-jump fence **ready but OFF**. |
| **Intra** | Hot sticky + second_repair prefer_await; capture/jump not actuating mass path. |
| **Inter** | Quiet\|Storm unchanged; must not drive Lean plant/jump today. |

### 3. vs Iter7 / plateau
- N=5: **13.4** ≈ Iter7 13.2; resume/fb/fr↓ on last draw.  
- N=10: **13.4** **↓** vs Iter7 13.9; fr/fb↓.  
- SoftWait Soft **0**. No hang. aj=0. Stretch &lt;10 unmet.  
- Hang-free memory snap **proven** off-path (`abc-iter8c`); jump restore still wrong for ERC-20 scale.

### 4. Cause for Iter 9 (named)
**Named cause:** Opcode-seconds on successful SuffixRepair remain because **Handler abs jump restore is not ≡ sequential** once memory+stack+write_replays are applied (fixtures fail seq≡par; 597 never gets tiny-bytecode eligible snaps). Capture-without-jump only adds plant tax. RebindOnly cannot absorb 597 value-changing fan-out RAW.

**Iter9 bets (falsifiable):**
1. **Correct Handler jump restore** — journal/read-origin/seed for ERC-20-scale frames with non-empty memory + write_replays so `specfence` seq≡par with **aj&gt;0**, then enable capture_window+jump on Lean SuffixRepair (cut resume opcode-seconds). Start from fixture suite before 597 N≥5.
2. **Post-SSTORE jump that refuses nonzero memory_words** is still forbidden (empty-memory falsified); do not reopen empty-memory.
3. Do **not** re-enable certified-prefix-only RebindOnly widen, first-repair Estimate park, or live_prime inspect without wall+seq proof.
4. Optional: cheaper resume without jump only if it cuts critical-path opcode (not just hsstore counters).

## Artifacts
- `lab/results/abc-iter8d-sf-occ.json`, `abc-iter8d-flip.json`, `abc-iter8d.run.log` (N=5 production)
- `lab/results/abc-iter8d-n10-sf-occ.json`, `abc-iter8d-n10-flip.json`, `abc-iter8d-n10.run.log`
- Hang-free snap proof: `abc-iter8c-*` (capture ON, aj=0, hsstore≫0)
- Falsified: `abc-iter8` (RebindOnly widen), `abc-iter8b` (first-repair Estimate park)

## Code touched
- `boundary.rs` — Handler SSTORE plant memory clone (≤8KiB cap)
- `tx_runner.rs` — `run_exec_loop` applies `PENDING_RESUME` without inspect_run
- `vm.rs` — memory-lite eligibility retained; production `suffix_jump=false`, `capture_window=false`
- `mod.rs` — resolve blurb Iter8
