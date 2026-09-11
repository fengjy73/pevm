# SpecFence failing-tx sub-grain + learning gaps (post-U1)

**Date:** 2026-09-11 (Asia/Shanghai)  
**HEAD:** `e9cca35` on `cursor/specfence-complete-cc-63b0`  
**Vocab:** Spec = Region; Fence = Bind / WaitFor / serial-lane+admit; Unfenced = optimistic  
**Artifacts:** `lab/results/post-u1-per-tx-{597,599,097}-c8.json`, `exec-process-*-post-u1.json`, `l1l2-b*.json`, `subgrain-focus-and-learning-excerpts.json`  
**Companion (tx-grain):** `specfence-post-u1-multiblock-parallel-diagnosis.md`

This note goes **below tx**: access-event / EdgeKey `(ℓ, reader, k, depth)` / call-frame / opcode-seconds / schedule slots, then audits **should-learn-but-didn't** vs **learned-but-unused**.

---

## Honesty gate

| Claim | Fact @8 post-U1 |
|-------|-----------------|
| U1 `force_prefix∧None` Unfenced | **0** on all three |
| Hot star Fence-cover | `unfenced_after_fence_on_hot_l = 0`; star Bind-after-Avoid dominates |
| Dominant residual | **`unfenced_writer_done`** (589 / 3237 / 1595) + R2 `rewind_to_cp` (88 / 170 / 211) |
| Live π | `choose_edge_action` (`edge.rs`) — **not** AEC `choose_action` / `choose_resolve` |
| SoftWait Soft | **0** |
| `prefer_admit` | **0 / 1 / 1** |
| R1 `rebind_only` | **0 / 8 / 4** (rare vs rewind) |

---

## Method (sub-tx)

For each focus tx:

1. L1 `tx_work` → gas / opcode_steps / n_reads / n_writes (effect budget).  
2. L1 `reads_from_sample` → RAW `(producer→consumer, ℓ)`.  
3. ProcessTrace per-reader reasons + parks + `final_incarnations`.  
4. Hot-ℓ timeline: `first_canary_seq / first_avoid_seq / first_fence_seq` + bind/wait/unf after Avoid.  
5. Code path: `vm.rs::maybe_wait_specfence` → `choose_edge_action` → `fence_wait_for` / Bind / Unfenced.  
6. Repair: `rem.rs` SuffixRepair (R2) / ForceBind / FullRestart (R4); U4 identity counters.

No per-access event log is emitted on the SF run; chronologies are **reconstructed** from reason aggregates + seq markers + L1 RAW + the fence_wait_for state machine.

---

## A. Why top failing txs did not parallelize (sub-tx)

### Shared edge-level failure machine

```
access EdgeKey(ℓ, reader, k, depth)
  → record Unpublished Wr edge
  → avoid? / essential? / force_prefix? / canary? / independence?
  → choose_edge_action:
       Data present     → Bind
       must_fence       → WaitFor(w) or WaitFor(reader-1)
       else             → Unfenced (canary|indep|cold)
  → if WaitFor:
       last_data_before? → Bind
       unfinished Executing spine tip? → park WaitForWriter (+ admit_spine)
       force_prefix + live pred? → WaitForSerial/Prefix
       else → **UnfencedWriterDone**  ← residual wall
              (if avoid=true, loc counter u_aa++, reason verb still writer_done)
```

Root class (not “tx failed”): **Region Fence chosen, but version visibility missing when writer status is Done** → optimistic read under Avoid → later validation abort → R2/R4 reexec loses cheap Bind identity.

Star ℓs are *not* the hole (Bind-after-Avoid holds). The hole is the **long tail of secondary ℓs** + **repair incarnations** hitting the same machine.

---

### Block 597 — fan_out (wave 434, chain 29, fanout 448)

**Star ℓ** `85335018835337005`: canary@5 → avoid@128 → fence@195 → **538 Bind / 0 Wait / 6 Unf (all before Avoid)**. Cover holds.

Focus band **tx 3–36** is a RAW chain on the star + three satellite ℓs (`1407…`, `1770…`, `6790…`), each ~35k gas / ~3660 opcode_steps / 34R+23W at call_depth mostly 1 (6 effects at depth 2).

| tx | gas / ops | RAW producers (sample) | bind/wait/unf | u_aa | park | inc | Dominant access verbs | Idle while cores free? | Abort/repair |
|---:|-----------|------------------------|--------------:|-----:|-----:|----:|-----------------------|------------------------|--------------|
| **5** | 352k / 3660 | ←3 on star+sats | 10/0/78 | 23 | 0 | 2 | writer_done 44, cold 19, canary 12 | No park; busy Unfenced+Bind | inc=2 → ≥1 SuffixRepair; rebind_only=0 block-wide |
| **8** | 352k / 3657 | ←5 | 9/1/69 | 14 | 1 | 2 | writer_done 28, cold 21, canary 16 | 1 WaitForWriter park slot | same |
| **11** | 232k / 3224 | ←9 | 14/2/78 | 10 | 2 | 3 | writer_done 40 | 2 park slots; steal live | inc=3 |
| **29** | 347k / 3654 | ←28 | 11/3/67 | 7 | 3 | 4 | writer_done 24 + wait serial/writer | 3 parks — **schedule tax** | inc=4 |
| **28** | 352k / 3660 | ←27 | 11/2/67 | 7 | 2 | 3 | canary 22, cold 29, writer_done only 9 | 2 parks | cold-first-wave heavy |
| **71** | 40k / 254 | ←38 star only | 5/1/58 | 4 | 1 | **5** | cold 30, writer_done 20 | light park | **max reexec in focus** despite tiny work |
| **30** | 352k / 3663 | ←29 | 15/4/83 | **0** | **4** | 4 | writer_done 32 + **wait_for_writer 4** | **COMPUTE WaitFor** — cores stolen (steal=112) but this frame parks | wait-primary |
| **3** | 352k / 3661 | ←0 | 4/1/44 | 8 | 1 | 1 | writer_done 23 | light | early chain tip |

#### Chronology (edge / frame) — stratified

**tx5 (canonical Avoid→writer_done):**  
1. Early `k≈0..`: cold Unfenced on private slots (effect_log shows first SLOADs cold, depth 1).  
2. Program canaries fire on hot candidates before Avoid (`unfenced_canary=12`).  
3. After star Avoid@seq128: star accesses Bind (`bind_published=10` for this reader).  
4. On **secondary ℓs** where Avoid already true but writer Finished **without** readable Data: `choose_edge_action→WaitFor` → `fence_wait_for` finds `unfinished_exec=None`, `requested` Done → **`UnfencedWriterDone`** with Avoid flag → contributes to `n_unfenced_after_avoid=23` (verb hist `unfenced_after_avoid=0` by design).  
5. Wrong origin → validation fail → R2 rewind (block `rewind_to_cp=88`); identity preserved counter 92 block-wide but **body still reexec** (`final_inc=2`).  
**Root (edge):** Avoid Region without residual Data publication → Fence demotes to UnfencedWriterDone on Done∅Data writer.

**tx30 (WaitFor park):**  
Same clique RAW ←29 on star. `wait_for_writer=4`, `n_park=4`, `u_aa=0`.  
When WaitFor arms, worker parks (`ParkKind::BlockingOther`); `ready_steal_on_wait` fills other Ready txs — but **this call-frame's opcode-seconds are stalled** on Executing predecessor. After wake, many accesses still hit writer_done Unfenced (32).  
**Root (schedule+edge):** Fence WaitFor correctly serializes the tip, but post-wake visibility hole remains; park burns wall on fan_out morph that should keep P≈8 on independents (`Σpark_ns≈86.7ms` → ~60% of 8·wall).

**tx71 (tiny tx, max inc):**  
Only 3 reads / 1 write / 40k gas, RAW ←38 on star. Yet `final_inc=5`, `unfenced_cold=30` across repair incarnations.  
**Root (repair frame):** Cold rediscovery each incarnation — force_prefix now Fences (U1) but R2 does not restore Bind residual for secondary slots; cheap R1 never fires (`rebind_only=0`).

**tx28 (canary/cold heavy):**  
First-wave canary+cold dominate before Avoid propagates to the reader's ℓ set; writer_done only 9.  
**Root (first-wave):** Intra-block canary grant is one-shot; `reopen_canary_if_probe_done` **never called in production** → later similar edges stay cold Unfenced until publish.

---

### Block 599 — long_chain (wave 261, chain 61, fanout 14)

Star/spine ℓ `13758554269703882113`: avoid@216 → **232 Bind / 0 Wait / 4 Unf indep** — cover holds. Wall is **not** star miss; it is writer_done×3237 + work inflation 1.62× (max_inc **15**).

| tx | gas / ops | bind/wait/unf | u_aa | writer_done | park | inc | Root sentence (edge/frame) |
|---:|-----------|--------------:|-----:|------------:|-----:|----:|----------------------------|
| **187** | 362k / **26361** | 4/0/240 | 81 | 147 | 0 | 3 | Deep nested frames (high opcode_steps): mass secondary ℓ WaitFor→Done∅Data under Avoid; almost no Wait park — pure Unfenced storm |
| **63** | 87k / 2518 | 8/0/109 | 63 | 85 | 0 | **7** | Repair thrash on spine reader; u_aa/unf ≈58% — Avoid theatre without Bind residual |
| **42** | 189k / 7669 | 6/0/174 | 50 | 107 | 0 | 5 | RAW ←31 on spine; same Done∅Data Unfence cascade across repair incs |
| **203** | 70k / 2269 | 16/0/112 | 30 | 80 | 0 | **15** | **Worst identity loss:** 16 Binds but 15 incarnations — R2/R4 reexec loop; U4 preserves ℓ→writer counts (166) but does not cheapen redo; R1=8 block-wide rare |
| **28** | 261k / 16205 | 5/0/185 | 48 | 148 | 0 | 4 | Nested handler depth; writer_done dominates; canary 28 on first wave |
| **45** | 276k / 17667 | 5/0/155 | 48 | 86 | 0 | 4 | Same cohort as 28/31 on spine ℓ |
| **326** | 162k / 5390 | 10/1/170 | 40 | 91 | 1 | 5 | Late tip + 1 WaitFor park; still writer_done heavy |
| **31** | 162k / 5997 | 6/0/192 | 40 | 112 | 0 | 5 | Spine mid; feeds 42 |

**Cores free but tx not running:** on 599, `prefer_admit≈1`, park Σ ~18% of core·time — **not** idle-starvation. Scheduler admits spine writers, but they are usually already Executing (Ready window missed). Validation/repair keeps reincarnating the same Region readers.

**Abort path for tx203:** final_inc=15 ⇒ ~14 abort→repair cycles. Likely SuffixRepair (`rewind_to_cp`) then ForceBind escalate (`force_bind_reabort=56` block) then occasional FullRestart (31). Lost identity: certified Bind prefix + residual Data not reused as R1 RebindOnly.

---

### Block 097 — WAW / long_chain spine (wave 198, chain 47, fanout 6)

Hot fanout ℓ `5756…` Bind-after-Avoid 120; spine ℓ `1375…` Bind 105 with **2** writer_done after Avoid. `multi_spine_admit=27` live. Work inflation **1.78×** highest.

| tx | gas / ops | bind/wait/unf | u_aa | writer_done | park | inc | Root sentence |
|---:|-----------|--------------:|-----:|------------:|-----:|----:|---------------|
| **34** | 161k / 6530 | 5/0/155 | 36 | 102 | 0 | 4 | Secondary-spine UnfencedWriterDone storm; RAW ←29 on spine |
| **36** | 136k / 9057 | 5/0/85 | 32 | 50 | 0 | 4 | Multi-producer (←2 on other ℓ + ←34 spine); Avoid residual |
| **320** | 119k / 7537 | 10/0/79 | 28 | 54 | 0 | 4 | Late spine tip cohort |
| **322** | 30k / 180 | 10/0/20 | 18 | 18 | 0 | **9** | Tiny work, **inc=9** — pure repair identity thrash (almost all unf = writer_done or u_aa) |
| **224** | 165k / 9660 | 3/4/74 | 19 | 35 | **4** | 6 | WaitFor serial+writer parks (compute tax) + writer_done |
| **29** | 139k / 6384 | 4/0/84 | 27 | 51 | 0 | 3 | Mid-spine; feeds 34 |
| **25** | 101k / 5230 | 4/0/60 | 24 | 45 | 0 | 3 | ←21 spine |
| **27** | 206k / 11497 | 3/0/84 | 24 | 56 | 0 | 2 | Same cohort |

**Root (S4 incomplete):** `admit_spine_writers` runs for multi-spine, but when secondary writers are Done∅Data or not Executing, Fence falls through to UnfencedWriterDone; repair reincarnates secondary writers (`inc` high on 320/322) without R1.

---

## B. Conflicts not Avoided / not Resolved (edge grain)

| edge (ℓ, reader, writer, kind) | should have Avoided how | actually did | should have Resolved how | actually did | cost |
|--------------------------------|-------------------------|--------------|--------------------------|--------------|------|
| 597 star `8533…`, readers 3..71, Wr ← prior clique | Avoid on first publish; later Bind Data | **OK:** avoid@128, bind_aa=538, u_aa=0, unf_after_fence=0 | — | Bind works | low on star |
| 597 sat `1407…` / `1770…` / `6790…`, same clique | Same | **OK** Bind-after-Avoid (58/56/39) | — | Bind | low |
| 597 secondary hot e.g. `1483…`, Wr | After Avoid: Bind residual or WaitFor live tip — never Unfence | avoid@1063; **u_aa=4**, writer_done=4, bind_aa=0 | Publish Done residual / keep Fenced | `fence_wait_for` → UnfencedWriterDone | 4 Unfences + likely abort |
| 597 `1517…` Wait-heavy | WaitFor only while Executing; Bind on Data | wait_aa=12, writer_done=1, bind=0 | R1 on stable Data | parks + 1 writer_done | park_ns tax |
| 599 spine `1375…` | Avoid+Bind | **OK** bind_aa=232 | R1 on abort | rewind dominates | repair meta elsewhere |
| 599 secondary cluster `1275…`,`2201…`,`1474…`,`4001…` (canary~400, avoid~570, fence~4030 delayed) | After Avoid@~570, Fence immediately | **Gap:** first_fence_seq ≫ first_avoid_seq (~3500 seq later); writer_done=4 each, u_aa=4 | Fence as soon as Avoid; residual Data | late Fence + UnfencedWriterDone | ×4 ℓ ×4 events |
| 599 `1206…` / `1526…` / `2411…` | Avoid@1629 → Bind | u_aa=7, writer_done=7, bind_aa=4–5 | same | mixed Bind + writer_done | high per ℓ |
| 097 spine `1375…` | Avoid+Bind | mostly OK; **2** writer_done after Avoid | R1 | rewind 211 | secondary writers |
| 097 `1621…` / `9943…` | Avoid@356 → Fence | u_aa=3, writer_done=3, fence_seq~3330 (**late**) | early Fence after Avoid | delayed Fence + UnfencedWriterDone | tip repair fuel |
| Focus txs × many untracked cold ℓs | Learn long-tail Avoid from first abort on ℓ | **Most writer_done not in top-16 hot** (597: ~12 on hot vs 589 total) | Promote ℓ into H/Avoid from writer_done signal | signal never enters learner | **bulk of wall** |

**Not Avoided class:** Avoid true ∧ UnfencedWriterDone (loc `u_aa` 128/1124/790) — visibility hole, not U1 force_prefix leak.  
**Not Resolved class:** R2 rewind thrash; R1 RebindOnly rare; U4 identity counter up but redo cost stays.

---

## C. Learning gaps

### C1. Should-learn-but-didn't (signals in traces → never learning state)

| Signal in traces | Where emitted | Missing learner sink | file:fn |
|------------------|---------------|----------------------|---------|
| `unfenced_writer_done` per ℓ / per reader | `vm.rs::fence_wait_for` + `process.rs::record` | No `LiveLearner::note_writer_done`; not in `InterBlockPrior` / `pack_top_locations` | `process.rs` reason idx 9; `learner.rs` (absent) |
| `unfenced_after_avoid` loc counter | `process.rs` when `avoid=true` ∧ Unfenced | Not fed to Avoid confidence / decay / admit | `process.rs:~226` |
| R4 `full_restart` / `force_bind_reabort` per ℓ | `rem.rs::escalate_full_restart`; metrics | `note_abort(cascade)` only — no R4-vs-R1 feature | `rem.rs:1563+`; `pevm.rs` abort hooks |
| `wait_park_ns` on fan_out clique | metrics + `note_steal_or_park_proxy` | Updates `e_idle` but **π ignores EV** | `learner.rs:612`; `edge.rs:choose_edge_action` |
| Secondary unfinished spine writers | `sketch.push_spine` / `unfinished_writers_before` | TopLocPrior has `chain_len_ema` but no multi-spine PreferAdmit prior | `sketch.rs:198+`; `learner.rs:TopLocPrior` |
| Canary probe done without Avoid | `try_canary` consumes grant | `reopen_canary_if_probe_done` **tests only** — first wave cannot re-probe | `sketch.rs:258` (no prod caller) |
| Per-tx incarnation / work inflation | scheduler final_inc | No hook to bias R1 vs R2 or sticky Region | (absent) |
| L1 RAW producer_effect_k / call_depth | finegrain L1 (offline) | Not online features for depth-aware Fence | `finegrain.rs` L1 only |

### C2. Learned-but-unused (state updated → not read by Avoid/Fence/admit/Resolve)

| State | Updated by | Dead w.r.t. live π | file:fn |
|-------|------------|--------------------|---------|
| **AEC EV** `e_wait_time`, `e_cascade`, `e_reexec`, `e_idle` + AdaptiveParams α/β/γ/δ | `LiveLearner::note_*` | Live path = `choose_edge_action` only; `choose_resolve`/`choose_action` **orphaned** from `maybe_wait_specfence` | `edge.rs:72-150`; `mod.rs:356 choose_resolve`; `resolve.rs:compute_ev` |
| `MorphWeights.wait_depth_prior` / `dominant_fan_out` | morph EMA + observe | Only dead AEC `w_remain` + engagement mode labeling | `learner.rs:70`; `resolve.rs:172` |
| `d_wait` / `cost_margin` | lab JSON | Explicitly deprecated for π | `learner.rs:143-146` |
| `sticky_resolve` in EV | `note_sticky_resolve` | `compute_ev`: `let _ = ctx.sticky_resolve` (M123 detox); edge path uses sticky only as `essential` bit | `resolve.rs:241`; `vm.rs:529` |
| `ChainTemplate` confidence / predicted_fanout | `seed_from_prior_morph`, `decay_warm_failures` | `template_live` → serial_lane only; confidence unused by Avoid/admit | `sketch.rs:21-30,320` |
| `prefer_admit` Ready window | `vm.rs` when `is_ready(w)` | Counter ≈0 — Ready rarely observed; not a control input | `vm.rs:610-614` |
| `bind_success_total` | `note_publish` / `note_bind_success` | No production reader for π | `learner.rs:551-563,811` |
| `meta_budget_rho` / SoftWait `meta_ops` | `note_meta_op` via dead choose_resolve | SoftWait Soft=0; edge path never checks meta budget | `learner.rs:743`; SoftWait ban |
| Engagement Quiet/Storm | `set_mode_from_morph` | Escalation/EarlyVal lean only — **does not change edge verbs** | `engagement.rs`; `pevm.rs` |

### C3. Warm-start that hurts or doesn't apply

| Case | Evidence | Analysis |
|------|----------|----------|
| **598→599 flip** | `post-u1-flip.json`: flip_count 1→2; 598 TPS~20k → 599 TPS~11k; wait_hard 1→55; aborts 19→167 | Morph flip α + HotSet seed fire, but **do not teach Done∅Data visibility** or R1. Warm prior ≠ repair identity. |
| **warm≪cold myth** | SF/OCC: 598≈0.32, 599≈0.30, 597≈0.22 — ratios similar; absolute wall on 599 is repair | Quiet warm revoke (U6 `quiet_fence_revoke=0` this run) not the wall; storm block simply has more writer_done/repair |
| **Intra first-wave** | Canary grants consumed; Avoid delayed on secondary ℓs (599: avoid@570 vs fence@4030) | First wave fails to Fence later similar edges on same block — canary not reopened; Avoid without residual Data still Unfences |

### C4. What *is* learned and used (control, not dead)

- Avoid broadcast on first publish → `choose_edge_action` must_fence (`sketch/edges.broadcast_avoid`).  
- HotSet / `in_h` / `live_fanout_hot` → canary gate + serial_lane.  
- Spine list → `admit_spine_writers` / WaitFor tip.  
- `is_sticky_resolve` → essential_antidep (Fence pressure).  
- InterBlockPrior top-ℓ → seed H + template + quiet revoke.  
- U4 force_writer / residual_writer → writer identity into `maybe_wait_specfence`.

---

## Implied native fixes (not if-else)

1. **Version-visibility Region:** when Fence selected and writer Done, **publish residual / last committed Data** into Bind path — UnfencedWriterDone must not be a hang-freedom escape on Avoid/essential ℓ. Native: Region table SoT includes Done→Data residual.  
2. **Resolve = R1 when value-stable:** promote RebindOnly over SuffixRepair rewind when Bind prefix + ℓ→writer identity preserved (U4 counter already proves identity often lives). Native: repair verb from Region checkpoint, not escalate ladders.  
3. **Learn writer_done / u_aa into H and Avoid confidence:** long-tail ℓs that emit Done∅Data become hot serial_lane **within the block** (and inter-block prior), closing the “hot top-16 OK, bulk miss” gap.  
4. **Multi-spine PreferAdmit as scheduler law:** Ready writers on ℓ must be addressable before Unfenced; admit window must not only count Executing. Native: ready-set ⊆ Region unfinished spine.  
5. **First-wave canary reopen + early Fence after Avoid:** call `reopen_canary_if_probe_done` or drop one-shot grant; collapse avoid→fence seq gap on secondary ℓs.  
6. **Retire or reattach AEC:** either delete dead EV EMAs / AdaptiveParams π fields or feed them into a single Region cost that actually chooses Bind/WaitFor/Unfenced — today they are learned-but-unused theatre.

Do **not**: SoftWait Soft, morph Storm protocol doors, 597 hardcodes, Boolean `d_wait` Wait cuts.

---

## Answers (parent report)

**Sub-grain failure modes (5):**  
1. Avoid∧Done∅Data → UnfencedWriterDone (visibility hole).  
2. Repair identity thin → R2 rewind / high incarnation (R1 rare).  
3. Long-tail secondary ℓs never enter hot Avoid/Bind cover.  
4. Fan_out WaitFor park burns wall while steal runs (597).  
5. First-wave canary one-shot + late Fence after Avoid (599/097 secondary).

**Learning unused:** AEC EV/AdaptiveParams αβγδ, wait_depth_prior, d_wait/cost_margin, sticky in EV, ChainTemplate confidence, prefer_admit≈0, bind_success_total, meta_budget, engagement verbs.  

**Learning missing:** writer_done/u_aa → prior, R4-per-ℓ, park→π, secondary spine prior, canary reopen, incarnation→R1 bias, online RAW depth.

**Supporting JSON:** `lab/results/subgrain-focus-and-learning-excerpts.json`, `subgrain-focus-excerpts.json`.
