# Light cover × multi-block reexec→CC

**Head:** `5d5265d` off PR32 `fd6d128`  
**Soft=0 · @8 · `SPECFENCE_ALL_REUSE=1` · 3 iters**

## PRIMARY 3356896 N=7 interleaved compare

| | OCC med | SF reuse | long ℓ | unfenced | double_pay | PRIMARY |
|---|---|---|---|---|---|---|
| PR31 | 0.916 | 1.147 | Opt/Defer | ~14 | 0 | false |
| PR32 | 0.908 | 1.100 | Win_16/17 | 0–1 | 0 | false |
| **this** | **0.992** | **1.184** | **Win_2 / Seg_2** (`w_need=2`) | 0–2 on cover; 12–16 on Defer | 0 after hat | **false** |

Reuse SF walls: 1.184, 1.520, 1.326, 1.117, **1.085**, **1.091**.  
Covering iters: i=2 `Win_2` unf=0 cover=1; i=5 `Seg_2` unf=2 cover=1.  
L4 Defer/Opt trials after measured light cover bring leftover 12–16 (not a half-Win nail).  
Gap vs OCC ≈0.19ms (same order as PR32) **without** `n_pairs−1` prepaid. Wall ≪ PR22 ~1.40.

3-iter reuse sweep on the same block: OCC reuse 1.329 / SF reuse **1.273** / arm `Win_1→Win_2` / unf=1 / dp=0 / cover=1 / Soft=0 — SF reuse ≤ OCC reuse on that harness.

## Multi-block (52 OCC-gap set, 51 loaded)

Loaded **51/52** (skip 19469097 OOM). Soft=0 on every row.  
SF≤OCC wall **7/51**; SF≤OCC reuse **13/51**.  
sys_reexec blocks **26**; last arm Win_2 **20**; rare/quiet stay Opt **13**.

| block | n | OCC | SF reuse | r_reuse | arm | unf | dp | sys | cover | w_need | class |
|------|---|-----|----------|---------|-----|-----|----|-------|--------|-------|-------|
| 3356896 | 176 | 1.007 | 1.273 | 0.96 | Win_1→Win_2 | 1 | 0 | 0 | 1 | 2 | reexec→CC Win_2 |
| 4864590 | 195 | 1.798 | 2.168 | 1.17 | Opt→Opt | 8 | 0 | 0 | 0 | 0 | rare/quiet |
| 5283152 | 150 | 1.396 | 1.899 | 1.25 | Full→Opt | 20 | 1 | 1 | 0 | 2 | sys then OCC yield |
| 8038679 | 237 | 1.290 | 1.636 | 1.03 | Full→Win_1 | 14 | 0 | 0 | 0 | 0 | ordered last |
| 8889776 | 330 | 2.787 | 6.245 | 2.10 | Win_1→Opt | 36 | 0 | 3 | 1 | 2 | sys then OCC yield |
| 9069000 | 56 | 2.282 | 2.886 | 1.18 | Opt→Opt | 11 | 0 | 0 | 0 | 0 | rare/quiet |
| 10760440 | 202 | 3.797 | 6.485 | 1.10 | Full→Win_1 | 64 | 0 | 0 | 0 | 0 | ordered last |
| 11114732 | 100 | 3.977 | 4.106 | 0.91 | Opt→Opt | 0 | 0 | 0 | 0 | 0 | rare/quiet |
| 11743952 | 206 | 8.905 | 8.931 | 0.78 | Opt→Opt | 11 | 0 | 0 | 0 | 0 | rare/quiet |
| 12159808 | 180 | 4.578 | 4.423 | 0.90 | Full→Full | 0 | 0 | 0 | 0 | 2 | ordered last |
| 12243999 | 205 | 3.058 | 3.778 | 1.22 | Win_1→Win_1 | 24 | 1 | 0 | 0 | 2 | ordered last |
| 12244000 | 133 | 4.862 | 5.038 | 1.00 | Full→Full | 23 | 0 | 0 | 0 | 0 | ordered last |
| 12459406 | 201 | 5.134 | 7.068 | 1.07 | Full→Opt | 69 | 0 | 1 | 0 | 2 | sys then OCC yield |
| 13217637 | 1100 | 5.498 | 72.057 | 7.31 | Win_1→Win_1 | 2 | 0 | 0 | 2 | 1 | ordered last |
| 14029313 | 724 | 4.117 | 13.244 | 3.05 | Full→Opt | 14 | 0 | 2 | 0 | 2 | sys then OCC yield |
| 14334629 | 819 | 6.260 | 29.917 | 4.29 | Win_1→Win_2 | 11 | 3 | 2 | 1 | 2 | reexec→CC Win_2 |
| 14383540 | 722 | 5.761 | 25.205 | 4.20 | Win_1→Win_2 | 9 | 0 | 1 | 1 | 2 | reexec→CC Win_2 |
| 14683600 | 660 | 68.942 | 18.233 | 0.16 | Win_1→Win_2 | 25 | 3 | 3 | 2 | 2 | reexec→CC Win_2 |
| 14689597 | 564 | 5.293 | 17.180 | 2.40 | Opt→Opt | 336 | 2 | 1 | 0 | 2 | sys then OCC yield |
| 14689598 | 111 | 1.872 | 2.623 | 1.01 | Win_1→Win_2 | 14 | 1 | 1 | 0 | 3 | reexec→CC Win_2 |
| 15199017 | 866 | 4.340 | 22.723 | 5.03 | Full→Opt | 3 | 1 | 0 | 0 | 2 | rare/quiet |
| 15274915 | 1226 | 5.326 | 113.634 | 21.24 | Win_1→Win_1 | 5 | 1 | 3 | 1 | 2 | ordered last |
| 15537394 | 80 | 1.964 | 2.362 | 0.90 | Opt→Opt | 35 | 0 | 0 | 0 | 0 | rare/quiet |
| 15538827 | 823 | 5.806 | 31.486 | 4.96 | Win_1→Win_2 | 15 | 1 | 4 | 1 | 2 | reexec→CC Win_2 |
| 15752489 | 132 | 1.890 | 2.672 | 1.22 | Full→Opt | 19 | 0 | 0 | 0 | 0 | rare/quiet |
| 16146267 | 473 | 4.049 | 13.168 | 2.84 | Win_1→Opt | 20 | 0 | 2 | 1 | 2 | sys then OCC yield |
| 16257471 | 98 | 5.220 | 5.522 | 0.90 | Full→Win_2 | 13 | 1 | 1 | 0 | 3 | reexec→CC Win_2 |
| 17034869 | 93 | 2.346 | 2.954 | 1.21 | Opt→Win_1 | 18 | 1 | 0 | 0 | 2 | ordered last |
| 17034870 | 184 | 6.730 | 8.174 | 1.12 | Full→Win_1 | 54 | 0 | 0 | 0 | 0 | ordered last |
| 17666333 | 961 | 7.916 | 37.650 | 4.44 | Win_1→Win_2 | 14 | 1 | 1 | 1 | 2 | reexec→CC Win_2 |
| 18426253 | 147 | 5.469 | 5.702 | 0.78 | Opt→Opt | 35 | 0 | 0 | 0 | 0 | rare/quiet |
| 18988207 | 186 | 4.636 | 7.053 | 1.29 | Win_1→Win_2 | 50 | 0 | 2 | 1 | 2 | reexec→CC Win_2 |
| 19426587 | 37 | 2.106 | 2.007 | 0.90 | -→- | 0 | 0 | 0 | 0 | 0 | rare/quiet |
| 19469098 | 268 | 11.681 | 23.388 | 1.57 | Win_1→Win_2 | 21 | 1 | 1 | 1 | 2 | reexec→CC Win_2 |
| 19469099 | 257 | 10.656 | 25.106 | 2.10 | Win_1→Win_2 | 9 | 0 | 3 | 2 | 2 | reexec→CC Win_2 |
| 19469101 | 469 | 16.333 | 44.673 | 2.54 | Win_1→Win_2 | 40 | 0 | 2 | 1 | 2 | reexec→CC Win_2 |
| 19505152 | 417 | 20.856 | 34.266 | 1.62 | Win_1→Win_2 | 18 | 0 | 1 | 1 | 2 | reexec→CC Win_2 |
| 19606598 | 91 | 8.016 | 4.182 | 0.43 | Full→Full | 0 | 0 | 0 | 0 | 2 | ordered last |
| 19606599 | 367 | 23.803 | 78.707 | 3.29 | Win_1→Win_2 | 41 | 2 | 1 | 1 | 2 | reexec→CC Win_2 |
| 19638737 | 381 | 11.781 | 28.175 | 1.42 | Win_1→Win_2 | 8 | 0 | 1 | 1 | 2 | reexec→CC Win_2 |
| 19716145 | 341 | 23.418 | 77.471 | 3.25 | Win_1→Win_2 | 14 | 3 | 3 | 2 | 2 | reexec→CC Win_2 |
| 19737292 | 195 | 12.123 | 19.645 | 1.11 | Full→Win_1 | 6 | 1 | 0 | 0 | 2 | ordered last |
| 19860366 | 430 | 19.325 | 55.644 | 2.74 | Win_1→Win_2 | 8 | 1 | 1 | 1 | 2 | reexec→CC Win_2 |
| 19917570 | 116 | 11.490 | 19.973 | 1.72 | Opt→Opt | 9 | 0 | 0 | 0 | 0 | rare/quiet |
| 19929064 | 103 | 7.951 | 13.521 | 1.58 | Win_1→Win_2 | 12 | 1 | 1 | 0 | 3 | reexec→CC Win_2 |
| 19932148 | 227 | 10.727 | 23.671 | 1.92 | Win_1→Win_2 | 41 | 0 | 1 | 1 | 2 | reexec→CC Win_2 |
| 19932703 | 143 | 9.282 | 13.283 | 0.00 | Full→Win_1 | 0 | 0 | 0 | 0 | 1 | ordered last |
| 19932810 | 270 | 15.866 | 39.962 | 1.68 | Win_1→Win_2 | 29 | 0 | 1 | 1 | 2 | reexec→CC Win_2 |
| 19933122 | 45 | 0.511 | 0.594 | 0.95 | -→- | 0 | 0 | 0 | 0 | 0 | rare/quiet |
| 19933597 | 154 | 11.842 | 10.163 | 0.46 | Opt→Opt | 18 | 0 | 0 | 0 | 0 | rare/quiet |
| 19934116 | 58 | 1.437 | 2.799 | 1.84 | -→- | 0 | 0 | 0 | 0 | 0 | rare/quiet |

**M2:** systematic-reexec / leftover-long blocks open a light ordered arm (`Win_2` / `w_need=2`, never `n_pairs−1`). Rare-conflict stays Opt.  
Later mainnet ids (19469098–19932810) repeat the same Opt→Win_2 loop. Six early sys-reexec rows yield to Opt (L4 prepaid), including 14689597 (`unf=336`, need=2).  
Fat `n_tx>700` Win_1 rows (13217637, 15274915) stay prepaid-heavy — hat keeps `w≤2`; not a FullChain climb.

OOM caveat: 19469097 (`memory allocation of ~140TB failed`) skipped.

## Safety

iter11 **0.01s**; `erc20_independent` **0.44s**; policy 50 + admit 33; Soft=0; no mid-plant; Instant idle ↛ ĉ.
