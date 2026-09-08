# VLDB-adjacent CC papers: how they plot and what they argue

Target plots for SpecFence mainnet: **TPS vs cores** (scalability line) and **abort rate vs cores**, one pair per block, plus a combined overview. Colors are bound to CC mode for the whole lab.

## Papers we actually steal from

| Paper | Venue | What the figures do | What the text argues with them |
|---|---|---|---|
| Yu et al., *Staring into the Abyss* | PVLDB 8 | (1) throughput vs cores; (2) stacked time breakdown (useful vs abort vs wait); (3) abort/throughput vs contention skew at **fixed** cores | Scalability **wall** is diagnosed by pairing TPS flattening with abort/wait blow-up. OCC looks fine until validation aborts dominate. Never show TPS without the abort companion. |
| Tanabe et al., *CCBench* | PVLDB 13 | Every point = **mean of 5 runs**; whiskers = **min–max**, not σ. Threads pinned. Throughput figure and abort-ratio figure are siblings. | Reproduces prior CC papers, then attributes gaps to abort vs lock vs latch. Error bars are part of the claim. |
| Huang et al., *Opportunities for Optimism* | PVLDB 13 | TPS vs 1–64 threads. **Perfect scalability** drawn as the diagonal through origin and the 1-thread point. Note slope change at HT/NUMA. | High contention: scale to 4–8 then **level off, decline, but not collapse**. Differences at low contention are mechanism overhead; at high contention they are abort irreconcilability. |
| Lu et al., *Aria* | PVLDB 13 | TPS vs threads **and** vs partitions; baselines Bohm / Calvin / PWV on the same axes | Deterministic CC is compared at the same commit-order constraint we have (preset order). Sequential/oracle is the ceiling, not “linear”. |
| Wang et al., *Polyjuice* | OSDI 21 | Median of 5×30s; TPS vs contention at **fixed** thread count; ablations on action space | Learned mixed CC (wait vs speculate vs early validate) is the closest methodological cousin. Low contention: metadata overhead can lose to Silo. High contention: mixing beats pure OCC/2PL. |
| Gelashvili et al., *Block-STM* | PPoPP 23 | Speedup vs sequential at 32 threads; contended vs uncontended | Primary OCC baseline for PEVM. Adaptive only in the weak sense (abort/retry). |

PDFs: `lab/literature/pdfs/`.

## Rules we follow in `lab/figures`

1. **Paired plots.** Every TPS-vs-cores line has an abort-rate-vs-cores sibling. Yu/CCBench: abort is the explanation, not a footnote.
2. **One color per mode**, stable across all blocks: Sequential `#637083`, OCC `#2563eb`, PCC `#0f766e`, SpecFence `#c2410c`.
3. **Mean + min/max whiskers** (CCBench), not a single run. Median is diagnostic only.
4. **Sequential is a horizontal dashed line**, not a 1-core-only polyline. Perfect-scale reference is a faint diagonal from the 1-core OCC point (Huang), labelled, not a claim.
5. **x = worker threads** `{1,2,4,8}` on this box (8 cores). Annotate if we ever cross NUMA/HT.
6. **Abort rate** = `occ_aborts / n_tx` (retries per transaction, Block-STM incarnations). Also dump `wait_admissions` and `region_promotions` so SpecFence cannot hide as “OCC with fewer aborts and no Wait”.
7. **Do not claim linear scaling.** The paper goal is TPS close to sequential-equivalent work at the DAG ceiling, and no collapse at 8 cores. Huang’s “levels off, does not collapse” is the language.
8. **Per-block facets + one overview.** Mainnet blocks differ in conflict; pooling them without a per-block panel is how you lie.

## Analysis questions each figure must answer

- Does TPS keep rising, level, or drop as cores go 1→8?
- If it levels/drops, does abort rate climb (OCC) or does wait dominate (PCC)?
- Does SpecFence sit between OCC and PCC: fewer retries than OCC, more parallelism than PCC?
- Independent of 1–5/6 stories: on *this* block, are Wait and Speculate both non-zero? If Wait=0 it is OCC; if Speculate=0 it is PCC.
