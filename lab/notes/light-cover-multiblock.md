# Light cover × multi-block reexec→CC

**Head:** `0274bb9` off PR32 `fd6d128`  
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

`lab/results/light-cover-3356896-n7.json` (local, gitignored) · tracked copy of the multi-block table below.

## Multi-block (52 OCC-gap set)

Loaded **33/52** before 19469097 OOM (`memory allocation of ~140TB failed`). Soft=0 on every row.  
SF≤OCC wall **4/33**; SF≤OCC reuse **9/33**.  
sys_reexec blocks **15**; last arm Win_2 **9**; rare/quiet stay Opt **9**.

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

**M2:** systematic-reexec / leftover-long blocks open a light ordered arm (`Win_2` / `w_need=2`, never `n_pairs−1`). Rare-conflict stays Opt.  
Six sys-reexec rows later yield to Opt (L4 prepaid blowout), including 14689597 (`unf=336`, need=2, never stuck a cover).  
Fat `n_tx>700` Win_1 rows (13217637, 15274915) stay prepaid-heavy — hat keeps `w≤2`; not a FullChain climb.

OOM caveat: skip 19469097 on later sweeps. 18 later ids not yet run.

## Safety

iter11 **0.03s**; `erc20_independent` **0.48s**; policy 50 + admit 33; Soft=0; no mid-plant; Instant idle ↛ ĉ.

