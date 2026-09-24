# SpecFence all-blocks deep evidence (Part 1)

**Date:** 2026-09-13 (Asia/Shanghai, UTC+8)  
**HEAD:** `87d3979` on `cursor/specfence-complete-cc-63b0`  
**Vocab:** Spec = Region; Fence = Bind / WaitFor / serial-lane+admit; Unfenced = optimistic  
**Companion SoT:** `lab/notes/specfence-complete-architecture-v2.md`  
**Sweep SoT:** `lab/notes/specfence-all-blocks-sweep-summary.md`  
**Focus subgrain:** `lab/notes/specfence-post-subgrain-multiblock-deep-diagnosis.md`

---

## Grain honesty (cannot store full ProcessTrace × 99 @ max detail)

| Coverage tier | Blocks | Grain available |
|---------------|--------|-----------------|
| **A — Full process + per-tx + edge/frame** | Focus **14689597 / 19606599 / 19469097** (+ neighbors via xblock) | `post-subgrain-per-tx-*-c8.json`, `exec-process-*-post-subgrain.json`, L1 DAG |
| **B — ProcessTrace digest** (reason hist + hot ℓ + metrics; **no** per-tx ranked) | `19807137, 6196166, 14029313, 14334629, 14689597, 19860366` (+ `8889776, 10760440, 2179522`) | `lab/results/all-blocks-process-{bn}.json` |
| **C — Metrics + fail-mode cluster only** | Remaining of 98 non-empty | `all-blocks-sf-occ-sweep.json` / corrected summary / N3 overlay |
| **Missing B for corrected worst10** | `19434587, 4330482, 19933612, 15199017` | Use N3/N1 metrics; infer class from morph peers — **explicitly not sub-tx** |

Universal holds on every SF row: `soft_wait_arms=0`, `await_at_a_arms=0`, `unfenced_writer_done=0` (where process digest exists), `unfenced_after_fence_on_hot_l=0`.

---

## Dominant cross-block failure modes (98 non-empty)

| Fail mode | n | What it means | Typical SF/OCC |
|-----------|--:|---------------|---------------:|
| **R2_SUFFIX_REPAIR_DOMINANT** | 21 | `rewind_to_cp` ≫ `rebind_only`; identity/FF unused as cheap path | 0.09–0.30 |
| **META_COLD_CANARY_GAP** | 21 | Low absolute rewind but wall still ~3–5×; canary/cold Unfenced + meta dominate | 0.20–0.50 |
| **QUIET_PARITY** | 18 | SF/OCC ≥ 1; Fence barely fires | ≥1.0 |
| **MIXED_FANOUT_BAND** | 17 | Mid pack fan_out; Bind+rewind+park mixed | ~0.25–0.45 |
| **BIND_RESIDUAL_LIGHT_REPAIR** | 17 | Residual Bind live; repair light; still wall gap vs OCC | ~0.23–0.50 |
| **WAIT_PARK_SCHEDULE** | 4 | `park_idle` owns large fraction of 8·wall | 0.24–0.33 |

**Block lists:**
- R2: `19807137, 6196166, 19860366, 19469096, 14545870, 19716145, 17666333, 19606600, 17034870, 19426586, 19469099, 14683600, 19606599, 19444337, 18085863, 19498855, 12244000, 12459406, 19469098, 19469097, 19434587`
- META_COLD: `14029313, 4330482, 15199017, 15274915, 6137495, 13217637, 14396881, 5283152, 3356896, 12520364, 13287210, 4864590, 8038679, 7280000, 14689596, 12047794, 15752489, 19606598, 9069000, 7279999, 11114732`
- WAIT_PARK: `10760440, 14689597, 12522062, 5526571`
- QUIET_PARITY / MIXED / BIND_RESIDUAL: see `/tmp`-regen from corrected pairs or `all-blocks-sf-occ-sweep-corrected-summary.json`

---

## Corrected worst 10 — deep portraits

Sources: N3 medians where overlaid; process digests where present (N=1 last-iter counters — wall claims use N3 medians).

### 1) 19807137 — SF/OCC **0.090**, wall **11.1×** (n_tx=712, fan_out) — GLOBAL WORST

| Face | Evidence |
|------|----------|
| Metrics (N3) | bind=9125, wait=105, rewind=**2002**, rebind=113, park=115, fr=33, aborts_sf=1940 |
| Process digest (N=1) | bind=8365, residual=5933, wd_learn=5933, id_pres=**1772**, ff=**5455**, rebind=108, rewind=1855, multi_spine=354, prefer=5, canary_re=63, idle≈0.11 |
| Reason hist | cold=882, canary=1761, indep=1092, writer_done=**0**, u_aa=0, wait_serial=118, wait_writer=25 |
| Hot ℓ | `6996519588683120047` bind=2291 (star cover holds) |
| Not Avoided | Closed on writer_done / hot-after-fence |
| Not Resolved | **R2 body thrash**: rewind≈18× rebind; FF+identity abundant but R1 underfire |
| Compute vs CC | **CC Resolve** dominant; park secondary (~11% idle) |
| file:fn | `rem.rs` SuffixRepair-first; `rem.rs::identity_stable_match` unused as default; `pevm.rs::try_validate`; `edge.rs::choose_edge_action` OK on Bind |
| Ideal TPS | Cap useful P≈8 on independents; **R1-first** on value-stable residuals should cut rewind toward rebind scale → target wall toward OCC ~12 ms band |
| Learn | Should: promote identity/FF→R1. Unused: id_pres/ff as resolve actuator. Used: wd→residual Bind |

### 2) 6196166 — SF/OCC **0.155**, wall **6.4×** (n_tx=108) — PARK WORST

| Face | Evidence |
|------|----------|
| Metrics (N3) | bind=1385, wait=56, rewind=166, rebind=13, park=50, dom=`park_idle≈0.93` |
| Process | idle≈**0.96**, park_ns≈60 ms on ~8 ms wall, steal=48, residual=639, id=337, ff=432, rebind=**2**, rewind=148 |
| RH | cold=310, canary=240, indep=130, wait_serial=48, wait_writer=9, writer_done=0 |
| Why | Clique WaitFor **serial** parks call-frame opcode-seconds; steal fills Ready but parked frames burn wall |
| Compute vs CC | **COMPUTE schedule** primary; CC R2 secondary |
| Ideal TPS | Non-blocking continuation / park budget keeping P≈8 on independents **without** SoftWait Soft |
| Learn | Should: schedule-slot PreferAdmit under heat. Unused: PreferAdmit=3 thin vs 48 parks |

### 3) 14029313 — SF/OCC **0.204**, wall **4.9×** (n_tx=724, mixed) — META/COLD

| Face | Evidence |
|------|----------|
| Metrics | bind=338, wait=7, rewind=20, rebind=1, park=8, unf=2627 |
| Process | residual=175, canary_re=61, cold=476, canary=1258, indep=881, idle≈0.01, rebind=0 |
| Why | Not R2 volume — **first-wave canary/cold rediscovery + Avoid lag** on wide block; OCC already fast (~3.3 ms) so SF meta is the gap |
| Compute vs CC | **CC learning H / cold** |
| Ideal TPS | Earlier pack_top / Avoid; quieter Fence tax |

### 4) 19434587 — SF/OCC **0.229**, wall **4.4×** (n_tx=390) — **no process digest** (grain C+)

| Face | Evidence |
|------|----------|
| N3 metrics | bind=4790, wait=59, rewind=**360**, rebind=12, park=55, fr=48, residual≈3834, ff≈2506, id≈415, prefer=18 |
| Class | Same as 19807137 family: **R2 + residual Bind + park**; soft=0 |
| Infer txs | Peer of hot post-Merge fan_out — expect high-inc tip readers + park clique (like 597/1943xxxx) |
| Ideal | R1-first + fanout_fr_collapse generalize |

### 5) 14334629 — SF/OCC **0.234**, wall **4.3×** (n_tx=819, mixed)

| Face | Evidence |
|------|----------|
| Process | bind=871, residual=619, rewind=36, rebind=**0**, canary_re=145, cold=447, canary=1393, idle≈0.02, u_aa=1 |
| Why | Residual Bind live; **R1 never fires**; canary reopen high but cold remains; light park |
| Class | BIND_RESIDUAL_LIGHT_REPAIR overlapping META_COLD |

### 6) 4330482 — SF/OCC **0.235** (n_tx=237) — **no process digest**

| Metrics | bind=81, wait=5, rewind=18, rebind=0, park=3, unf=509, residual=54, ff=57 |
| Class | META_COLD_CANARY_GAP — small Bind absolute, wall from Unfenced/meta vs tiny OCC wall |

### 7) 19933612 — SF/OCC **0.237** (n_tx=130) — **no process digest**

| Metrics | bind=1134, wait=16, rewind=48, rebind=1, park=16, residual=984, prefer=6, idle≈0.26 |
| Class | BIND_RESIDUAL + park secondary; fan_out hot |

### 8) 14689597 (597) — SF/OCC **0.239**, wall **4.2×** — FULL SUBGRAIN

See post-subgrain note. Top failing txs: **72,71,60,43,73,74,20,29,11,28**.

| tx | b/w/u | park | inc | Root (access/edge/frame/slot) |
|---:|------:|-----:|----:|-------------------------------|
| 72 | 25/5/40 | 5 | **9** | Star Bind OK; each R2 inc re-cold private ℓ; WaitFor parks schedule |
| 71 | 40/1/38 | 1 | 6 | Same satellite; cold 35 |
| 60 | 24/1/41 | 1 | 5 | Cold rediscovery |
| 43 | 51/0/27 | 0 | 5 | Pure repair thrash, no park |
| 29 | 44/3/28 | 3 | 3 | Clique RAW; Fence Wait + canary/cold |
| 11 | 89/0/34 | 0 | 4 | Heavy Bind cover — **Resolve not Avoid** |

L1: morph=fan_out, wave≈434, chain≈29, fanout≈448. Ideal P≈8 on wave; serialize only RAW chain+star.

### 9) 15199017 — SF/OCC **0.240** (n_tx=866, heuristic quiet) — **no process digest**

| Metrics | bind=187, wait=3, rewind=10, rebind=0, unf=2373, canary_re=112, residual=121 |
| Class | META_COLD — heuristic over-quiet; wall gap from canary/meta on large n_tx |

### 10) 19860366 — SF/OCC **0.241**, wall **4.2×** (n_tx=430)

| Process | bind=2387, residual=1905, rewind=148, rebind=5, canary=2362, cold=1407, prefer=**25**, park idle≈0.25, fr=26, u_aa=11 |
| Class | R2 + canary storm + park; PreferAdmit live but resolve still expensive |

---

## Focus family refresh (597/599/097 + neighbors)

| Block | L1 morph | SF med / OCC | wall × | R1/R2 | park idle | Top failing txs |
|------:|----------|-------------:|-------:|------:|----------:|-----------------|
| 14689597 | fan_out | 17.7 / 4.0 | 4.41× | 0/95 | ~0.52 last | 72,71,60,43,29 |
| 19606599 | long_chain | 35.2 / 9.7 | 3.64× | 2/172 | ~0.14 | 322,51,24,63,28 |
| 19469097 | long_chain | 19.0 / 7.1 | 2.68× | 3/195 | ~0.08 | 322,71,73,316,37 |
| 19606598 | quiet pred | ~4.0 / 1.7 | ~2.4× | light | ~0 | — |
| 19469096 | harsher Wait | 16.4 / 3.5 | ~4.7× | 3/? | high wait_hard 110–140 | WaitHard morph |
| 14689596 | light fan | 4.6 / 1.0 | ~4.6× | 2 | light | — |

Warm PreferAdmit fires on 597 (12) / 096 (10) / 097 (13) in xblock — **Ready window opens under heat**, not cold-start H. Wall class unchanged: `SuffixRepair_makespan_cold_rediscovery`.

---

## Morph sample (≥3 quiet / ≥3 mixed-spine / ≥3 mid fan_out ∉ worst10)

### Quiet (SF/OCC≥1)
| bn | SF/OCC | n_tx | Notes |
|---:|-------:|-----:|-------|
| 2179522 | 1.074 | 222 | N3 corrected; Fence almost off |
| 14689595 | 1.17 | 23 | Focus quiet neighbor |
| 14689599 | 1.11 | 43 | Focus quiet |
| (+ early tiny: 46147, 116525, …) | >1 | ≪100 | Do not claim production TPS win |

### Mixed / spine
| bn | morph | SF/OCC | bind | rewind | Class |
|---:|-------|-------:|-----:|-------:|-------|
| 14029313 | mixed | 0.204 | 338 | 20 | META_COLD |
| 14334629 | mixed | 0.234 | 818 | 39 | BIND_RESIDUAL |
| 3356896 | spine | 0.297 | 105 | 16 | META_COLD; residual=54, rebind=0 |
| 4864590 | mixed | 0.324 | 70 | 9 | META_COLD |

### Mid-pack fan_out (≈ median 0.335, not worst10)
| bn | SF/OCC | bind | rewind | rebind | park | Class |
|---:|-------:|-----:|-------:|-------:|-----:|-------|
| 19737292 | 0.335 | 637 | 31 | 0 | 17 | BIND_RESIDUAL; residual=524, ff=283 |
| 12243999 | 0.335 | 616 | 32 | 0 | 11 | same; canary_re=43 |
| 19932148 | 0.334 | 1215 | 87 | 0 | 26 | MIXED + park_idle≈0.27; fr=22 |
| 19638737 | 0.340 | 1152 | 45 | 0 | 17 | BIND_RESIDUAL |
| 10760440 | 0.326 | 1270 | 80 | 6 | 31 | WAIT_PARK idle≈0.55 |

**Shared mid-pack sentence:** residual Bind + writer_done learning **used**; R1≈0; cold/canary Unfenced scales with repair; OCC still ~3× wall.

---

## Conflicts: Avoided vs Resolved (edge grain, cross-set)

| Class | Status across 99 | Evidence |
|-------|------------------|----------|
| Done∅Data → UnfencedWriterDone | **CLOSED** | writer_done=0 on all digests |
| Avoid ∧ Unfenced (u_aa) | **CLOSED** as loc class | u_aa≈0 |
| Hot star after Fence Unfenced | **CLOSED** | hot_after_fence=0 |
| Cold/canary first-wave on long-tail ℓ | **OPEN** | cold+canary dominate residual Unfenced |
| R2 vs R1 on identity/FF | **OPEN dominant** | rewind ≫ rebind everywhere hot |
| WaitFor park makespan (fan_out) | **OPEN secondary** | 6196166 / 597 / 10760440 |
| SoftWait / Await@a | **BAN HELD** | 0 / 0 |

---

## Learning audit snapshot (→ Part 2 in architecture)

| Signal | In sweep metrics? | Enters live learner/π? |
|--------|-------------------|------------------------|
| writer_done / bind_residual | yes | **yes** → H / residual Bind |
| canary_reopen | yes | **yes** (prod reopen) |
| prefer_admit | yes | **yes** but Ready-thin on cold |
| writer_identity_preserved / journal_ff_hits | yes | **coded**, under-selected as R1 |
| rewind_to_cp / rebind_only ratio | yes | **not** a resolve prior |
| wait_park_ns / steal | yes | updates e_idle (retired EV); **π ignores** |
| engagement Quiet/Storm | switches=1 | **banned** from edge π |
| AEC choose_resolve / αβγδ | retired | **dead theater** — delete from SoT |
| Morph heuristic fan_out overcall | in summary | not calibrated to L1 DAG on all 99 |

---

## Ideal parallel → TPS (laws, not patches)

1. **Fence only RAW CP Regions**; UnfencedIndependence on wave (already mostly true).  
2. **Residual Bind on Done writers** (done — keep).  
3. **R1 RebindOnly default** when `identity_stable_match` ∨ FF value-stable (`rem.rs`) — replace SuffixRepair-first.  
4. **Incarnation-stable residual map** so repair does not cold-miss already-seen ℓ.  
5. **Park budget / continuation** so fan_out keeps P≈8 on independents without SoftWait Soft.  
6. **Quiet morph: Fence off** — protect SF/OCC≥1 cohort.

**One-liner:** remaining gap is **repair/schedule makespan on contended Regions**, not access-face writer_done leaks.

---

## Artifacts index

| Path | Role |
|------|------|
| `lab/results/all-blocks-sf-occ-sweep.json` | 99 pairs N=1 |
| `lab/results/all-blocks-sf-occ-n3-outliers.json` | N=3 overlay |
| `lab/results/all-blocks-sf-occ-sweep-corrected-summary.json` | distribution |
| `lab/results/all-blocks-process-*.json` | digests (tier B) |
| `lab/results/post-subgrain-*` | focus full grain |
| `lab/results/l1l2-b{597,599,097}.json` | L1 DAG |
| `lab/notes/specfence-complete-architecture-v2.md` | Part 3 SoT |
