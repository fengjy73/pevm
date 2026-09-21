# PC / CC / learn — 压壳后三面（做了什么 / 没做好）

**Sweep head:** `2add58d` · Soft=0 · N=3 reuse @8 · 99/99  
**Sweep:** `lab/notes/specfence-shell-cut-then-redig-sweep.md`  
**TPS JSON:** `lab/notes/specfence-shell-cut-then-redig-summary.json`  
**Instant-off:** `lab/notes/specfence-shell-cut-then-redig-instant-off.md`  
**Land:** `lab/notes/specfence-shell-cut-then-redig-land-v1.md`  
**Baseline:** PR #42 33/98 · median 0.908  
**PC / CC / learn are analysis lenses, not split modules.** Instant-off is the primary wall. Corpus TPS **win-rate** is the north star.

This package pressed the remaining ~65-block **ungated shell** (execute/validate/调度 vs OCC), then reswept. Soft=0. One spine. `select_arm` is the only mouth.

## Outcome (honest)

| lens | SoT bar | this | hit |
|------|---------|-----:|:---:|
| SF TPS ≥ OCC | clearly > 33/98 | **26 / 98** | no |
| SF/OCC median | > 0.908 | **0.923** | yes |
| NEAR 14396881 | better than 0.840 | **0.893** | yes |
| NEAR 13217637 | better than 0.835 | **0.928** | yes |
| lazy 4–27× tail | none | none (wall max 3.41 = 13287210 OCC-noise) | yes |
| Soft=0 / iter11 / erc20 | hold | hold | yes |

Win-rate is the north star and it **regressed**. Median rose because the left tail compressed (2179522 quiet-block 0.095→1.133; NEAR shells +0.05–0.09) while several PR42 winners flipped just below 1.0. The remaining wall is **not** “the same 65-block ungated shell.”

---

## PC — utilization / ungated OCC task selection

### 做了什么

- **S1:** `skip_ungated_tx_path_tax` is OCC-equivalent on every ungated tx, **thin included**. Block-level `skip_ungated_path_tax` stays mid/large so thin D1 / HotSet still walk (same-sender lazy never OrderedAdmit — empty wait-set ≠ no structure).
- **S3:** order branches stay on gated txs. Done-on-success always stamps (iter11 flush race). Engagement / kernel counters stay (reuse telemetry).
- **S4:** mid/large empty/short-chain reuse leans `end_block`. Only large lazy-update reuse skips writer-order / MV / incarnation walks.

NEAR shells moved: 14396881 0.840→0.893, 13217637 0.835→0.928, wait-set 0, Opt→Opt. Quiet-block 2179522 is no longer a 10× OCC-noise loser (this run 1.133; SF reuse ~15 ms vs OCC ~17 ms).

### 没做好

- Ungated execute+validate matching OCC does **not** mint TPS wins. Most remaining losers already have wait-set 0 and `next_sf_task` OCC-picks. The residual is Detect / arm / validate abort, not the issue switch.
- Thin still pays D1 / HotSet / engagement. 3356896 stays ~0.69; this run climbed Win_2→Win_8 (`selected_arms` Win_8/14) — a learn leak, not a missing path-tax skip.
- 13287210 (n=1414, cap-0, wait-set 0, Opt→Opt) is 0.293 this run. PR42 2.578 was OCC ~22 ms; this OCC ~4.4 ms and SF 14.9 ms. PC path is already OCC-shaped; the wall is SF reuse being slower than a quiet OCC, not a leftover wait-set.

### PC localization

The ~65-block ungated shell is **pressed**. Further path-tax skips on thin end_block / HotSet regress mixed_hot / p1a. Next PC cut is **not** another `skip_ungated_*` flag.

---

## CC — cover_window / wait-set / conflict object

### 做了什么

- **S2:** near-independent / lazy-update large: wait-set soft-cap 0; `skip_useless_cover_probe`. Mid-band never leftover-slides (19469101 first-pick empty `short_chain`). Mid reuse drops leftover flush. Large ERC-20 may still slide a leaking loc and still flush a live probe (fence-cover / subgrain).
- **S5:** lazy never OrderedAdmit. L1 controlled cover probe on the **first** mid-band begin only. Reuse does not *start* a cover probe (`cover_probe_n==0`). C4 still deepens `cover_window` at `end_block`.
- 19469101 hang class completes (N=3 Opt→Opt, wait-set 8, Soft=0). 15274915 stays Opt on the long loc (Opt/74+Opt/41) — no lazy Full/996.

### 没做好

- Mid leftover-slide ban + reuse no-start probe **closes a livelock**, it does not cover a mid-band leftover-long. 8889776 0.872→0.785 (Full→Win_1). 19716145 1.035→0.933 (still Full, lost the win). 19469101 0.880→0.860 (wait-set 8, Opt). The wait-set is a soft-cap leftover hole, not a covering prefix.
- **19807137** under-covered storage is the new absolute wall among real spines: sweep 0.778→0.593, Full/2, SF 25 ms / OCC 15 ms, wait-set 0. Instant-off is **Opt×5 / 1.30×** (covering 0, unfenced 595–600) — sweep Full/2 is a ĉ/PROFILE leak, not the Instant-off object. Cover cannot absorb L≫64.
- 14334629 Opt/485 on a large loc (0.713) is leftover-long Detect, not a wait-set plant. Cap-0 did not make SF = OCC.
- 6196166 Win_2/49+Win_2/26 (n=108 thin-ish) is over-wide cover on a small block — CC object is wrong (too much OrderedAdmit), not missing cover.

### CC localization

The live CC objects are:

1. **Under-covered conflict spine** (19807137): cover_window cannot absorb leftover; Full/2 is still a learn-uphill leak.
2. **Mid-band leftover-long after no-start probe** (8889776 / 19469101 / 19716145): first-block Opt leftover is sticky; reuse will not plant Win_2. That is safer than the hang, and worse than a proven cover.
3. **Thin / small-n window mill** (3356896 Win_8, 6196166 Win_2×75): train_hat / light-cover is climbing where OCC abort is cheaper.

---

## Learn — probe budget / arm / ĉ

### 做了什么

- Probe budget still 2. Unproven `probe_cover_w` is Win_2. Proven cover reads `loc_cover_window`. Instant idle stays out of ĉ.
- Reuse does not start a probe after first-block Opt leftover (`last_crisis` must not force Win_2 — 19469101 N=3).
- Thin `train_hat` is capped `cores.max(4).min(8)` — 6137495 stays Win_1/27, not Win_16. 3356896 still showed Win_8/14 this run (hat=8), so the cap is the climb, not a 16-wide mill.

### 没做好

- First-block crisis → reuse skip-seed is a **hard refuse**, not a measured “cover lost to abort CF.” Learn cannot recover a mid-band leftover-long across iters without restarting the livelock class.
- 19807137 Full/2 after under-covered sticky-Opt is a ĉ leak: a 2-pair loc is coverable, the 571-writer storage spine is not. Morph / short Full is the wrong object.
- 3356896 Win_2→Win_8: thin light-cover is still learning uphill against OCC abort. `train_hat` 8 is enough to blow prepaid on n=176.
- Win-rate regression is partly **arm noise**: 19716145 Full→Full 1.035→0.933 is a 1 ms OCC/SF flip, not a new policy.

### Learn localization

Learn still cannot say “this leftover-long is cheaper as OCC abort **this block** and also cheaper as Win_2 **next block**” without the 19469101 plant→refuse→flush. The missing mouth is a **one-shot measured probe that cannot queue leftover**, not a wider `cover_window`.

---

## New bottleneck (after this package)

The remaining ~72 losers are **not** one ungated-shell class. Split:

| class | reps | Instant-off | what to cut next |
|-------|------|-------------|------------------|
| Thin window climb | 6196166 0.629, 3356896 0.691, 6137495 0.743 | 1.43× / 1.13× | nail thin `train_hat` to Win_2; do not learn Win_8 on n≤176 |
| Under-covered spine | 19807137 0.593 | **1.30× Opt×5** | never Full/Seg on the long storage loc (sweep Full/2 is ĉ/PROFILE); keep Instant-off Opt = OCC abort |
| Mid leftover-long, no probe | 4330482 0.703, 8889776 0.785, 19469101 0.860 | 1.26× Win_1 | measured one-shot cover that cannot T3-slide or first-pick flush |
| Large leftover Detect | 14334629 0.713, 15274915 0.784 | 1.17× Opt | Opt is already the object; residual is validate/abort, not wait-set |
| OCC-quiet noise | 13287210 0.293 | 3.77× jitter | do not chase |

Do **not**: another `skip_ungated_tx_path_tax` variant; lean thin `end_block`; drop large live-probe flush; restart Win_2 on every mid reuse.

---

## Instant-off (primary wall, N=5 @8)

See [`specfence-shell-cut-then-redig-instant-off.md`](specfence-shell-cut-then-redig-instant-off.md). No `sf_le_occ`. Soft=0 every iter.

| block | sweep × | Instant × | Instant object | correction to the sweep story |
|-------|--------:|----------:|----------------|-------------------------------|
| 13287210 | 3.41 | **3.77** | Opt×5, unf 2–5 | OCC + SF reuse both jitter 4–39 ms. Do not chase. |
| 19807137 | 1.69 | **1.30** | **Opt×5**, covering 0, unf 595–600 | Sweep Full/2 is ĉ/PROFILE. Instant-off already sticky Opt. Residual is Detect on L≫64, ~5 ms. |
| 6196166 | 1.59 | **1.43** | Win_2→…→Win_8, cover_w=8 | Confirms thin window mill. Worst *real* Instant-off after 19807137. |
| 3356896 | 1.45 | **1.13** | Win_1→…→Win_8 | Climb is real; Instant-off median is near OCC. Sweep over-weighted Win_8. |
| 4330482 | 1.42 | **1.26** | Opt→Win_1/28+Win_1/23 | Mid leftover plant. Safer than T3-slide; still not a measured one-shot cover. |
| 14334629 | 1.40 | **1.17** | Opt×5, unf 33–62 | Leftover Detect is already the object. Residual abort, not wait-set. |

Instant-off reorders the next cut: **thin `train_hat` nail** and **forbid sweep Full on 19807137-class**, not another `skip_ungated_*`.

## Invariants that held

- Soft=0 every row (sweep and Instant-off).
- lazy never OrderedAdmit; 15274915 no Full/996.
- 19469101 N=3 completes.
- iter11 / p4 / mixed_hot / p1a / fence-cover / subgrain / erc20 independent.
- No 4–27× lazy thousand-writer tail.
