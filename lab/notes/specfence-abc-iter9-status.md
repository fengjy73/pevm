# SpecFence A+B+C Iter 9 status

**Date:** 2026-09-08 (Asia/Shanghai)  
**Branch:** `specfence`  
**Base tip:** `13b08f4` (Iter8)  
**Authority:** `specfence-abc-unified-protocol.md`, `specfence-abc-iter8-status.md`, `specfence-global-cc-evm-rethink.md`

## Mandate

Correct Handler-path absolute jump restore so **seq≡par with aj>0** on fixtures, then enable capture+jump on Lean SuffixRepair when `jump_is_safe`. SoftWait Soft~0; no Lean inspect/live_prime; keep Iter5–8 resolve path. Stretch: 597 median wall &lt;13.4 toward &lt;10.

## Root cause (diagnosed)

Handler memory snap (Iter8) was real, but **production jump broke seq≡par** because restore ≠ sequential certified prefix:

1. **Early post-SSTORE tip** (`sstore_index=1`, gas≈7618) armed while continuation still held **later-slot write_replays** (sticky last gas≈2370). Jump applied later presents then re-executed later SSTOREs → wrong Transfer logs / storage (from≠caller on M2).
2. **`write_prefix_jump_is_safe` skipped early-tip check** when `wr.len()==1` or plant gases empty (`post_sstore` + empty `live_gases`).
3. **`post_sstore` + `write_replays=[]`** allowed jumps past SSTORE with no journal replay (`effects_w=0` on Lean — Write effects noted only at finalize).
4. **Plant write_replay gas clobbered** at finalize (`gas_remaining_after=0` → sticky last), so tip gas could not certify which SSTORE the snap belonged to.
5. **Jump snap selection used `k ≤ cp.k` only**, so last-SSTORE tips between `cp` and `k_fail` were often invisible; early tips at lower `k` won.

## Attack landed (restore correctness)

| Fix | Where |
|-----|--------|
| Handler plant peeks SSTORE key/value, notes **per-SSTORE write_replay + tip gas** | `boundary.rs` `sstore_plant_capture_eth` |
| Preserve plant gas when finalize re-notes with 0; touch `first_k` | `rem.rs` `note_write_replay` |
| Embed **`write_replays_at_tip` + `sstore_index`** on Handler snap | `BoundarySnapshot` |
| Prefer max `sstore_index` among live snaps with **`k < k_fail`** | `rem.rs` `build_continuation` |
| Keep plant-gas replays in FF even if `first_k` late | `build_continuation` filter |
| `jump_is_safe`: refuse empty post_sstore replays; require tip embedding; refuse early tip if later plant gases exist; **Handler multi-SSTORE (`sstore_index!=1`) refused** until seq≡par | `boundary.rs` |
| Seed missing journal accounts in `apply_write_replays` (no `load_account`) | `boundary.rs` |
| `run_exec_loop` PENDING_RESUME apply kept (Iter8) | `tx_runner.rs` |

## Measured this iter

| Trial | Result |
|-------|--------|
| Broad memory-lite jump (pre-fix) | **seq≠par** on m2/p4/p1a; aj&gt;0; wrong ERC-20 Transfer topics |
| Tip-scoped / last-SSTORE multi (`sidx=2`) with embedding | still **seq≠par** on pevm fixtures (stack/MV) |
| Single-SSTORE Handler gate only | **tests green**; aj≈0 on ERC-20 clusters (2× SSTORE) |
| Production jump/capture OFF | **tests green**; SoftWait Soft=0; no hang |

## Production decision (Iter9)

**Jump OFF, capture OFF** — same as Iter8 wall posture.

- Multi-SSTORE Handler last tip still falsifies seq≡par on fixtures → cannot enable on 597 ERC-20.
- Single-SSTORE gate is hang-free + seq≡par when it fires, but **aj≈0** on 597-class bytecode (always ≥2 SSTOREs per transfer).
- Capture-without-jump remains tax (Iter8c wall↑). SoftWait Soft stays 0. No live_prime/inspect.

Restore plumbing stays compiled so Iter10 can widen the gate once multi-SSTORE seq≡par holds.

## Multi-block table

### Primary: N=5 (`abc-iter9`)
| Block | SF wall med | SoftWait Soft | hsstore | aj | notes |
|------:|------------:|--------------:|--------:|---:|-------|
| **597** | **17.4** | **0** | 0 | 0 | jump/capture OFF; noisy vs Iter8d 13.4 / Iter8 16.5 |
| **599** | **27.9** | **0** | 0 | 0 | elevated vs Iter8d 19.2 (machine noise + no aj win) |
| **097** | **18.1** | **0** | 0 | 0 | elevated vs Iter8d 11.8 |
| **598** | **3.0** | **0** | 0 | 0 | quiet OK |

### Secondary: N=10 (`abc-iter9-n10`)
| Block | SF wall med | SoftWait | aj | vs Iter8 N=10 (14.7) |
|------:|------------:|---------:|---:|----------------------|
| **597** | **19.4** | **0** | 0 | elevated; no jump actuation |
| **599** | **29.4** | **0** | 0 | elevated |
| **097** | **19.4** | **0** | 0 | elevated |
| **598** | **3.3** | **0** | 0 | quiet OK |

`absolute_jump_applied=0`, `handler_sstore_capture=0`. SoftWait Soft **0**. No hang.
Wall not improved vs Iter8d because jump stayed OFF (unsafe multi-SSTORE). SSTORE plant uses stock fast-path when `!plant_tls_active()`.

Tests: `cargo test -p pevm --lib` **97 ok**; `--test specfence` **24 ok / 13 ignored** (incl. Iter9 seq≡par smoke).

## Iter 9 diagnosis (required answers)

### 1. detect / avoid / resolve — which hole now?
**Resolve still primary for &lt;10.** Restore *diagnosis* is complete: early tip + missing/clobbered plant write_replays caused seq≠par; multi-SSTORE Handler jump still unsafe even after tip embedding. Avoid OK (SoftWait Soft=0). Detect OK. Wall plateau unchanged while jump stays OFF.

### 2. Region / Fence / intra / inter
| Axis | Verdict |
|------|---------|
| **Region** | ℓ / `a` OK; Handler tip now carries `sstore_index` + `write_replays_at_tip` at post-SSTORE `k`. |
| **Fence** | Abs-jump fence **ready but OFF** (multi-SSTORE unsafe; single scarce). SoftWait Soft dormant. |
| **Intra** | Iter5–8 resolve path unchanged; capture/jump not actuating mass path. |
| **Inter** | Quiet\|Storm unchanged; must not drive Lean plant/jump today. |

### 3. vs Iter8 / plateau
- Production posture = Iter8 (jump/capture OFF) → expect wall ≈ 13.4 N=5/10.
- Stretch &lt;10 unmet; aj=0 on 597.
- Hang-free memory snap + correct *gates* landed; enablement blocked on multi-SSTORE seq≡par.

### 4. Cause for Iter 10 (named)
**Named cause:** Opcode-seconds on successful SuffixRepair remain because **multi-SSTORE Handler abs jump is still not ≡ sequential under pevm MV** even with tip-embedded write_replays (fixtures fail when `sidx≥2` allowed). Single-SSTORE tips are safe but do not fire on ERC-20 597. Capture-without-jump is pure tax.

**Iter10 bets (falsifiable):**
1. Make multi-SSTORE last tip seq≡par — likely need certified stack/calldata binding, journal warmth for prefix SLOADs, and/or refuse jump unless `write_replays_at_tip` presents match FF Storage values origin-stable.
2. Or cut resume opcode-seconds **without** abs jump (longer sticky Await / cheaper RebindOnly) — prior widen attempts falsified; need new evidence.
3. Do **not** re-enable empty-memory jump, live_prime inspect, or capture-without-jump for wall.
4. Widen Handler gate from `sstore_index==1` only after a dedicated multi-SSTORE fixture shows aj&gt;0 + seq≡par.

## Artifacts
- `lab/results/abc-iter9-sf-occ.json`, `abc-iter9-flip.json`, `abc-iter9.run.log` (N=5)
- `lab/results/abc-iter9-n10-sf-occ.json`, `abc-iter9-n10-flip.json`, `abc-iter9-n10.run.log`

## Code touched
- `boundary.rs` — Handler plant write_replay + tip embed; jump_is_safe Iter9 gates; apply_write_replays seed
- `rem.rs` — preserve plant gas; jump snap prefer sstore_index / k&lt;k_fail; plant-gas FF filter
- `vm.rs` — production jump/capture OFF; eligibility retained
- `tx_runner.rs` — PENDING_RESUME apply (unchanged Iter8)
- `mod.rs` — Iter9 blurb
- `tests/specfence.rs` — Iter9 seq≡par smoke (jump OFF)
