# SpecFence post-subgrain multiblock deep diagnosis

**Date:** 2026-09-13 (Asia/Shanghai, UTC+8)  
**HEAD:** `dbf9f15` (`cursor/specfence-complete-cc-63b0`, synced)  
**Vocab:** Spec = Region; Fence = Bind / WaitFor / serial-lane+admit; Unfenced = optimistic  
**Fresh artifacts (THIS HEAD, N=3 @8, tag `post-subgrain`):**
- `lab/results/post-subgrain-sf-occ.json`, `post-subgrain-flip.json`, `post-subgrain-xblock.json`
- `lab/results/exec-process-{14689597,19606599,19469097}-post-subgrain.json`
- `lab/results/post-subgrain-per-tx-{597,599,097}-c8.json`
- `lab/results/post-subgrain-multiblock-parallel-summary.json`
- `lab/results/post-subgrain-run.log`
**Compare baselines:** `exec-process-*-post-u1.json`, `exec-process-*-subgrain-fixes.json`  
**Do not treat as current:** pre-subgrain / pre-U1 `*-c8.json` process dumps.

---

## Honesty gate

| Claim | Fresh @ `dbf9f15` fact |
|-------|------------------------|
| Subgrain Done→Bind / residual closed `unfenced_writer_done` | **Confirmed 0** on 597 / 599 / 097 (post-U1 was 589 / 3237 / 1595) |
| `unfenced_after_avoid` reason + loc residual | **0** / **0** (u_aa_total=0) |
| Hot star Fence-cover | `unfenced_after_fence_on_hot_l = 0`; Bind-after-Avoid dominates star ℓ |
| SoftWait Soft / Await@a | **0 / 0** |
| SF still ~3–4.5× OCC wall | **Yes.** Median SF/OCC wall **4.41× / 3.64× / 2.68×** (597/599/097). Mean median-wall ratio **~3.36×**. Last-iter TPS mean SF/OCC **0.264** (597 last-iter was p90 blow-up 24.6 ms vs med 17.7). |
| Makespan win vs OCC from residual Bind | **No.** writer_done class closed; wall now dominated by **R2 SuffixRepair body reexec + cold rediscovery** (+ 597 WaitFor park on bad iters). |
| Live π after AEC retire | `choose_edge_action` only — not `choose_resolve` / AdaptiveParams αβγδ |

Named dominant wall class **now:** **`SuffixRepair_makespan_cold_rediscovery`**  
(was post-U1 `Region_spine_admit_and_repair_identity` with `unfenced_writer_done` as access face; access face closed, repair face remains).

Hard bans unchanged: SoftWait Soft storms, EV Await doors, tip-identity Bind gate, OCC-retry as π, 597-only hardcodes.

---

## Method

1. Rebuilt `specfence_g7_smoke` on tip; `SPECFENCE_G7_TAG=post-subgrain SPECFENCE_G7_ITERS=3 SPECFENCE_G7_XBLOCK=1`.  
2. Fresh ProcessTrace + finegrain incarnation snapshot on last SF iter per focus block; per-tx ranked dumps.  
3. Crossed with L1 DAG (`l1l2-b*.json`: morph / wave / chain / fanout / RAW sample / tx_work).  
4. Diffed counters vs post-U1 and subgrain-fixes JSON (what moved vs what still owns wall).  
5. Classified failures: **parallel compute/scheduler** vs **CC Region/Fence/Resolve** at access / edge / frame / schedule-slot grain.  
6. Audited learning: exhaustive π inventory vs live actuators post AEC retirement.

---

## What moved (post-U1 → subgrain-fixes → post-subgrain)

| Counter (597 / 599 / 097) | post-U1 | subgrain-fixes | **post-subgrain (this)** |
|---------------------------|--------:|---------------:|-------------------------:|
| `unfenced_writer_done` | 589 / 3237 / 1595 | **0 / 0 / 0** | **0 / 0 / 0** |
| `u_aa_total` | 128 / 1124 / 790 | 0 / 0 / 0 | **0 / 0 / 0** |
| `bind_total` | 760 / 550 / 462 | 1630 / 3305 / 1974 | **1432 / 3948 / 2116** |
| `bind_residual` | — | 871 / 2778 / 1493 | **671 / 3439 / 1666** |
| `canary_reopen` | — | 23 / 299 / 79 | **21 / 290 / 84** |
| `writer_done_learned` | — | 871 / 2778 / 1493 | **671 / 3439 / 1666** |
| `prefer_admit` | 0 / 1 / 1 | 0 / 3 / 0 | **1 / 7 / 6** (xblock warm higher) |
| `rebind_only` (R1) | 0 / 8 / 4 | 3 / 15 / 23 | **0 / 2 / 3** (under-fire again) |
| `rewind_to_cp` (R2) | 88 / 170 / 211 | 84 / 133 / 203 | **95 / 172 / 195** |
| `unfenced_cold` | 1566 / 1218 / 428 | 1939 / 1070 / 465 | **2110 / 1444 / 683** |
| Soft / Await@a / hot_after_fence | 0 | 0 | **0** |
| SF wall last-iter ms | 18.0 / 34.0 / 19.5 | 22.9 / 36.6 / 23.5 | **24.6† / 35.4 / 20.1** |
| SF wall **median** ms | — | 22.9 / 36.6 / 23.5 | **17.7 / 35.2 / 19.0** |

†597 last-iter = p90 (24.6); median 17.7. Report medians for wall claims; last-iter process dumps still valid for reason hist / per-tx.

**Moved:** Avoid→Done∅Data Unfenced class **eliminated** by residual Bind; Bind roughly **2–7×** post-U1; canary reopen + PreferAdmit laws **live**.  
**Did not move wall:** R2 rewind stays ~post-U1; cold Unfenced **rose**; R1 stayed rare / regresses vs subgrain-fixes dump; 597 park can still burn ≥50% of 8·wall on bad iters.

---

## Per-block portraits + wall accounting

### Shared wall accounting (last SF iter process + median walls)

| Block | morph (L1) | wave / chain / fanout | SF med / OCC med | wall × | park_ms (last) | idle≈park/(8·wall) | work_infl | max_inc | R1 / R2 / R4fr |
|------:|------------|----------------------:|-----------------:|-------:|---------------:|-------------------:|----------:|--------:|---------------:|
| **597** | fan_out | 434 / 29 / 448 | **17.7 / 4.0** | **4.41×** | 102.9† | **~0.52**† | 1.26× | 9 | 0 / 95 / 6 |
| **599** | long_chain | 261 / 61 / 14 | **35.2 / 9.7** | **3.64×** | 39.6 | ~0.14 | **1.65×** | 11 | 2 / 172 / 35 |
| **097** | long_chain | 198 / 47 / 6 | **19.0 / 7.1** | **2.68×** | 12.5 | ~0.08 | **1.71×** | 10 | 3 / 195 / 22 |
| **598** | quiet | 80 / 6 / 6 | **3.8 / 1.4** | **2.71×** | 0.2 | ~0.01 | — | — | 0 / 10 / 1 |

†597 park is last-iter (p90 wall). Median wall implies lower typical park, but steal=82 + wait txs=39 show schedule tax is structural on fan_out.

**Access mix now (last iter):** Bind dominates hot Regions; residual Unfenced is **canary + cold + independence** — **not** writer_done / after_avoid.

| Block | bind | wait | unf | canary | cold | indep | writer_done |
|------:|-----:|-----:|----:|-------:|-----:|------:|------------:|
| 597 | 1432 | 53 | 3280 | 543 | **2110** | 627 | **0** |
| 599 | 3948 | 55 | 5091 | **2527** | 1444 | 1120 | **0** |
| 097 | 2116 | 42 | 2910 | 1541 | 683 | 686 | **0** |

**Repair face:** `journal_ff_hits` 390 / 2277 / 1188 accompany SuffixRepair, yet `rebind_only` 0 / 2 / 3 — FF **replays body cheaply-ish inside R2**, but **R1 RebindOnly almost never replaces R2**. `writer_identity_preserved` 146 / 191 / 197 and `force_bind_reabort` 23 / 64 / 44 show identity is tracked but **does not cheapen redo**.

### Neighbors (xblock, cheap)

| Fam | Block | SF warm wall | SF cold | OCC | Notes |
|-----|------:|-------------:|--------:|----:|-------|
| 597 | 596 | 4.6 | 5.5 | 1.0 | PreferAdmit warm=2 |
| 597 | **597** | 18.7 | 18.6 | 5.2 | PreferAdmit warm=**12** (Ready window opens under heat) |
| 597 | 598 | 5.4 | 4.6 | 2.1 | light |
| 599 | **598** | 4.4 | 4.0 | 1.7 | quiet predecessor |
| 599 | **599** | 35.2 | 37.7 | 9.9 | PreferAdmit warm=7 |
| 097 | **096** | 16.4 | 15.6 | 3.5 | long_chain; wait_hard **110–140**; PreferAdmit warm=**10** |
| 097 | **097** | 19.9 | 20.4 | 5.3 | PreferAdmit warm=13 |

Neighbor **096** is a harsher WaitHard morph than 097; warm PreferAdmit fires, but OCC still ~4–5× faster — same repair/cold class, not a warm-H miss alone.

### Hot-ℓ Fence-cover (still holds)

| Block | star ℓ | avoid→fence seq | bind_after_avoid | unf after fence |
|------:|--------|-----------------|-----------------:|----------------:|
| 597 | `8533…7005` | canary@7 → avoid@165 → fence@210 | **539** | **0** |
| 599 | `13758…82113` | avoid@260 → fence@349 | **233** | **0** |
| 097 | `5756…35096` | canary@760 → avoid@769 → fence@864 | **127** | **0** |

Star is **not** the hole. Hole is **secondary ℓ cold + repair incarnation rediscovery + R2 default**.

---

## Concrete txs that failed to parallelize (sub-grain)

Fail_score = park + 2·inc (+ legacy u_aa/force_prefix terms now ~0). Aborts not in finegrain event map this run; **final_incarnation** is the reexec proxy.

### 14689597 (fan_out) — clique band + star satellites

| tx | gas / ops | RAW (sample) | b/w/u | park | inc | Dominant verbs | Root (access/edge/frame/slot) |
|---:|-----------|--------------|------:|-----:|----:|----------------|-------------------------------|
| **72** | 40k / 254 | ←38 on star | 25/5/40 | **5** | **9** | cold 37, wait writer/serial | **Tiny tx, max reexec:** star Bind works (25) but each SuffixRepair incarnation re-discovers private/cold slots; WaitFor parks schedule slot while clique repairs. **Frame:** R2 body reexec; **R1=0**. |
| **71** | 40k / 254 | ←38 star | 40/1/38 | 1 | 6 | cold 35 | Same satellite pattern; Bind↑ but cold per-inc. |
| **60** | 40k / 254 | ←38 star | 24/1/41 | 1 | 5 | cold 38 | Cold rediscovery owns Unfenced. |
| **43** | 40k / 254 | ←38 star | 51/0/27 | 0 | 5 | cold 24 | No park — pure repair thrash. |
| **29** | 347k / 3654 | ←28 star+sats `1770…`,`1407…`,`6790…` | 44/3/28 | 3 | 3 | canary 14, cold 13, wait_writer 3 | Clique RAW chain; Fence WaitFor parks; post-wake residual Bind on star, cold on long-tail ℓ. |
| **20** | 347k / 3661 | ←19 same sats | 28/3/20 | 3 | 3 | cold+canary+wait | Schedule slot tax on fan_out. |
| **11** | 232k / 3224 | ←9 | 89/0/34 | 0 | 4 | bind 89, cold 21 | Heavy Bind (cover) yet inc=4 — **Resolve not Avoid**. |
| **28** | 352k / 3660 | ←27 | 30/2/40 | 2 | 3 | canary/cold | First-wave canary before Avoid on secondary ℓs. |

**Class mix 597:** CC Resolve (inc≥2 on satellites) **+** COMPUTE WaitFor park on clique readers. Independents elsewhere stay UnfencedIndependence (S2 OK).

### 19606599 (long_chain / nested)

| tx | gas / ops | b/w/u | park | inc | Root sentence |
|---:|-----------|------:|-----:|----:|---------------|
| **322** | 28k / 127 | 34/0/2 | 0 | **11** | Spine tip: almost all Bind, almost no Unfenced — **identity/repair cascade**, not Avoid miss. R1 unused. |
| **51** | 105k / 4963 | 34/5/15 | **5** | 7 | Wait serial/writer parks nested frames; canary 10. |
| **24** | 186k / 12383 | 20/6/30 | **6** | 6 | Deep opcode_steps; WaitFor schedule + canary first-wave. |
| **43** | 131k / 7611 | 23/5/23 | 5 | 6 | Same. |
| **63** | 87k / 2518 | 106/0/29 | 0 | 8 | Mass Bind + cold residual; reexec without park. |
| **28 / 42 / 31** | 261–189k / 6–16k ops | 217/154/151 bind | 0 | 6–7 | Nested handler depth; cold+canary across incs; **CC Resolve**. |

Almost no over-Fence independents. Wall = **abort/repair + cold rediscovery**, not idle (idle only ~14%).

### 19469097 (WAW / long_chain spine)

| tx | gas / ops | b/w/u | park | inc | Root sentence |
|---:|-----------|------:|-----:|----:|---------------|
| **322** | 30k / 180 | 31/0/2 | 0 | **10** | Secondary-spine tip Bind-heavy; max_inc identity loss. |
| **71 / 73 / 76 / 77** | ~30k / 300 | Bind+wait serial | 0–2 | 5–6 | Tiny cohort on program chain; R2 thrash. |
| **316** | 141k / 5526 | 132/0/48 | 0 | 5 | Bind cover + cold/canary residual. |
| **37** | 139k / 10926 | 18/3/20 | 3 | 4 | Wait parks + nested depth. |
| **36 / 224** | 136–165k | high bind | 0 | 4 | Multi-spine cohort; PreferAdmit live (21 block-wide) ≠ cheap resolve. |
| **335** | 22k / 29 | 0/0/1 | 0 | 5 | Indep Unfenced only — **cascade victim**, not Fence miss. |

`multi_spine_admit=21` + PreferAdmit=6 live; still high incarnation on tips → **admit ≠ resolved identity**.

---

## Conflicts: not Avoided vs not Resolved

### Not Avoided (should Fence/Bind, got Unfenced) — **mostly closed**

| Symptom | Evidence now | Verdict |
|---------|--------------|---------|
| Done∅Data → UnfencedWriterDone | writer_done **0**; bind_residual **671/3439/1666** | **Closed** by residual Bind SoT |
| Avoid ∧ Unfenced (u_aa) | u_aa **0**; reason after_avoid **0** | **Closed** as loc residual class |
| Hot star after Fence Unfenced | hot_after_fence **0** | **Closed** |
| Cold / canary before Avoid on long-tail ℓ | cold **2110/1444/683**; canary **543/2527/1541** | **Still open** as *first-wave / long-tail H* — not the banned writer_done path |
| force_prefix ∧ writer=None | **0** | Closed (U1) |

Cold/canary Unfenced is often **correct Unfenced** on true independents or pre-Avoid probes; the damage is when those edges sit on **repair incarnations** of clique readers (cold count scales with inc).

### Not Resolved (abort → expensive R2/R4) — **dominant open**

| Symptom | Evidence | Mechanism (file:fn) |
|---------|----------|---------------------|
| SuffixRepair thrash | rewind_to_cp **95/172/195** | `rem.rs` SuffixRepair-first; `pevm.rs::try_validate` |
| R1 under-fire | rebind_only **0/2/3** despite identity_preserved **146/191/197** and ff_hits **390/2277/1188** | `value_stable` / `identity_stable_match` path loses to true_suffix / estimate / default R2 |
| Full restart / force_bind escalate | full_restart 6/35/22; fb_reabort 23/64/44 | R4 after depth cap |
| Incarnation inflation | max 9/11/10; work 1.26–1.71× | Body reexec rediscovers cold edges |
| FF hits without R1 | journal_ff_* ≫ rebind_only | FF is **inside** R2 continuation, not a substitute for RebindOnly |

**Avoid** on star: Bind-after-Avoid holds. **Resolve** after abort remains the wall — same named Region repair identity problem, now without the writer_done access face.

---

## Compute / scheduler vs CC Region/Fence/Resolve

| Class | 597 | 599 | 097 | Verdict |
|-------|----:|----:|----:|---------|
| CC residual Bind (writer_done closed) | live | live | live | Access face OK |
| CC abort/reexec (inc≥2) | 40 txs | 65 | 70 | **CC Resolve dominant** |
| CC cold rediscovery Unfenced | 2110 | 1444 | 683 | **CC / learning H** on repair frames |
| COMPUTE WaitFor park | 39 txs; idle≈0.52 last-iter | 30; ~0.14 | 35; ~0.08 | **597 secondary schedule tax** |
| Wrongly Fenced independents | ~0 | ~0 | ~0 | S2 holds |
| PreferAdmit Ready miss | 1 (cold); warm xblock **12** | 7 | 6 | Ready-window dependent; not primary wall |

**597:** fan_out **should** keep P≈8 on wave 434 independents. Steal works (82) but WaitFor BlockingOther parks **call-frame opcode-seconds** on clique readers → schedule secondary tax.  
**599/097:** chain length caps useful P; wall is almost pure **CC Resolve + cold**.

---

## Learning: missing vs unused (post AEC retire)

### Live π (exhaustive actuator map)

| Signal / prior | Wired? | Read by Fence/admit/resolve? | Status |
|----------------|--------|------------------------------|--------|
| `note_writer_done` / `writer_done_hot` | yes | `pack_top_locations` → H → `note_hot` / Fence | **Used** (counters 671/3439/1666) |
| `bind_cover` ← bind_success | yes | H / cover | **Used** |
| `reopen_canary_if_probe_done` | yes (prod `maybe_wait_specfence`) | early Fence after probe Done | **Used** (21/290/84) |
| PreferAdmit ready-set law | yes | Unfenced path + `admit_spine_writers` | **Used** (1/7/6; warm higher) |
| `wait_depth_prior` | yes | H `note_hot` only (not OR'd essential) | **Used weakly** |
| `ChainTemplate.confidence` → `template_live` | yes | serial_lane / H | **Used** |
| `choose_edge_action` | **sole access π** | Bind/WaitFor/Unfenced | **Live** |
| R1 `identity_stable_match` / value_stable | coded | `try_validate` RebindOnly | **Under-selected** (learned identity unused as cheap path) |
| `choose_resolve` / AEC EV / AdaptiveParams αβγδ | retired | not on access path | **Retired unused (intentional)** |
| `d_wait` / `cost_margin` / `meta_budget` | retired | lab/tests only | **Retired** |
| SoftWait `meta_ops` | Soft=0 | — | **Retired** |
| engagement Quiet/Storm | switches=1 | π **must not** consult (V5-P0) | **Learned morph unused by edge π** (by design) |

### Should have been learned / used but wasn't (gaps that still cost wall)

1. **Value-stable R1 as default when identity held** — identity_preserved and FF hits are abundant; rebind_only≈0. Learning/telemetry exists; **resolve policy still SuffixRepair-first**.  
2. **Cold-edge residual across incarnations** — `unfenced_cold` scales with repair; writer_done learning does not teach "this ℓ was cold-only because first wave, Bind residual next inc".  
3. **Pack_top long-tail before first Abort** — canary/cold still dominate 599 first wave; reopen helps but Avoid publish lag remains (`first_fence_seq` ≫ `first_canary_seq` on 597: 210 vs 7).  
4. **Schedule-slot PreferAdmit on fan_out parks** — PreferAdmit=1 on cold 597 while 39 txs park; warm xblock shows 12 — **Ready window + heat** not cold-start H.

### Learned but unused (post-retire inventory)

| Item | Why unused |
|------|------------|
| AEC `choose_resolve` / EV Await doors | Retired; not live π |
| AdaptiveParams αβγδ / `d_wait` / `cost_margin` / `meta_budget` | Retired from access path |
| engagement Quiet/Storm as edge actuator | Explicitly banned from π; morph→H only |
| SoftWait meta wake credit | Soft=0 |
| High `journal_ff_hits` as R1 proxy | Consumed inside R2 continuation, **not** promoted to RebindOnly count |

---

## Ideal parallel recipe from DAG → higher TPS

| Block | Ideal useful P | CP constraint | Actual SF | Native TPS lever (no if-else salad) |
|------:|---------------:|---------------|-----------|-------------------------------------|
| **597** fan_out | **~8** on wave 434; serialize only RAW chain ~29 + star readers | star ℓ + sats `1407/1770/6790` | Parks clique; R2 satellites (tx72 inc=9) | (1) **R1-first** when identity/FF match on secondary readers; (2) residual Bind already — keep; (3) **non-blocking ready steal that preserves parked frame progress** without BlockingOther wall (scheduler Continuations / park budget), not more Fence on independents |
| **599** long_chain | P≈ min(8, wave 261) but CP chain 61 caps | spine ℓ + nested frames | Bind↑, R2↑, canary storm | (1) PreferAdmit spines (live); (2) **R1 over R2** on value-stable nested reads; (3) collapse cold rediscovery on repair incs (carry Bind residual set across incarnation) |
| **097** long_chain | similar; fanout 6 | WAW spine + multi-spine | same | Same as 599; multi_spine_admit already live — pair with cheap resolve |
| **598** quiet | OCC-lite | short chain | ~2.7× still | Keep Quiet morph H; do not arm Storm Fence |

**Recipe one-liner:** *Fence only RAW CP; residual Bind on Done writers (done); PreferAdmit unfinished spines (done); **replace R2 body reexec with R1 RebindOnly whenever identity/FF says value-stable; carry cold→Bind residual across incarnation; keep fan_out workers on independents while clique Waits.***

---

## Ranked next native bottlenecks (no if-else salad)

1. **`SuffixRepair_makespan` / R1-underfire** — rewind ≫ rebind; identity+FF unused as cheap path. Native: promote value-stable RebindOnly to default when `identity_stable_match` ∧ !true_suffix (strengthen existing `try_validate` law, not new flags).  
2. **`Cold_rediscovery_across_incarnation`** — unfenced_cold owns residual access; tiny txs (597 tx72) pay max_inc. Native: incarnation-stable residual / origin map so repair does not cold-miss already-seen ℓ.  
3. **`WaitFor_BlockingOther_park` (597 fan_out)** — schedule secondary; steal≠no-wall. Native: park budget / continuation that keeps P≈8 on independents without dropping Fence correctness on clique.  
4. **`ForceBind_reabort_escalate`** — fb_reabort 23/64/44 feeds R4. Native: fewer escalate arms once R1/residual hold.  
5. **`Canary_first_wave_lag` (599)** — canary 2527 before Avoid density; reopen helps but fence_seq lag remains. Native: structural H from writer_done/pack_top earlier (already wired — tune rank, don't add OR gates).

**Not next:** re-opening SoftWait, EV Await, AEC choose_resolve, writer_done Unfenced fallthrough, tip-identity Bind gate, 597 hardcodes.

---

## Bottom line for parent

- **Dominant wall class now:** `SuffixRepair_makespan_cold_rediscovery` (writer_done=0; R2+cold own wall; 597 adds WaitFor park).  
- **Top failing txs:** 597 **72/71/60/43/29**; 599 **322/51/24/63/28**; 097 **322/71/73/316/37**.  
- **Learn gaps:** R1 identity/FF **learned but unused** as resolve; cold-across-inc **missing**; AEC/AdaptiveParams **retired unused (ok)**; PreferAdmit Ready thin on cold 597.  
- **TPS recipe:** keep residual Bind + PreferAdmit; **R1-first resolve** + incarnation-stable Bind residual; fan_out park budget for independents.  
- **Next bottleneck name:** **`SuffixRepair_makespan`** (R1-underfire).
