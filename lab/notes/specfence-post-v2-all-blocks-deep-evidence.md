# SpecFence post-v2 all-blocks deep evidence

**Date:** 2026-09-13 (Asia/Shanghai, UTC+8)  
**HEAD:** `b63c75d` on `cursor/specfence-complete-cc-63b0`  
**Vocab:** Spec = Region; Fence = Bind / WaitFor / serial-lane+admit; Unfenced = optimistic  
**Companion SoT:** `lab/notes/specfence-complete-architecture-v3.md`  
**Sweep artifacts:** `lab/results/arch-v2-all-blocks-sf-occ-sweep.json`, `…-n3-outliers.json`, `…-corrected-summary.json`, `lab/results/arch-v2-process-{bn}.json`

---

## 0. Headline after v2 land

| Set | median SF/OCC | wall SF/OCC | notes |
|-----|--------------:|------------:|-------|
| Prior (pre-v2 land, corrected n=98) | **0.356** | **2.79×** | `87d3979` era |
| **This tip v2 (corrected n=98)** | **0.326** | **3.05×** | Δ median **−0.030** |
| Quiet morph median | 1.10 | — | cohort still ≥1; must not regress |
| Fan_out morph median | 0.298 | — | owns the left tail |

**Verdict:** v2 single-cut land (R1-first door, inc_carry, park PreferAdmit heat, edge visibility SM, quiet H-skip, structural collapse) **did not move the cost class**. Soft/Await bans hold (`soft=0`, `await=0` on every SF row). Access-face leaks stay closed (`writer_done=0`, `u_aa≈0`, `hot_after_fence=0` on digests). The remaining gap is still **makespan cost class vs OCC**, not missing Avoid on stars.

Hard bans held on every SF row: SoftWait Soft storms, EV Await doors, tip-identity Bind gate, OCC-retry-as-π, 597 hardcodes.

---

## 1. Coverage & honesty

| Tier | Blocks | Grain |
|------|--------|-------|
| **A — Full process + per-tx + edge/frame** | Focus **14689597 / 19606599 / 19469097** (+ neighbors) | `post-subgrain-per-tx-*-c8.json` (pre-tip HEAD `dbf9f15` — structural; morph unchanged), L1 DAG `l1l2-b*.json` |
| **B — ProcessTrace digest on tip** | Worst-set: `19807137, 6196166, 19606599, 14689597, 6137495, 15199017, 14029313, 14334629, 19469096` (+ empty `19910734`) | `arch-v2-process-{bn}.json` |
| **C — Metrics + fail-mode only** | Remaining of 98 non-empty | corrected summary |
| Empty | `19910734` | excluded from ratios |

N=1 mandatory across 99; N=3 overlay on 18 real outliers/focus (+ empty). Corrected set drops `n_tx=0` and overlays N=3 medians.

---

## 2. Corrected distribution (n=98)

| Metric | median | p10 | p90 | mean | geo |
|--------|-------:|----:|----:|-----:|----:|
| **SF/OCC TPS** | **0.326** | 0.238 | 1.161 | 0.517 | 0.406 |
| **wall SF/OCC** | **3.05×** | — | 4.21× | 2.94× | — |
| SF TPS | ~19k | — | — | — | — |
| OCC TPS | ~56k | — | — | — | — |

Morph (counter heuristic; quiet↑ vs prior after v2 quiet-seed):

| Cluster | n | median SF/OCC |
|---------|--:|--------------:|
| fan_out | 63 | **0.298** |
| quiet | 31 | **1.10** |
| mixed | 4 | 0.346 |

---

## 3. Dominant failure modes (every non-empty block accounted)

| Fail mode | n | Meaning | Typical SF/OCC |
|-----------|--:|---------|---------------:|
| **R2_SUFFIX_REPAIR_DOMINANT** | **36** | `rewind_to_cp` ≫ `rebind_only`; body SuffixRepair owns wall | 0.08–0.30 |
| **QUIET_PARITY** | 18 | SF/OCC ≥ 1; Fence barely fires | ≥1.0 |
| **WAIT_PARK_SCHEDULE** | 14 | `park_idle` / WaitFor schedule tax on RAW clique | 0.13–0.27 |
| **META_COLD_CANARY_GAP** | 14 | Low absolute rewind; wall from canary/cold Unfenced + meta vs tiny OCC wall | 0.19–0.35 |
| **BIND_RESIDUAL_LIGHT_REPAIR** | 9 | Residual Bind live; repair light; still wall gap | 0.24–0.35 |
| **MIXED_FANOUT_BAND** | 7 | Mid morph; Bind+rewind+park mixed | ~0.25–0.61 |

**Block lists (complete):**

- **R2 (36):** `5526571, 8889776, 12244000, 12964999, 12965000, 14334629, 14383540, 14545870, 14683600, 15537394, 15538827, 16146267, 16257471, 17034870, 17666333, 18085863, 18426253, 18988207, 19426586, 19434587, 19444337, 19469096, 19469097, 19469098, 19469101, 19498855, 19505152, 19606599, 19606600, 19638737, 19737292, 19807137, 19932703, 19932810, 19933597, 19933612`
- **WAIT_PARK (14):** `4370000, 6137495, 6196166, 7280000, 10760440, 12459406, 12522062, 14689597, 19469099, 19606597, 19716145, 19860366, 19929064, 19932148`
- **META_COLD (14):** `3356896, 4330482, 4369999, 4864590, 5283152, 7279999, 8038679, 11114732, 11743952, 12047794, 12159808, 13217637, 14029313, 15199017`
- **BIND_RESIDUAL (9):** `9069000, 12243999, 14689596, 14689598, 15274915, 15752489, 17034869, 19606598, 19917570`
- **MIXED (7):** `2179522, 5891667, 11814555, 12300570, 12520364, 13287210, 14396881`
- **QUIET_PARITY (18):** `46147, 116525, 930196, 1150000, 1796867, 2462997, 2641321, 2674998, 2675000, 2688148, 9068998, 14689595, 14689599, 15537393, 19426587, 19923400, 19933122, 19934116`

Account check: 36+18+14+14+9+7 = **98**.

---

## 4. Worst 10 — below-tx portraits (tip process + metrics)

Sources: N=3 medians where overlaid; `arch-v2-process-*` digests (N=1 last-iter counters — wall claims use N=3).

### 1) 19807137 — SF/OCC **0.076**, wall **13.2×** (n_tx=712) — GLOBAL WORST

| Face | Evidence |
|------|----------|
| N3 metrics | rewind=**1906**, rebind=155, bind≈9k, wait≈100, fr high, soft=0 |
| Process | bind=9664, residual=6948, wd_learn=6747, id_pres=**2060**, ff=**6178**, rebind=168, rewind=2074, prefer=6, canary_re=61, idle≈0.10, collapse=46, absorb=120 |
| RH | cold=857, canary=1753, indep=1121, writer_done=**0**, u_aa=0, wait_serial=68, wait_writer=27 |
| Hot ℓ | `6996519588683120047` bind=2565 (star cover holds) |
| **Not Avoided** | Closed on writer_done / hot-after-fence / u_aa |
| **Not Resolved** | **True-suffix value-changing aborts** → R2 body thrash; id/FF abundant at observe time but `value_stable` fails at validate → R1 door closed (`pevm.rs::try_validate` requires value match on `true_suffix`) |
| Compute vs CC | **CC Resolve (R2)** primary; park secondary (~10% idle) |
| file:fn | `pevm.rs::try_validate` R1 door; `rem.rs::identity_stable_match` / `value_stable_match`; `rem.rs` SuffixRepair; `edge.rs::choose_edge_action` OK on Bind |
| Ideal TPS | Cap useful P≈8 on wave independents; **do not pay Fence meta when abort inevitable** — OCC reincarnation cheaper than Bind×9k + R2×2k |
| Learn | Used: wd→residual Bind, collapse/absorb. Unused as cost-class change: id_pres/ff→R1 on true_suffix (structurally blocked). Missing: ROI gate that skips Fence when R2 inevitable |

### 2) 6196166 — SF/OCC **0.128**, wall **7.8×** (n_tx=108) — PARK WORST

| Face | Evidence |
|------|----------|
| N3 | park_idle≈**1.19**, rewind=166, rebind=7, bind=1.3k |
| Process | idle≈**1.19**, park_ns≈79 ms on ~8 ms wall, steal=48, prefer=21, residual=711, id=346, ff=388, rebind=3, rewind=150 |
| RH | cold=227, canary=241, wait_serial=44, wait_writer=12, writer_done=0 |
| Why | Clique WaitFor **serializes** call-frame opcode-seconds; steal fills Ready but parked frames burn wall |
| Compute vs CC | **COMPUTE schedule** primary; CC R2 secondary |
| file:fn | `vm.rs::maybe_wait_specfence` / `fence_wait_for`; `scheduler.rs::admit_spine_heat`; `learner.rs::note_park_heat` / `prefer_admit_heat` |
| Ideal TPS | Hot Region **serial lane** without parking 8 workers; cold OCC-parallel |
| Learn | Used: prefer_admit heat (21). Unused: park_ns does not remove WaitFor when ROI negative |

### 3) 19469096 — SF/OCC **0.185**, wall **5.4×** (n_tx=250)

| Face | Evidence |
|------|----------|
| Process | rewind=449, rebind=5, wait=164, idle≈**0.77**, residual=1358, id=391, ff=993, prefer=30, collapse=38 |
| Class | R2 + WaitHard morph; serial WaitFor + SuffixRepair cascade |
| file:fn | same Resolve + Wait path as above |

### 4) 8889776 — SF/OCC **0.188**, wall **5.3×** (n_tx=330)

| Metrics N3 | rewind=70, rebind=7, bind≈678 |
| Class | R2_SUFFIX_REPAIR_DOMINANT mid-hot |
| Grain | C+ (no tip process in top10 digest set for this bn — use metrics) |

### 5) 14029313 — SF/OCC **0.193**, wall **5.2×** (n_tx=724) — META/COLD

| Process | bind=431, rewind=26, rebind=3, unf=2621, canary RH=1253, cold=372, indep=996, idle≈0, prefer=0 |
| Why | Not R2 volume — **first-wave canary/cold rediscovery + Avoid lag** vs OCC ~3 ms |
| Compute vs CC | **CC protocol meta / cold discovery** |
| Ideal | Unfenced path ≡ OCC cost (no canary tax class) |

### 6) 14689597 (597) — SF/OCC **0.216**, wall **4.6×** — FULL SUBGRAIN + tip process

L1: morph=`fan_out`, wave_width=**434**, chain=29, max_program_fanout=**448**. Ideal parallel: P≈8 on wave; serialize only RAW star/chain.

| tx | b/w/u | park | inc | Root (access/edge/frame/slot) |
|---:|------:|-----:|----:|-------------------------------|
| 72 | 25/5/40 | 5 | **9** | Star Bind OK; each R2 inc re-cold private ℓ (cold 37); WaitFor parks |
| 71 | 40/1/38 | 1 | 6 | Satellite; cold rediscovery |
| 60 | 24/1/41 | 1 | 5 | Cold rediscovery |
| 43 | 51/0/27 | 0 | 5 | Pure repair thrash, no park |
| 29 | 44/3/28 | 3 | 3 | Clique RAW; Fence Wait + canary/cold |
| 11 | 89/0/34 | 0 | 4 | Heavy Bind cover — **Resolve not Avoid** |

Tip process: idle≈0.49, prefer=54, residual=862, rewind=63, rebind=2, cold RH=2223, writer_done=0.  
**Not Avoided:** closed. **Not Resolved:** R2 + cold-on-inc despite `inc_carry` land — private ℓ still rediscovered as UnfencedCold.  
file:fn: `rem.rs::inc_carry_seen` landed but cold still dominates RH; `vm.rs` Unfenced→residual only when sketch residual/force_writer.

### 7) 19860366 — SF/OCC **0.227**, wall **4.4×**

WAIT_PARK + R2 mix; park_idle≈0.42 (N3). PreferAdmit live but Resolve still expensive.

### 8) 15199017 — SF/OCC **0.227**, wall **4.4×** (n_tx=866)

| Process | bind=206, rewind=9, rebind=0, unf=2374, canary=1757, cold=109, residual=142, prefer=0, idle≈0 |
| Class | META_COLD — heuristic quiet/quiet_ish; wall gap from canary/meta on large n_tx |
| Ideal | OCC-default Unfenced; Fence off |

### 9) 6137495 — SF/OCC **0.229**, wall **4.4×** (n_tx=60)

WAIT_PARK; idle≈0.35; small block but WaitFor clique burns wall vs OCC <1 ms.

### 10) 14545870 — SF/OCC **0.237**, wall **4.2×** (n_tx=456)

R2 dominant (rewind=202, rebind=1 on N1); peer of hot post-Merge fan_out.

---

## 5. Focus family + quiet/mixed samples

| Block | L1 morph | SF/OCC (N3) | wall × | fail mode | Ideal parallel note |
|------:|----------|------------:|-------:|-----------|---------------------|
| 14689597 | fan_out (wave 434, fan 448) | 0.216 | 4.64 | WAIT_PARK | Serialize star only; OCC-parallel wave |
| 19606599 | long_chain (chain 61) | 0.298 | 3.36 | R2 | Chain Fence ROI may exist; still R2 cascade |
| 19469097 | long_chain (chain 47) | 0.290 | 3.45 | R2 | Same |
| 19606598 | quiet (wave 80) | 0.316 | 3.16 | BIND_RESIDUAL | Should be near quiet parity; residual Bind tax |

**Quiet samples (SF/OCC≥1):** early tiny + `14689595/99`, `19934116`, … — Fence≈off ⇒ SF beats or matches OCC.  
**Mixed:** `14029313` META_COLD; `14334629` R2/BIND mix.  
**Mid fan_out ≈ median 0.30:** residual Bind + writer_done learning **used**; R1≈0; OCC still ~3× wall.

---

## 6. Conflicts: Avoided vs Resolved (edge grain)

| Class | Status on tip | Evidence |
|-------|---------------|----------|
| Done∅Data → UnfencedWriterDone | **CLOSED** | writer_done=0 on digests |
| Avoid ∧ Unfenced (u_aa) | **CLOSED** | u_aa≈0 |
| Hot star after Fence Unfenced | **CLOSED** | hot_after_fence=0 |
| Cold/canary first-wave on long-tail ℓ | **OPEN** | cold+canary dominate Unfenced RH |
| R2 vs R1 on true_suffix value change | **OPEN dominant** | rewind ≫ rebind; value_stable false ⇒ R2 correct but **expensive vs OCC reincarnation** |
| WaitFor park makespan | **OPEN secondary** | 6196166 / 597 / 19469096 |
| SoftWait / Await@a | **BAN HELD** | 0 / 0 |

**Below-tx law:** failures are **access / edge / frame / schedule-slot**, not “tx failed.” Repair incarnations re-open cold edges; WaitFor parks burn frames while independents exist.

---

## 7. Parallel compute vs concurrency control

| Class | When | Dominant blocks | Actuator today | Why still loses to OCC |
|-------|------|-----------------|----------------|------------------------|
| CC Avoid miss | Unfenced on essential | — | residual Bind | Mostly closed |
| CC Resolve miss | abort → R2 body | R2×36 | R1-first door | True value change ⇒ R2 inevitable; SF R2 **>** OCC reincarnation cost |
| CC cold rediscovery | UnfencedCold × inc | 597 tx72, META_COLD | inc_carry | Carry incomplete vs private ℓ; canary tax class |
| CC protocol meta | Edge SM + Bind×N | all fan_out | classify_edge | Paid even when Fence doesn't reduce aborts |
| COMPUTE park | WaitFor BlockingOther | WAIT_PARK×14 | PreferAdmit | Parks burn wall; doesn't serialize Region cheaply |
| Wrong Fence on indep | rare | — | independence cert | OK |

**Cost model (falsifiable):**
```
wall = useful_EVM + wait_idle + abort_recovery + protocol_meta
OCC ≈ useful_EVM + cheap_reincarnation
SF  ≈ useful_EVM + wait_idle + SuffixRepair_body + Edge/Bind/canary_meta
```
When Fence does not strictly cut abort_recovery below OCC reincarnation, SF wall > OCC wall by construction.

**TPS derivation from block info:** useful parallel width ≈ `min(P, L1.wave_width)`. Fan_out 597: wave=434 ⇒ P=8 saturated if schedule work-conserving. Contended RAW star/chain must serialize — but serialization via **WaitFor park across workers** is the wrong mechanism; a **hot Region serial lane** keeps other workers on wave independents. Long_chain: critical path ≈ chain_length × tx_cost; Fence on chain can reduce abort cascade **only if** Wait tax < expected OCC abort×reexec on that chain.

---

## 8. Learning audit on tip (post-v2)

### Used (live → structure)

| Signal | nz blocks /99 | Sink |
|--------|--------------:|------|
| writer_done_learned / bind_residual | 76 | residual Bind / H — **KEEP class** |
| canary_reopen | 78 | probe→Fence |
| prefer_admit | 53 | Ready spine under park heat |
| fanout_fr_collapse / absorb | 58 / 46 | structural spine — landed |
| pack_top / morph_hat | end_block | InterBlockPrior decay |

### Landed but does not change cost class

| Signal | Why unused as cost-class win |
|--------|------------------------------|
| writer_identity_preserved (75) + journal_ff_hits (76) | Observe-time abundance; **true_suffix value_stable** still false → R1 cannot fire; rewind:rebind stays 10–200× |
| r1_first_bias / note_resolve_r1 | Widens only `!true_suffix` identity path; hot aborts are true_suffix |
| inc_carry_seen | Survives reset; cold RH still dominates (597 tx72-class private ℓ) |
| quiet_fence_revoke | **0** — quiet_fence_off works as seed skip; revoke unused |
| engagement_switches | **0** — banned from edge (good) |

### Missing (would need to change cost class — or delete)

1. **Fence ROI / makespan gate** — admit Fence only if predicted Δmakespan < 0 vs OCC path.  
2. **Unfenced ≡ OCC path** — same reincarnation cost; no canary/Edge tax class when Fence off.  
3. **Hot Region serial lane** without multi-worker WaitFor park.  
4. **Repair reincarnation cost** aligned to OCC (or skip repair theater).  
5. L1 RAW depth online for ROI prior (offline DAG already falsifies).

### Dead theater still in tree (must delete in v3 land, not re-tune)

`AdaptiveParams` αβγδ, `choose_resolve` / AEC EV, SoftWait meta_ops, Morph Storm as π, tip-identity Bind refuse — already banned from live edge; strip from SoT and eventually code.

---

## 9. Why SF ≪ OCC after Region/Fence/residual/R1/PreferAdmit/all-blocks

One paragraph (expanded in v3 SoT): SpecFence still **buys conflict information with a higher cost class** — Edge/Bind/canary meta and WaitFor idle — then often **still pays SuffixRepair body** when values truly change. OCC buys the same information with **cheap discovery reincarnation** only. v2 closed access leaks and opened an R1 door that cannot fire on true_suffix value changes; it therefore could not move median SF/OCC (0.356→0.326). The structural mistake is treating Fence as default insurance rather than a **strict makespan optimization** over an OCC-cost baseline.

---

## 10. Artifacts index

| Path | Role |
|------|------|
| `lab/results/arch-v2-all-blocks-sf-occ-sweep.json` | 99 pairs N=1 @ tip |
| `lab/results/arch-v2-all-blocks-sf-occ-n3-outliers.json` | N=3 overlay |
| `lab/results/arch-v2-all-blocks-sf-occ-sweep-corrected-summary.json` | distribution + fail modes |
| `lab/results/arch-v2-process-*.json` | tip process digests |
| `lab/results/post-subgrain-per-tx-*-c8.json` | focus below-tx |
| `lab/results/l1l2-b{597,599,097,598}.json` | L1 DAG |
| `lab/notes/specfence-complete-architecture-v3.md` | Part C SoT |

