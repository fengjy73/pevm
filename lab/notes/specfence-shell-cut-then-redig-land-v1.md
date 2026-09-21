# 压剩余 ungated 壳 — S1–S5 land

**Base:** PR #42 `cursor/specfence-pc-cc-learn-complete-2cd0` @ `42cf30d`  
**SoT:** `uploads/specfence-shell-cut-then-redig-v1.md`  
**Baseline:** SF TPS≥OCC **33/98**, median **0.908**; ~65 losers are ungated execute/validate/调度壳  
**Soft=0 · one spine · `select_arm` is the only mouth**

Lazy-update chains stay **not** OrderedAdmit objects. Ungated OCC task selection, Done-on-success (when a later plant can race), and Soft=0 are preserved.

## Landed

| ID | Content |
|----|---------|
| **S1** | `skip_ungated_tx_path_tax` is OCC-equivalent for every ungated tx (thin included). Block-level `skip_ungated_path_tax` stays mid/large — thin D1 / HotSet still walk (same-sender lazy has no wait-set). Leftover-long still keeps gated D1 / end_block. |
| **S2** | Near-independent / lazy-update large: wait-set soft-cap 0; `skip_useless_cover_probe` bans cover plant. Mid/large reuse: `skip_reuse_leftover_flush` + no *start* of a cover probe (`cover_probe_n==0`). Thin `train_hat` stays ≤8 (no 6137495 Win_16 climb). |
| **S3** | Order branches stay on gated txs. Ungated execute+validate is OCC-equivalent (`skip_ungated_tx_path_tax` + `is_gated`). Done-on-success always stamps (iter11 flush race). Engagement / kernel counters stay (test + reuse telemetry). |
| **S4** | Mid/large empty/short-chain reuse leans `end_block`. Only large lazy-update reuse skips writer-order snapshot, MV walk, and per-tx incarnation mutex walk. Thin HotSet and mid-band leftover (p4) keep the walk. |
| **S5** | lazy never OrderedAdmit; L1 controlled cover probe on mid-band real spines; Done-on-success when a plant can race; Soft=0; professional terms. |

## Invariants

- Thin first incarnation still persists D1 (3356896 4→31).
- Thin leftover-long wait-set still takes gated D1 / end_block.
- Thin reuse never leans / never skips end_block walks (HotSet / p1a).
- Mid-band coverable spines still probe Win_2 on the **first** begin; unproven probe stays Win_2.
- 19469101 leftover-slide hang class: mid/large never T3-slides (even with empty short_chain at first-pick flush). Reuse does not *start* a cover probe (`cover_probe_n==0`); `skip_reuse_leftover_flush` drops pending idle. C4 still deepens `cover_window` at `end_block`. First-block Opt leftover may set `last_crisis`; forcing Win_2 on reuse hung N=3.
- iter11 Done-on-success: stamp stays whenever `has_pending_gated` / pending idle / thin storage-like D1 can plant.

## Acceptance (after 99-block Soft=0 sweep)

- SF TPS≥OCC **clearly > 33/98**
- median **> 0.908**
- NEAR shell reps 14396881 / 13217637 better (shell ↓)
- no lazy fat-tail
- iter11 / erc20 independent
