# SpecFence SfMvMemory land v3

**Date:** 2026-09-22  
**Tip:** `168a1b3` on `cursor/specfence-sf-ps-true-spine-d6e8` (PR #45)  
**Baseline:** [`specfence-sf-mvmemory-land-v2.md`](specfence-sf-mvmemory-land-v2.md) (`0bb7548`)  
**Harness:** Soft=0 Instant-off (`env -u SPECFENCE_HANG_TRACE`), 8 cores, `SPECFENCE_COMPARE_CHECK=1`, N=5 both  
**Acceptance:** **TPS SF/OCC ≥ 1.5** both `3356896` and `15274915`.  
**Co-author:** `0xstride <fengjy73@users.noreply.github.com>`

---

## Verdict

**NOT MET — not a soft success below 1.5.** Soft=0 Instant-off N≥5 both: `seq=par`, `occ_picks=0`, `estimate_block_sf=0`, four-class + WAR first-class held.

| Block | v1 TPS | v2 TPS | **v3 reuse med TPS** | vs ≥1.5 |
|------:|-------:|-------:|---------------------:|:-------:|
| 3356896 | ~0.76 | 0.61 | **0.71** | gap ~2.1× |
| 15274915 | ~0.48 | 0.72 | **0.70** | gap ~2.1× |

Thin recovered from v2 regression (0.61→0.71) via OCC-shaped kernel; still below v1 best calm. Large holds ~0.70 with ChainSpine one-hop; `chain_ab/c` ≈ **1.5–1.7×** (target ≥3× not met).

---

## Levers verified this land

### 1. OCC-shaped thin kernel (`sf_occ_shaped`)

**When:** thin Soft=0 ∧ `!has_wait_once_peer_before(tx)`.

| Skip | Keep |
|------|------|
| `consult_ungated_wait_once` body | **all** `access_log.note` (fail_k/prefix) |
| `engagement.begin_tx` atomics (force lean) | tip install if `is_wait_once_producer` |
| `note_execute_edge` museum | WaitOnce consumer full Avoid path |

**Result:** calm reuse `(c)=0` iters return (focus[2]/3]); reuse med 0.71 vs v2 0.61. Absolute SF−OCC on calm still ~0.5 ms on ~1.5 ms OCC wall → TPS ~0.7. Remaining tax is ordinals + set_tx + finish shell — not consult.

**Discarded again:** Opt `access_log` skip (v2 autopsy).

### 2. ChainSpine one-hop → `Q_released` (no Aborting Blocking)

Merged from `cursor/chainspine-one-hop-wake-cbb6`:

```
chain_release / publish_data
  → wake_exact → wave.push_ready
  → ready_edges.wake_planted_on_publish → wave.push_ready(succ)
drain_wave → runnable.wake_idle(succ, Q_released)
consult: schedule-defer Soft=0 (recover_executing, no add_dependency Aborting)
```

| Iter | chain_ab/c | ratio | early_tip | est |
|-----:|-----------:|------:|----------:|----:|
| 1 | 185/110 | 1.68× | 107 | 0 |
| 2 | 224/185 | 1.21× | 148 | 0 |
| 3 | 171/115 | 1.49× | 116 | 0 |
| 4 | 185/113 | 1.64× | 108 | 0 |

**Result:** `(b)>(c)` sustained; **not ≥3×**. SF wall ~12–17 ms vs OCC ~8–10 ms. Sticky `chain_n` still collapses (62→4–5 in focus print). One-hop wake helps Avoid counts; wall still dominated by FullReplay 50–99 + Rewind.

---

## Soft=0 Instant-off N=5 @8 (tip `168a1b3`)

### 3356896

| Iter | OCC | SF | TPS | (c) | waw_ab/c | est |
|-----:|----:|---:|----:|----:|---------:|----:|
| 0 | 3.50 | 3.30 | 1.06 | 23 | 20/21 | 0 |
| 1 | 1.90 | 2.11 | 0.90 | 1 | 2/1 | 0 |
| 2 | 1.67 | 2.41 | 0.69 | **0** | **2/0** | 0 |
| 3 | 1.54 | 2.07 | 0.74 | **0** | **2/0** | 0 |
| 4 | 1.56 | 2.69 | 0.58 | 17 | 8/17 | 0 |

Reuse med TPS **0.71**. Calm (c)=0 still SF≳OCC.

### 15274915

| Iter | OCC | SF | TPS | Full | chain_ab/c | war_ab/c | est |
|-----:|----:|---:|----:|-----:|-----------:|---------:|----:|
| 0 | 10.12 | 12.07 | 0.84 | 36 | 0/0 | 53/2 | 0 |
| 1 | 9.49 | 12.34 | 0.77 | 50 | 185/110 | 134/0 | 0 |
| 2 | 9.30 | 16.86 | 0.55 | 99 | 224/185 | 215/1 | 0 |
| 3 | 8.79 | 13.54 | 0.65 | 60 | 171/115 | 195/0 | 0 |
| 4 | 8.59 | 13.45 | 0.64 | 50 | 185/113 | 183/1 | 0 |

Reuse med TPS **0.70**. WAR `war_ab≫war_c` intact.

---

## Invariants (held)

| Check | 3356896 | 15274915 |
|-------|:-------:|:--------:|
| seq≡par | ok | ok |
| occ_picks | 0 | 0 |
| soft_wait_arms | 0 | 0 |
| estimate_block_sf | **0** | **0** |
| WAR first-class | yes | yes |
| Four-class RAW/WAR/WAW/Chain | yes | yes |

---

## Honest remaining gap vs ≥1.5

**Thin:** Even with OCC-shaped skip on non-consumer txs, calm `(c)=0` leaves ~0.5 ms SF shell on ~1.5 ms OCC. To hit 1.5 need SF ≲ OCC/1.5 ≈ 1.0 ms — **cut ~50% of remaining SF wall**. Next candidates (not shipped):

1. **Per-tx ordinal strip** only for WaitOnce/crit ℓ (not global Opt skip) — risk: prefix on unexpected conflict ℓ  
2. **Bypass SpecFence finish museum** on ungated OCC-shaped (no HotSet/rw_prior walks)  
3. Accept thin may need **real parallelism win** (more antichain) not just tax cut — WAW chain of ~17 on 176 txs limits SF≪OCC

**Large:** Chain Avoid ratio stuck ~1.5–1.7×; wall ~1.5× OCC. Next:

1. **Hold sticky spine length** (crit_n collapse 62→5 kills antichain + tip quality)  
2. **Coalesce Chain FullReplay** into prefix Rewind when `chain_released(pred)` at validate  
3. **Producer-done one-hop earlier** (Version claim → schedule defer succ without Data wait when OrderedTip safe) — high correctness risk

---

## Discarded (still)

Estimate Block, thin Rewind, 15-hold, mark_gated, long Chain busy-spin, Opt `access_log` skip.

---

## Call-graph (v3 delta)

```
Thin OCC-shaped (sf_occ_shaped)
  set_tx: !has_wait_once_peer_before(tx) ∧ n≤176
  consult → Ok (ordinals already noted)
  !producer → skip tip install / note_execute_edge
  lean without engagement.begin_tx

ChainSpine one-hop
  publish_data / chain_release
    → wake_exact + wake_planted_on_publish → wave
  drain_wave → Q_released (always for wave bag)
  Soft=0 Blocking on chain → recover_executing + Blocked (no Aborting)
```
