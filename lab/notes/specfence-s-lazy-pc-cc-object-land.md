# S-lazy PC/CC object correction — full land

**Branch:** `cursor/specfence-s-lazy-object-14f0`  
**Base:** PR #34 `cursor/specfence-prepaid-losers-b5de` @ `0144211` (code tip; not docs PR #35)  
**Design:** `lab/notes/specfence-s-lazy-pc-cc-object-fix-v1.md`  
**Why:** `lab/notes/specfence-pr34-k8-pc-cc-learn-analysis.md`  
**Soft=0 · one spine · no P0/P1/P2 staging**

`CostPolicy::select_arm(ℓ)` remains the only arm mouth. Lazy writer chains are **not** OrderedAdmit objects.

## Landed

| ID | Content |
|----|---------|
| **C1** | `basic_lazy` / lazy writer chains never plant OrderedAdmit. Not hot D1 ordered candidates. Sys-reexec on that ℓ does not promote Win. |
| **C2** | Real Basic / storage keep light-cover reexec→CC. Fat reuse plants promoted real spines only. S-mixed: gate the real spine, not lazy. |
| **C3** | No Full(n) on ultra-long storage. Full stays short-chain only (`n_pairs ≤ ORDER_WINDOW_K`). |
| **P1** | Gates ≠ global mode. Ungated txs pick via OCC collaborative index. Closed gates are skip-only (`next_task_with_wave_ready`). Fat + lazy already seen ignores leftover reservations. Sub-fat CallWaw / Win_2 spines that plant gates **without** ProducerStage reserve still skip-gate (not OCC-steal through holes). |
| **P2** | n≥512: soft-cap `begin_blocked` to `cores.max(8).min(16)`. Keep earliest holes (real-spine prefix). |
| **P3** | Fat + lazy already seen → lean `end_block` (skip HotSet / inter-prior / sketch). Never persist lazy writer orders. |
| **L1** | Morph gate: lazy bucket does not carry Win/Full/Seg. New lazy ℓ does not inherit 3356896 Win_2. |
| **L2** | Do not raise `w_need` / sys-reexec on lazy to kill unfenced. Lazy reward is Opt/Defer wall only. |
| **L3** | Hot sticky no-order on lazy. Cold generate is Opt/Defer only. |

## Preserve

Done-on-success / iter11; no mid-plant; Soft=0; Instant idle ↛ ĉ; no double-pay prefix+OCC tail on lazy; 3356896 Basic `0x32be` still light-cover Win_2.

## Implementation notes

- `new_promoted_seeded` must not re-enter `promoted` (callers hold `entry()` write lock).
- Full-shell EmptyTo A1 is `THIN_N_MAX < n < FAT_N` (400). Fat n≥512 EmptyTo is A0 even after envelope eff-WAW; real spines stay C2.
- `lazy_seen` resets each `begin_block` so a prior lazy block cannot lean-end a later real spine.
- `next_sf_task`: `fat_lazy || (!reserved && !pending_gated)` → OCC idx. Pending gates without reserve → skip-gate dual-path. Fat+lazy leftover reserve is **not** a mode switch. This unstuck 19469101 (n=469, 22 min livelock → 16 ms).

## Metrics (Soft=0, N=3 reuse @8 unless noted)

### K8 S-lazy (acceptance 1)

`14396881` is not in the 52-id high-bound list; run with `SPECFENCE_ALL_BLOCKS`. PR34 K8 walls from `specfence-pr34-k8-pc-cc-learn-analysis.md`.

| block | n | PR34 × | this reuse × | this wall × | warm `occ_pick_while_gated` | admit | arm |
|------:|--:|-------:|-------------:|------------:|----------------------------:|------:|-----|
| **14396881** | 1346 | **25.0** | **2.62** | 3.41 | **763** | 15 | Full→Full |
| **15199017** | 866 | **5.1** | **2.47** | 2.44 | **874** | 69 | Opt→Full |
| **13217637** | 1100 | **13.5** | **1.51** | 2.59 | **1103** | 87 | Opt→Opt |

PRIMARY lever holds: 4–25× → 1.5–2.6× reuse; warm pick_occ ≫ 0; begin holes no longer lazy-dominated (admit 15–87 on 800–1300 tx, not 95-hole prepaid).

### 52-block Soft=0 reuse (acceptance 2)

52/52 loaded. Soft=0 every row. `19932703` OCC `reuse_med` is a 2 s outlier (wall 6.7 ms); that block is scored on wall (SF 6.1 / OCC 6.7 = 0.91).

| | PR34 | this |
|---|------|------|
| loaded | 52/52 | **52/52** |
| SF≤OCC reuse | **16/52** | 12/52 |
| SF≤OCC wall | 11/52 | 10/52 |
| reuse median | — | **1.17** |
| reuse max | ≥18 (15274915) / K8 25 | **2.97** (19638737) |
| **S n≥512 ≥4×** | **5** | **0** |
| Soft | 0 | **0** |

Max and fat-tail are the corpus win. SF≤OCC *count* did not rise: leftover 2–3× sits on mid-band real spines (8889776 2.63, 16146267 2.74, 19638737 2.97, 15274915 2.91) — Detect tax, not PR24 abort trains. Prefer this PRIMARY drop over chasing the 16→12 count.

Reuse SF≤OCC: 9069000, 11114732, 11743952, 12459406, 15752489, 18988207, 19426587, 19606598, 19917570, 19932703 (wall), 19933122, 19934116.

### 3356896 (acceptance 3)

| harness | OCC | SF reuse | × | long ℓ | PRIMARY |
|---------|-----|----------|---|--------|---------|
| PR34 N=7 | 0.914 | 1.240 | 1.36 | Win_2 / Seg_2 unf=0 dp=0 | false |
| this N=7 | **0.990** | **1.330** | **1.34** | Win_8 after leftover (wall not fat-tail) | false |
| this 3-iter reuse | 1.41 | 1.52 | 1.07 | Win_2→Win_8 | — |

No fat-tail. 3-iter / N=7 can climb `w_need` to 8 on leftover; wall stays ~1.3×. Not polished further (corpus lever first).

### Real spines (acceptance 4)

| block | n | reuse × | note |
|------:|--:|--------:|------|
| 19469101 | 469 | 1.88 | hung 22 min on OCC-steal-through-gates; skip-gate dual-path → 16.8 ms, Opt, no train |
| 18988207 | 186 | 0.90 | Win_1→Win_2, SF≤OCC |
| 19505152 | 417 | 1.73 | Opt→Win_1 |
| 19606599 | 367 | 1.19 | Opt→Win_1 |

No PR24-class abort trains. Highest real-spine ratios stay under 3×.

### Safety (acceptance 5)

- Soft=0 every compare / sweep row
- lib specfence policy/admit/ready_edge/producer **119** passed
- specfence integration **44** passed / 20 ignored including **iter11**
- erc20_independent **ok**

## Commands

```
cargo test -p pevm --release --lib -- specfence::policy -- --test-threads=1
cargo test -p pevm --release --lib -- specfence::admit -- --test-threads=1
cargo test -p pevm --release --test specfence -- --test-threads=1
cargo test -p pevm --release --test erc20 -- independent -- --test-threads=1

SPECFENCE_COMPARE_ITERS=7 cargo run -p pevm --release \
  --config 'profile.release.lto=false' --example specfence_3356896_compare

SPECFENCE_ALL_REUSE=1 SPECFENCE_ALL_ITERS=3 SPECFENCE_ALL_PROCESS_TOP=0 \
  SPECFENCE_ALL_BLOCKS=14396881,15199017,13217637,3356896 \
  cargo run -p pevm --release --config 'profile.release.lto=false' \
  --example specfence_all_blocks_sweep

SPECFENCE_ALL_REUSE=1 SPECFENCE_ALL_ITERS=3 SPECFENCE_ALL_PROCESS_TOP=0 \
  cargo run -p pevm --release --config 'profile.release.lto=false' \
  --example specfence_all_blocks_sweep
```
