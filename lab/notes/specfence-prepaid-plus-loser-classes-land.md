# Detect 预付压税 × 多块输 OCC 类型 — 整包落地

**基线:** PR #33 `cursor/specfence-light-cover-mb-3352` @ `5be75bd`  
**分支:** `cursor/specfence-prepaid-losers-b5de`  
**设计:** `uploads/specfence-3356896-prepaid-plus-loser-classes-v1.md`  
**Soft=0 · 一条脊 · 无 P0/P1/P2 分期**

`CostPolicy::select_arm(ℓ)` 仍是唯一决策口。轻覆盖 + reexec→CC + S1 未闸 OCC pick 保留。

## 落地

| ID | 内容 |
|----|------|
| **P1** | `note_started` 只在本 tx 有闸或 leftover flush 排队时打点。兄弟闸不再税独立集。 |
| **P2** | Detect 洞打开（未闸，或有闸但 pred 已 Done）走 `validate_optimistic_fast` ≡ OCC。 |
| **P3** | ĉ 重叠感知：薄块 Detect 墙是 stall 工期，不是 hops×stall。U 欠覆盖才加 leftover OCC 尾。 |
| **U** | `train_hat` 允许 `w_need` 越过 oversub hat=2，**仅当 Win_2+ 仍漏**（unf≥8）。首跳 Win_1 leftover 不爬 train_hat（否则 3356896 预付 Win_8）。 |
| **O** | 已 cover_ok **或已种 covering 前缀** 不再 T3 滑 leftover Detect。安静轻 Win_2+ 即使 leftover hops 也记 cover_ok。 |
| **D** | Win_2+ 欠覆盖列车才 `double_pay` 并升 `w_need`。`hops < w_need` + OCC 尾仍禁。 |
| **L4** | sys-reexec 后抬 Opt/Defer 到 cover+δ。**cover_ok + 未测 Opt prior 不得撤**。Opt 自身测得更便宜才允许 prepaid blowout 回 OCC。 |
| **S** | n≥512 复用且 D1 已存 → 跳 HotSet / inter-prior / sketch。`train_hat` 肥块 ≤4。 |

## Compare 3356896 @8 Soft=0 N=7 interleaved

| rev | OCC med | SF reuse | long ℓ | cover unf | dp | PRIMARY |
|-----|---------|----------|--------|-----------|----|---------|
| PR33 | 0.992 | 1.184 | Win_2 / Seg_2 | 0–2 | 0 | false (gap ~0.19) |
| `8be2caa` | **0.914** | **1.240** | Win_2 / Seg_2 (Opt retreat i=4/6) | **0** on cover | **0** | **false** |

Covering path after leftover-slide knife: i=1 `Win_2` refuse=0 unf=0 wall=1.399; i=5 `Seg_2` refuse=0 unf=0 wall=**1.003**. Reuse median still includes Opt retreats after a noisy Win_2. Soft=0. No Full-spine nail. `w_need=2`.

3-iter reuse sweep on the same block: OCC reuse **1.503** / SF reuse **1.344** / ratio **0.89** — **SF reuse ≤ OCC** on that harness (same as PR33). Last arm Opt unf=15 (3-iter did not stick cover).

## Multiblock reuse sweep (`SPECFENCE_ALL_REUSE=1` N=3 @8)

52/52 loaded (19469097 no longer OOM). Soft=0 every row.

| | PR33 | this |
|---|------|------|
| loaded | 51/52 | **52/52** |
| SF≤OCC wall | 7/51 | **11/52** |
| SF≤OCC reuse | 13/51 | **16/52** |
| sys_reexec blocks | 26 | 1 |
| Win_2 last | 20 | 2 |
| Opt last | 13 quiet + others | 19 |
| dp>0 | many | **2** |
| policy / admit | 50 / 33 | **56 / 34** |

Reuse SF≤OCC **gained** 14689598, 15752489, 19737292, 19917570, 19929064, 19932148, 19934116; **lost** 11114732, 15537394, 16257471, 19933597; kept 9.

### Class deltas (same heuristic as the design table)

| class | PR33 | this | note |
|-------|------|------|------|
| **U** Win_2 + unf≥8 | 19 | **2** | 18988207 unf 50→26 need=8; most former U left Win_2 |
| **O** cover + ratio≥1.5 + unf<8 | 2 | **0** | leftover slide off on planted cover |
| **D** dp>0 | 6 | **2** | 18988207, 19469098 |
| **L4** sys+Opt+unf≥20 | 3 | **0** | 14689597 unf **336→139**, ratio **2.40→1.09** (last Full) |
| **S** n≥512 ratio≥4 | ~6 | **5** | 15274915 21.2→18.5; 13217637 7.31→7.15 — still fat |

`arm_last=Full` on fat n is the **chosen** loc telemetry (short Full still legal; long spine still cannot persist FullChain).

## Safety

- Soft=0 every compare/sweep row
- iter11 **0.02s**; erc20_independent **0.52s**
- policy **56** + admit **34**
- no mid-plant; Instant idle ↛ ĉ

```
SPECFENCE_COMPARE_ITERS=7 cargo run -p pevm --release \
  --config 'profile.release.lto=false' --example specfence_3356896_compare

SPECFENCE_ALL_REUSE=1 SPECFENCE_ALL_ITERS=3 SPECFENCE_ALL_PROCESS_TOP=0 \
  cargo run -p pevm --release --config 'profile.release.lto=false' \
  --example specfence_all_blocks_sweep
```
