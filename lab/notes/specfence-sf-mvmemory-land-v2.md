# SpecFence SfMvMemory land v2

**Date:** 2026-09-22  
**Tip:** `3da62da` on `cursor/specfence-sf-ps-true-spine-d6e8` (PR #45)  
**Baseline:** [`specfence-sf-mvmemory-land-v1.md`](specfence-sf-mvmemory-land-v1.md)  
**Harness:** Soft=0 Instant-off (`env -u SPECFENCE_HANG_TRACE`), 8 cores, `SPECFENCE_COMPARE_CHECK=1`, N=5 both  
**Acceptance:** **TPS SF/OCC ≥ 1.5** both `3356896` and `15274915`.  
**Co-author:** `0xstride <fengjy73@users.noreply.github.com>`

---

## Verdict

**NOT MET — do not call ~0.7–0.9 a soft success.** Soft=0 Instant-off N≥5 both: `seq=par`, `occ_picks=0`, `estimate_block_sf=0`, WAR first-class + four-class audit intact.

| Block | Reuse med TPS | Best iter TPS | Gap to 1.5 |
|------:|--------------:|--------------:|-----------:|
| 3356896 | **0.61** (OCC 1.54 / SF 2.55) | 0.93 cold; calm (c)=0 still ~0.71 | ~2.5× |
| 15274915 | **0.72** (OCC 10.1 / SF 13.9) | 1.02 one reuse; typical 0.64–0.72 | ~2× |

Shipped mechanisms that moved evidence (not the bar): **ChainSpineTip** (large true publish without DashMap mill), thin spin/tip tax cuts, Chain brief Released-poll. Discarded: Opt `access_log` skip (broke fail_k/prefix → `full_from_0`), long Chain busy-spin (wall↑).

---

## What changed vs v1

### Thin scaffolding tax (gap #1)

| Lever | Result |
|-------|--------|
| Scheduler-first spin `4k`, tip probe `/64` (was `131k`×DashMap) | Helps when WaitOnce races; calm (c)=0 still SF≳OCC |
| Skip redundant Version tip same inc | `early_tip` census honest |
| Ungated light `publish_data` (Released, no sketch/dag museum) | Avoid hits tip without wake museum |
| Drop dead thin `register_waiter` | Ungated finish never woke them |
| **`access_log` Opt skip** | **DISCARDED** — `full_from_0`, WaitOnce Learn dead |
| `any_wait_once` consult early-out | Kept (safe when no WaitOnce/crit) |

**Honest thin gap:** On calm reuse with WAW `(c)=0`, absolute SF−OCC is only ~0.2–0.5 ms, but OCC wall is ~1.3–1.6 ms → TPS stays ~0.7–0.9. Cutting remaining SF shell (engagement, metrics, AccessArm DashMap on every `basic`) without breaking ordinals did not reach 1.5. OCC abort train on 176-tx WAW is cheaper than SF Avoid scaffolding.

### Large Chain Avoid (gap #2)

| Lever | Result |
|-------|--------|
| **ChainSpineTip** (`bind_chain_spine` + `chain_claim`/`chain_release`) | `early_tip` ~114–191 (≲3×chain), not ~1674 |
| Crit-only; WaitOnce DashMap tip stays thin-only | Wall not back to 15ms mill from multi-ℓ tips |
| Brief Released-poll (≤512) before `park_publish_wait` | Avoids long serial park when publish is imminent |
| Four-class: reuse **chain_ab > chain_c** (e.g. 212/155, 200/135) | **(b) ahead of (c) but not ≫** |

**Honest large gap:** Chain Avoid improved vs v1 (`late≈avoid` → `ab≳c`), but FullReplay still 69–149 and SF wall ~12–14 ms vs OCC ~8–9 ms. Sticky `chain_n` still collapses mid-reuse (52→ focus print variance). Need **(b)≫(c)** (roughly 3–5×) and SF wall ≤ OCC/1.5 ≈ 5.5–6 ms.

---

## Four-class Soft=0 Instant-off N=5 @8 (tip `3da62da`)

### 3356896

| Iter | Full | (c) | waw_ab/c | war_ab/c | early_tip | est |
|-----:|-----:|----:|---------:|---------:|----------:|----:|
| 0 | 16 | 16 | 13/14 | 12/2 | 16 | 0 |
| 1 | 2 | 2 | 4/2 | 1/0 | 2 | 0 |
| 3 | **0** | **0** | **4/0** | 0/0 | 0 | 0 |
| 4 | 21 | 21 | 13/21 | 28/0 | 21 | 0 |

Calm (c)=0 still exists; TPS on those iters still ≪1.5.

### 15274915

| Iter | Full | chain_ab/c | war_ab/c | early_tip | est | note |
|-----:|-----:|-----------:|---------:|----------:|----:|:-----|
| 1 | 94 | **212/155** | 229/0 | 142 | 0 | ab>c |
| 2 | 149 | 301/272 | 314/2 | 191 | 0 | noisy |
| 3 | 90 | **204/148** | 177/0 | 137 | 0 | ab>c |
| 4 | 69 | **200/135** | 146/0 | 114 | 0 | ab>c |

WAR Avoid remains first-class (`war_ab≫war_c`). RAW low volume. `estimate_block_sf=0`.

---

## Call-graph delta (v2)

```
Thin Avoid tax cut
  consult: scheduler-first spin; no register_waiter fallthrough
  install_version_tip: skip same-inc Version rewrite
  ungated record: publish_data WaitOnce/crit only (no wake museum)
  any_wait_once: skip consult when no WaitOnce+no crit

ChainSpineTip (large sticky≥32)
  begin: bind_chain_spine(loc, writers) after plant_nearest_preds
  execute: chain_claim(writer) — AtomicU8 flag, not BTreeMap tips
  Data: publish_data → chain_release + wake_exact
  abort: chain_clear
  consult: has_released / live_writer / true_publish_ready hit flags
  brief Released-poll then park_publish_wait
```

---

## Remaining gap vs TPS ≥ 1.5 — next mechanism

**Not soft-success at 0.8.** Next mechanism (ordered):

1. **OCC-shaped thin Soft=0 kernel when PE empty / WaitOnce ℓ unused this tx**  
   Byte-identical `basic`/`storage` to OCC (no AccessArm DashMap, no engagement, no tip) except the WaitOnce ℓ consult. Preserves ordinals only for WaitOnce/crit reads (narrower than the discarded global Opt skip). Target: SF wall ≤ OCC when `(c)=0`.

2. **ChainSpine exact one-hop wake into RunnableSet without Blocking park**  
   On `chain_release`, push nearest succ to `Q_released` (plant already recorded waiters) so Avoid is schedule-publish, not park/spin. Keep antichain fill for non-succ. Goal: `chain_ab ≥ 3× chain_c` and SF wall ≤ ~6 ms on this host.

3. **Do not:** Estimate Block, thin Rewind, 15-hold, broad mark_gated, re-enable WaitOnce DashMap tip on large, skip all `access_log` notes.

---

## Invariants (held)

| Check | 3356896 | 15274915 |
|-------|:-------:|:--------:|
| seq≡par | ok | ok |
| occ_picks | 0 | 0 |
| soft_wait_arms | 0 | 0 |
| estimate_block_sf | **0** | **0** |
| WAR first-class | yes | war_ab≫war_c |
| Four-class audit | yes | yes |

---

## Discarded this land

- Opt-path global `access_log.note` skip (broke prefix / WaitOnce Learn)  
- Long Chain busy-spin (8192) before park (wall↑ to ~18 ms)  
- Large WaitOnce|crit DashMap tip plane (v1 tax)
