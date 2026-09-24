# Instant-off — remaining wall after S1–S5 shell cut

**Sweep head:** `2add58d` · Soft=0 · Instant-off N=5 @8 (`specfence_3356896_compare`, `SPECFENCE_COMPARE_BLOCK`)
**Sweep:** [`specfence-shell-cut-then-redig-sweep.md`](specfence-shell-cut-then-redig-sweep.md)
**Three-lens:** [`specfence-shell-cut-then-redig-pc-cc-learn.md`](specfence-shell-cut-then-redig-pc-cc-learn.md)
**Raw:** `lab/results/instant-off/<block>.json` (gitignored)
**K:** worst remaining TPS after the 99-block Soft=0 sweep: `13287210`, `19807137`, `6196166`, `3356896`, `4330482`, `14334629`

Primary wall = SF reuse median (iters 1..4) vs OCC median. Instant-tax (`yield_ns` / `worker_busy_ns` / `idle_core_ns`) is **not** in the wall. Soft=0 every iter. Host `nproc=4`, request 8 cores (same as the sweep).

No block is `sf_le_occ`. Soft=0 on every row.

## Total table

| block | n | sweep SF/OCC | OCC med | SF cold | SF reuse | Instant × | arms | cover | unf | end µs | class |
|------:|--:|-------------:|--------:|--------:|---------:|----------:|------|------:|----:|-------:|-------|
| **13287210** | 1414 | 0.293 | 5.992 | 9.650 | **22.571** | **3.77** | Opt×5 | 0 | 2–5 | 435–492 | OCC-quiet reuse jitter |
| **19807137** | 712 | 0.593 | 18.186 | 23.784 | **23.670** | **1.30** | Opt×5 | 0 | 595–600 | 504–598 | under-covered storage spine |
| **6196166** | 108 | 0.629 | 1.863 | 2.745 | **2.656** | **1.43** | Win_2→Opt→Win_8→Defer→Win_8 | 0–4 | 73–82 | 76–261 | thin window mill |
| **3356896** | 176 | 0.691 | 1.057 | 1.259 | **1.189** | **1.13** | Win_1→Win_2→Opt→Defer→Win_8 | 0–1 | 14–16 | 51–67 | thin light-cover climb |
| **4330482** | 237 | 0.703 | 1.241 | 1.564 | **1.569** | **1.26** | Opt→Win_1×4 | 0–2 | 10–13 | 50–166 | mid leftover Win_1 plant |
| **14334629** | 819 | 0.713 | 7.229 | 8.588 | **8.466** | **1.17** | Opt×5 | 0 | 33–62 | 433–583 | large leftover Detect |

Sweep ratio and Instant-off ratio **disagree in magnitude** on every real spine except the OCC-quiet outlier. Instant-off is the primary wall. Sweep win-rate stays the north star for corpus count.

---

## 13287210 — OCC-quiet reuse jitter (do not chase)

| OCC walls | 5.99, 20.98, 10.22, 4.05, 4.32 | med 5.99 |
| SF walls | 9.65, 5.18, 22.57, 38.94, 5.25 | reuse med 22.57 |
| Soft | 0 | refuse 0 · wait_dep 0 · covering 0 · wait-set 0 |

- Arm: Opt×5. No `selected_arms`. `edge_4_31=true` every iter. `off_edge_inc` is a handful of txs (`1,2,27,286,419`).
- OCC abort 0–2. SF abort 1–2. unfenced 2–5. reexec 2–5. `end_block` 435–492 µs is the **whole** `opt_path_tax` (lean walk still ~0.45 ms on n=1414).
- SF reuse is not a mode: 5.18 and 38.94 sit in the same reuse series. OCC also swings 4.05–20.98. Sweep OCC ~4.4 ms vs PR42 ~22 ms is the same class.
- **PC:** wait-set 0, ungated path already OCC-shaped, no OrderedAdmit. Residual is reuse jitter + ~0.45 ms end_block, not a leftover gate.
- **Do not:** plant a cover, lean thin `end_block`, or treat 3.77× as a lazy Full.

---

## 19807137 — under-covered storage spine (Instant-off Opt, not sweep Full/2)

| OCC walls | 2492, 19.05, 18.19, 14.74, 17.34 | med 18.19 (cold OCC 2.5 s is first-iter noise) |
| SF walls | 23.78, 24.86, 18.77, 20.63, 23.67 | reuse med 23.67 |
| Soft | 0 | refuse 0 · covering 0 · wait-set 0 |

- Instant-off arm is **Opt×5**, `selected_arms` empty, `covering_n=0`, `win_w=0`. Sweep Full/2 is a **PROFILE / ĉ leak**, not the Instant-off object.
- unfenced 595–600 every iter. reexec 2008–4424. SF occ_aborts 1055–1560 vs OCC 729–1466. `off_edge_inc` ~588–591 txs. `abort_cf_ns` 29–49 ms (signal, not added to the wall).
- `end_block` 504–598 µs. `opt_path_tax` equals `end_block` (no ordered prepaid).
- Instant-off **1.30×** is much closer to OCC than the sweep 1.69× (SF 25.1 / OCC 14.9). Instant-off OCC median is slower (18.2 vs 14.9); SF is slightly faster (23.7 vs 25.1). The structural gap is ~5 ms of Detect/abort on a 571-writer storage loc that cover_window cannot absorb.
- **CC:** sticky Opt is the right object. Full/Seg on a 2-pair loc (sweep) is the wrong object. Cover cannot eat L≫64.
- **Learn:** Instant-off does **not** invite Full. Sweep still can. ĉ must not morph a 2-pair loc into Full while the 571-writer spine stays leftover-long.
- **Next cut:** keep Instant-off Opt = OCC abort; forbid sweep/PROFILE Full leak on this loc. Do not plant Win_2.

---

## 6196166 — thin window mill (worst *real* Instant-off after 19807137)

| OCC walls | 2.60, 1.79, 1.86, 1.57, 1.87 | med 1.86 |
| SF walls | 2.75, 2.66, 2.57, 2.56, 3.10 | reuse med 2.66 |
| Soft | 0 | n=108 |

- Arms: `Win_2 → Opt → Win_8 → Defer → Win_8`. `w_cap=8`, `cover_window=8` every iter. covering 3, 4, 1, 0, 2.
- `selected_arms` last iter: `Win_8/48 + Win_8/30 + Win_5/9 + Win_4/6 + Win_4/6 + Full/2 + Full/1`. That is over-wide OrderedAdmit on a 108-tx block.
- unfenced 73–82. reexec 166–201. OCC abort ~83–108; SF abort 90–117. `train_hat` is climbing where OCC abort is already cheap.
- ungated_occ swings 0 / 184 / 169 / 29 / 230 as the plant appears and disappears. `edge_OA` 0→30 on the last Win_8.
- Instant-off **1.43×** matches the sweep class (0.629). This is the live learn leak, not a missing path-tax skip.
- **Next cut:** nail thin `train_hat` to Win_2. Do not learn Win_8 on n≤176.

---

## 3356896 — thin light-cover; Instant-off already 1.13×

| OCC walls | 1.50, 1.00, 1.09, 0.78, 1.06 | med 1.06 |
| SF walls | 1.26, 1.23, 1.19, 1.07, 1.18 | reuse med 1.19 |
| Soft | 0 | covering 1/0/0/0/1 · wait-set from sweep = 3 |

- Arms: `Win_1 → Win_2 → Opt → Defer → Win_8`. `w_cap` 2→8. `cover_window` 0→8. Last iter `Win_8/14`.
- commute 77 every iter (the historical 3356896 commute set). `opt_maj=true`. refuse 0–7. ungated_occ 21–54. `edge_4_31=false` (D1 4→31 still walks on first incarnation; not this reuse series).
- Instant-off **1.13×** is much closer to OCC than the sweep 0.691 (1.45×). Sweep N=3 landed on `Win_8/14` as the median object; Instant-off reuse median still includes Opt/Defer iters at 1.07–1.19 ms.
- The climb `Win_2 → Win_8` is real (`train_hat` cap 8 *is* the mill). It does not dominate Instant-off the way the sweep suggested.
- **Next cut:** same as 6196166 — nail thin hat to Win_2. Do not drop D1 / HotSet (mixed_hot / p1a).

---

## 4330482 — mid leftover Win_1 plant

| OCC walls | 1.82, 1.25, 1.24, 1.07, 1.05 | med 1.24 |
| SF walls | 1.56, 1.69, 1.57, 1.45, 1.54 | reuse med 1.57 |
| Soft | 0 | covering 0→2 |

- Cold is Opt + `Full/2`. Reuse plants `Win_1/28` then `Win_1/23`. `w_cap=2`. `cover_window=0` (Win_1, not a grown cover).
- unfenced 10–13. reexec 18–29. ungated_occ 236–255 (≈ n). This is a **small** leftover plant, not a 65-block ungated shell.
- Instant-off **1.26×** vs sweep 1.42×. Residual ~0.33 ms is prepaid/refuse on the two Win_1 locs plus `end_block` 50–166 µs.
- **CC:** first-block Opt leftover became a reuse Win_1. Safer than a T3-slide hang; still not a measured one-shot cover.
- **Next cut:** one-shot measured cover that cannot leftover-slide. Do not restart Win_2 on every mid reuse (19469101).

---

## 14334629 — large leftover Detect (already the right object)

| OCC walls | 7.24, 7.23, 6.29, 13.33, 6.30 | med 7.23 |
| SF walls | 8.59, 9.08, 7.51, 6.64, 8.47 | reuse med 8.47 |
| Soft | 0 | covering 0 · wait-set 0 |

- Opt×5. `selected_arms` empty in compare (sweep recorded `Opt/485`). unfenced 33–62. reexec 46–108. SF abort 24–49 vs OCC 27–49.
- `opt_path_tax` = `end_block` 433–583 µs. `abort_cf_ns` 3.4–9.5 ms (Detect signal). `storage_inc` has `14` (and sometimes `17`). `main_inc` is `4` / `103` / `171`.
- Instant-off **1.17×** vs sweep 1.40×. Residual is validate/abort on a leftover-long loc, not a wait-set plant.
- **Do not:** cover-probe this loc. Cap-0 already dropped the wait-set. Next residual is Detect cost, not another `skip_ungated_*`.

---

## Instant-off vs sweep (honest)

| block | sweep × | Instant × | who is the wall | why they disagree |
|-------|--------:|----------:|-----------------|-------------------|
| 13287210 | 3.41 | **3.77** | Instant-off reuse jitter | OCC and SF both swing 4–39 ms; not a policy object |
| 19807137 | 1.69 | **1.30** | Instant-off Opt Detect | Sweep Full/2 is ĉ/PROFILE; Instant-off stays Opt |
| 6196166 | 1.59 | **1.43** | both — window mill | Instant-off confirms Win_8 climb |
| 3356896 | 1.45 | **1.13** | Instant-off is near OCC | Sweep median hit Win_8; Instant-off median did not |
| 4330482 | 1.42 | **1.26** | Instant-off Win_1 plant | Same class, smaller ms |
| 14334629 | 1.40 | **1.17** | Instant-off leftover Detect | Same object, quieter OCC |

Sweep win-rate (26/98) remains the corpus north star. Instant-off says the **next policy cut** is not another ungated shell skip:

1. **Thin `train_hat` nail to Win_2** (6196166 Instant 1.43×; 3356896 climb still real).
2. **Forbid Full/Seg on 19807137-class leftover-long** in the sweep/PROFILE path (Instant-off already Opt).
3. **One-shot measured mid cover that cannot T3-slide** (4330482 Win_1; 8889776 / 19469101 from the sweep).

Do **not**: another `skip_ungated_tx_path_tax`; lean thin `end_block`; drop large live-probe flush; restart Win_2 on every mid reuse; chase 13287210.
