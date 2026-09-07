# SpecFence A+B+C Iter 10 status

**Date:** 2026-09-08 (Asia/Shanghai)  
**Branch:** `specfence`  
**Base tip:** `62c9cce` (Iter9)  
**Authority:** `specfence-abc-unified-protocol.md`, `specfence-abc-iter9-status.md`, `specfence-global-cc-evm-rethink.md`

## Mandate

Iter9 **regressed wall** (597 N10 **19.4** vs Iter8d **13.4**) with jump/capture still OFF. First restore wall <=~13.4 SoftWait Soft=0 by finding/fixing Iter9 accidental hot-path tax. Then attack Iter10 cause (multi-SSTORE jump == seq **or** cut resume opcode-seconds without abs jump). Stretch <10.

## Root cause (Iter9 wall regression)

With jump/capture OFF, Iter9 still taxed the mass path:

1. **Fat Handler SSTORE wrap always installed** — every SSTORE paid TLS check through a large plant body (peek/sload/write_replay/tip embed) even when `plant_tls` never armed.
2. **`note_write_replay` first_k-from-gas** — finalize notes with filled tip gas wrote `first_k`, shifting true_suffix / RebindOnly vs Iter8 journal-only `first_k`.
3. **`build_continuation` keep-all `gas>0` write_replays** — plant-gas filter widened FF write sets vs Iter8 (repair-path; kept out of production mass path this iter).

Machine noise amplified Iter9 numbers (OCC@597 also ~3.6->5.5), but (1)+(2) are real accidental tax/behavior vs Iter8d.

## Attack landed (production = `abc-iter10`)

| Fix | Where |
|-----|--------|
| **Stock SSTORE unless plant wanted** | `ethereum.rs` + `handler_sstore_plant_install_wanted()` — install only for `SPECFENCE_HANDLER_CAPTURE` / `ABSOLUTE_JUMP=1` / research inspect |
| Thin SSTORE + `#[cold]` plant body | `boundary.rs` `sstore_plant_capture_eth` |
| Revert `first_k`-from-gas; keep plant-gas preserve only | `rem.rs` `note_write_replay` |
| Revert keep-all `gas>0` write_replays filter | `rem.rs` `build_continuation` |
| Keep Iter9 tip embed + jump_is_safe gates (OFF) | `boundary.rs` / `vm.rs` |

### Measured / rejected this iter

| Trial | Result |
|-------|--------|
| Tax-strip only (`abc-iter10`) | **597 N=5 med 13.3** Soft=0; **N=10 med 13.8** Soft=0 — **restored <= Iter8d** |
| Value-stable journal FF Basic+Storage | **livelock** on Iter9 smoke under concurrency — **falsified** |
| Storage-only value-stable FF (`abc-iter10b`) | N=5 **12.8** tease; N=10 **18.2 up** resume/fb up — **falsified** |
| Multi-SSTORE Handler abs jump | not enabled (still seq!=par; left for Iter11) |

## Multi-block table

### Primary: N=5 (`abc-iter10`)
| Block | SF wall med | OCC wall med | SoftWait Soft | hsstore | aj | notes |
|------:|------------:|-------------:|--------------:|--------:|---:|-------|
| **597** | **13.3** | 3.8 | **0** | 0 | 0 | restored vs Iter9 17.4 / ~ Iter8d 13.4 |
| **599** | **20.7** | 9.6 | **0** | 0 | 0 | ~ Iter8d 19.2 |
| **097** | **12.4** | 5.7 | **0** | 0 | 0 | ~ Iter8d 11.8 |
| **598** | **2.1** | 1.2 | **0** | 0 | 0 | quiet OK |

### Secondary: N=10 (`abc-iter10-n10`)
| Block | SF wall med | SoftWait | aj | vs Iter9 N10 / Iter8d N10 |
|------:|------------:|---------:|---:|---------------------------|
| **597** | **13.8** | **0** | 0 | **down vs 19.4**; ~ Iter8d 13.4 (noise) |
| **599** | **21.7** | **0** | 0 | down vs Iter9 29.4 |
| **097** | **12.1** | **0** | 0 | down vs Iter9 19.4 |
| **598** | **2.1** | **0** | 0 | quiet OK |

`absolute_jump_applied=0`, `handler_sstore_capture=0`. SoftWait Soft **0**. No hang on 597/599 benches.

Tests: `cargo test -p pevm --lib` **97 ok**; `--test specfence` **24 ok / 13 ignored** (solo). Parallel suite has rare schedule flakes (`m3_prior_bind` assert; occasional Iter9 smoke stall under load) — not reproduced solo; same family as prior iters.

## Iter 10 diagnosis (required answers)

### 1. detect / avoid / resolve — which hole now?
**Resolve still primary for <10.** Wall regression was **hot-path tax**, not resolve progress — fixed. Opcode-seconds on SuffixRepair remain (aj=0; capture tax; multi-SSTORE jump unsafe). Avoid OK (SoftWait Soft=0). Detect OK.

### 2. Region / Fence / intra / inter
| Axis | Verdict |
|------|---------|
| **Region** | ell / `a` OK; tip embed retained for future jump. |
| **Fence** | SoftWait Soft dormant; abs-jump fence OFF; BO 2nd-repair Await kept. |
| **Intra** | Mass path stock SSTORE again; plant install gated. |
| **Inter** | Quiet|Storm unchanged; must not drive Lean plant/jump today. |

### 3. vs Iter8 / Iter9 / plateau
- N=5: **13.3** <= Iter8d 13.4; much less than Iter9 17.4.
- N=10: **13.8** much less than Iter9 19.4; ~ Iter8d 13.4.
- SoftWait Soft **0**. Stretch <10 unmet.
- Value-stable FF without abs jump **falsified** (livelock / N10 wall up).

### 4. Cause for Iter 11 (named)
**Named cause:** Successful SuffixRepair still re-executes certified-prefix interpreter-seconds because **multi-SSTORE Handler abs jump is still not == sequential under pevm MV**, and **no hang-free value-stable FF / RebindOnly widen** cut those seconds without wall up.

**Iter11 bets (falsifiable):**
1. Multi-SSTORE last tip seq==par — certified stack/calldata + journal warmth for prefix SLOADs; refuse unless tip `write_replays_at_tip` matches FF Storage origin-stable; then enable capture+jump (`SPECFENCE_HANDLER_CAPTURE=1` path already gated).
2. Cheaper resume without abs jump only with **new** evidence (prior value-stable FF / RebindOnly widen / Estimate park falsified).
3. Do **not** re-enable capture-without-jump, live_prime inspect, or keep-all gas write_replays on mass path.
4. Keep production plant install **off** unless enabling jump/capture for a measured trial.

## Artifacts
- `lab/results/abc-iter10-sf-occ.json`, `abc-iter10-flip.json`, `abc-iter10.run.log` (N=5 production)
- `lab/results/abc-iter10-n10-sf-occ.json`, `abc-iter10-n10-flip.json`, `abc-iter10-n10.run.log`
- Falsified: `abc-iter10b-*` (Storage value-stable FF)

## Code touched
- `boundary.rs` — thin/cold SSTORE; `handler_sstore_plant_install_wanted`; Iter9 gates retained
- `chain/ethereum.rs` — conditional plant install
- `rem.rs` — note_write_replay first_k revert; write_replays filter restore
- `vm.rs` — Iter10 production comment; jump/capture OFF
- `mod.rs` — Iter10 blurb
