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
| **P1** | Gates ≠ global mode. Ungated txs pick via OCC collaborative index (`next_task_with_wave_ready(None, ready)`). Closed gates are skip-only. |
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
