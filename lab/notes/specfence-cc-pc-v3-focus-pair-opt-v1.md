# Focus pair optimize v1

**Date:** 2026-09-22
**Tip:** `ffa79ab` on `cursor/specfence-sf-ps-true-spine-d6e8`
**Baseline code:** census `97df2f7`, note `lab/notes/specfence-cc-pc-v3-full-land-result.md`
**Blocks:** `3356896` and `15274915` only. No mixed-49.
**Harness:** Instant-off, Soft=0, 8 cores, `SPECFENCE_COMPARE_CHECK=1`. Primary wall is the reuse median (for two reuse samples, the higher one).
**specfence-lab:** `repos/fengjy73/specfence-lab` returns HTTP 404. This note is the record on the pevm branch.

Lib release tests: 416 passed. `complete_arch_edge_pi_seq_eq_par_softwait0` passed. Both blocks `seq=par`, `occ_picks=0`, no hang.

## What the code does

A learned writer chain is the longest non-beneficiary location whose writer list is at least 32 and at most a quarter of the block. `15274915`'s shared basic (`abd6bb3978815b97`, about 50–75 writers) qualifies. `3356896`'s shared basic (`dff71d59d972d654`, 15 writers) does not.

On the next block the chain head is pushed last, so the local LIFO pop starts it before lower indices. Each later writer stays off the queue until the previous one commits, then runs ungated. `mark_gated` is not set. A quiet block records a shorter conflict snapshot; the longer chain is kept so the hold does not drop for one iter.

## RewindTo stays 0

The failing read is a basic at k=4 (`15274915`) or k=5/6 (`3356896`). The product path is ungated (`skip_ungated_tx_path_tax` and `!is_gated`). That path does not push a `CallEntry` checkpoint, and the write checkpoints run only after `!optimistic_ungated`. `last_checkpoint_before(fail_k)` is empty, so `PartialAbortRewind` cannot arm. Turning it on without a new checkpoint would re-enter the hang class that kept RewindTo off the single-invalid path. The interpreter restarts from the top. `ff_head` still answers prefix reads when a snap was kept (`full_from_0=0` on `3356896`). `resolve_rewind` stayed 0 on every sample below.

## Discarded

- **Gated nearest-pred (`note_consumer_on`).** The successor leaves the fast Opt path. FullReplay and the wall on `3356896` rose.
- **Hold every chain, including the 15-writer spine.** An earlier N=5 on `3356896` was primary SF/OCC 2.235/1.572 = 1.42, above the census note's 1.28. Full counts fell and the wall did not.
- **One-shot `InconsistentRead` when a lower Data publish covers a Storage read of the learned location.** `3356896` N=5 primary 2.878/1.001, one reuse iter FullReplay 92. The retry ran the doomed tx twice and cascaded.

## Same-host numbers

`97df2f7` rebuilt on this host. The note's 1.28 and 2.34 are a quieter sample (OCC 1.652 and 8.542). This host's OCC medians are lower, and a single N=3 swings. Compare ratios inside one run, and compare code on this host.

### 3356896

Chain stays under 32, so this pass does not plant a hold. Head is still tx 66 at about 0.4 ms.

| | census note | census `97df2f7` here, N=5 | this tip, calmer N=3 |
|:---|---:|---:|---:|
| OCC median ms | 1.652 | 1.041 | 1.093 |
| SF reuse median ms | 2.114 | 1.487 | 1.615 |
| primary SF/OCC | 1.28 | 1.43 | 1.48 |
| FullReplay reuse | 7, 19 | 32, 16, 8, 9 | (with the 1.615 ms median) 21, 26 |
| full_from_0 | 0 | 0 | 0 |
| WaitOnce reuse | 3, 4 | | 9, 9 |
| resolve_rewind | 0 | 0 | 0 |
| explore | 0 | 0 | 0 |
| seq=par, occ_picks | yes, 0 | yes, 0 | yes, 0 |

An unlucky N=5 on this tip printed primary 2.446/0.819. The same binary's next N=3 was 1.48. That is the census band on this host, not a cut under 1.28.

### 15274915

N=7 reuse median. Head is the learned chain's first writer.

| | census note | census `97df2f7` here, N=7 | this tip, N=7 |
|:---|---:|---:|---:|
| OCC median ms | 8.542 | 5.316 | 5.057 |
| SF reuse median ms | 20.030 | 12.590 | 11.089 |
| primary SF/OCC | 2.34 | 2.37 | 2.19 |
| head first-start | tx 116 ~2.2 ms | tx 537 ~2.5–2.9 ms | tx 129 ~0.60 ms |
| tail first-start | tx 1219 ~6.7–8.2 ms | tx 1184 ~4.4–4.8 ms | tx 1219 ~1.0–5.1 ms |
| FullReplay reuse | 150, 121 | 69–137 | 111–192 |
| full_from_0 reuse | 55, 48 | 18–52 | 32–80 |
| WaitOnce reuse | 66, 30 | 27–81 on a shorter run | 9–30 |
| resolve_rewind | 0 | 0 | 0 |
| explore | 0 | 0 | 0 |
| seq=par, occ_picks | yes, 0 | yes, 0 | yes, 0 |

The head move is stable across the six reuse iters. The wall ratio is under the note's 2.34 and under this host's census N=7 (2.37). Shorter runs overlap: one census N=3 here was already 9.674/5.647 = 1.71, and one run of this tip was 9.479/5.488 = 1.73. `full_from_0` did not reliably fall. WaitOnce fell on the held iters because the successor starts after the pred has committed, so the read does not park.

## Still open

1. **`3356896` early WAW.** The read is ungated Opt. `decide` / WaitOnce runs when a live writer tip is already visible. The writer has not installed an Estimate on the first incarnation until `record` at the end, so the read takes pre-state and validation FullReplays at k=5/6. Parking a writer that has not started fills every core. The 15-writer hold costs more wall than those replays. Ratio stays in the census band (about 1.4–1.5 here, note 1.28).
2. **Resolve entry is still FullReplay.** Prefix snaps cover `3356896` (`full_from_0=0`, `journal_ff_hits` > 0). The interpreter does not resume at `fail_k`. `15274915` still has reuse `full_from_0` in the 30–80 range.
3. **Crit-chain remaining work is only the held spine.** `15274915` starts that head at ~0.6 ms. `3356896` is still index LIFO (`corr(index, first_start)` about +1). Pick is not remaining work along the 15-writer chain.
4. **Learn.** `explore=0`. The early-WAW arm is stored and is not consulted on the ungated read, so the next incarnation of a short chain still fails at the same k.
