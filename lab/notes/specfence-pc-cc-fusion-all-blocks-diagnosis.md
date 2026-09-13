# SpecFence PC⊗CC fusion — all-blocks diagnosis

**Date:** 2026-09-13 (Asia/Shanghai, UTC+8)
**Tip:** `4a91b5f` (`cursor/specfence-clean-slate-8598` / parallel-compute plant)
**Vocab:** Spec=Region; Fence=Bind/WaitFor/serial-lane/ordered_admit; Soft=0 ban; no P0/P1/P2 staging in design
**π SoT (frozen fields):** `lab/notes/specfence-complete-architecture-v4-frozen-grain.md` — **plant/π may be superseded** by v5 where evidence demands (called out below)
**PC SoT (this tip):** `lab/notes/specfence-parallel-compute-architecture.md` + `impl.md` + `parallel-compute-sweep-summary.json`
**Companion catalog:** `lab/notes/specfence-pc-cc-fusion-per-block-catalog.json`
**New arch SoT:** `lab/notes/specfence-complete-architecture-v5-pc-cc-fusion.md`
**Honesty:** committed median SF/OCC **0.744**; this Soft=0 N=1 rerun nonempty median **0.802**; quiet median **0.950** (19/41); fan_out/spine tails still weak. **No celebration of 0.744.**

---

## 0. Executive answer

### Why OCC⊗PCC fusion failed

Parallel-compute split OccKernel/PccKernel **and** frozen-π OCC⊗PCC were **grafted**, not fused. The computer answers “which incarnation kernel?”; π answers “Bind/Wait/Unfenced for this \(a\)?”. In the plant they only meet at `mark_pcc` after `TryPcc`, and `TryPcc` only after **intra-abort** PE ∩ ROI. So:

1. **Default path is OccKernel + UnfencedOcc** for almost every access (`unfenced_occ_fast` **284 594** vs `pcc_fire_at_a` **3 040** across 98 nonempty).
2. **Conflict is discovered at validate** → OccKernel B0 (`full_restart` **=** `occ_aborts` **5 680**; `rebind_only` **5**; `rewind_to_cp` **1**). Timely Resolve (R1a/R1b) is effectively **dead**.
3. **PE trains late and at the wrong place:** OccKernel abort uses `loc_k.or(Some(1))` because rem/`first_k` is banned on OccKernel — so star RAW that lives at **\(k≈6\)** (597 effect-raw) is learned as **k-class 1**. Next incarnation still misses PE at the real access.
4. **Switch is not timely and not at the right grain:** switch = first Fire on an incarnation (tx-kernel upgrade), not an access-local state machine. Prior PE is intentionally `roi_skip` Unfenced (`pcc_makespan_win` requires intra abort). Quiet morph freezes Fence off until abort heat — correct for 2179522 Bind-tax fear, fatal for fan_out first wave.
5. **Parallel-compute stages are incomplete:** `next_sf_task` = Block-STM indices + wave park. `wait_park_count` is high while `edge_wait_for≈0` — parks are **OCC ESTIMATE→Blocking**, not PCC serial-lane. execute∥validate pipeline / per-worker steal deques **not built** (impl honesty).

**One line:** fusion failed because mode is an **incarnation kernel fork** gated by **post-abort PE**, while wall is decided by **access/edge schedule + repair class** — and repair class collapsed to **B0-only**.

### Is the switch timely? Right place?

| Question | Verdict | Evidence |
|----------|---------|----------|
| Timely? | **No** | Fire≪aborts on worst blocks; PE after B0; first-wave star consumers Unfenced |
| Right place? | **No** | Kernel keyed by `tx_idx`; Avoid π wants \(a=(t,k,\mathrm{depth},\ell)\); training uses residual **k=1** |
| Right stage? | **No** | Avoid should arm at Detect/edge visibility; Resolve should arm at validate-fail grain; plant arms PCC at Fire and Resolve only on PccKernel |

---

## 1. Method (what was actually run)

| Step | Artifact | Soft |
|------|----------|-----:|
| Inventory 99 dirs under `data/ethereum/blocks` | 98 nonempty; empty **19910734** | — |
| `specfence_all_blocks_sweep` N=1 @8 | `lab/results/parallel-compute-all-blocks-sweep.json` | **0** |
| Focus+worst subset N=1 | `lab/results/parallel-compute-focus-worst-sweep.json` | **0** |
| Process digests top-worst | `lab/results/all-blocks-process-*.json` | **0** |
| Effect-raw / contiguous (offline, prior) | `effect-raw-*-b{597,599,097}.json`, contiguous-segments | — |
| Post-subgrain per-tx (prior HEAD, still below-tx truth for RAW/inc) | `post-subgrain-per-tx-*-c8.json` | 0 |

Exclude-set counters on this tip: force_prefix_as_pi / canary_live_verb / inc_avoid / H-OR ≈ **0** on SF rows checked. SoftWait Soft = **0** all pairs.

---

## 2. Headline numbers (ruthless)

| Metric | Committed honesty (`parallel-compute-sweep-summary.json`) | This Soft=0 N=1 rerun |
|--------|----------------------------------------------------------:|----------------------:|
| nonempty median SF/OCC | **0.744** | **0.802** |
| p10 / min | 0.468 / **0.234** | 0.580 / 0.315 |
| quiet median (heuristic) | **1.020** (24/46≥1) | **0.950** (19/41) |
| fan_out named 14689597 | **0.535** | **0.606** (focus rerun 0.367) |
| quiet tail 2179522 | **0.234** | **1.354** (N=1 variance — do not advertise) |
| Soft / await | 0 / 0 | 0 / 0 |

### Aggregate kernels / repair (98 nonempty)

```
{
  "occ_kernel_execs": 41073,
  "pcc_kernel_execs": 3004,
  "occ_kernel_validates": 84317,
  "detect_accesses": 261593,
  "unfenced_occ_fast": 284594,
  "predicted_essential_hits": 3107,
  "pcc_fire_at_a": 3040,
  "pcc_roi_skip": 67,
  "edge_bind": 3029,
  "edge_wait_for": 11,
  "edge_unfenced": 284594,
  "occ_aborts": 5680,
  "full_restart_B0": 5680,
  "rebind_only_R1a": 5,
  "rewind_to_cp_R1b": 1,
  "prefix_skip_roi_b0": 2238
}
```

**Read this table as the fusion autopsy:**
- OccKernel execs **≪** OccKernel validates → validate stampede / re-validate.
- PccKernel ≈ Fire ≈ Bind; WaitFor **11** total — PCC is almost Bind-only theater.
- B0 full_restart **locks 1:1** with occ_aborts; R1a/R1b ≈ **extinct**.
- `prefix_skip_roi_b0` **2238** is a **counter smell** (ROI/B0 path metric) without PrefixSkip Resolve wins.

### Cohort mix (catalog)

```
{
  "QUIET_PARITY": 20,
  "QUIET_META_GAP": 21,
  "MIXED_WEAK": 18,
  "PARK_ESTIMATE_STAMPEDE": 5,
  "LATE_PE_B0_ONLY": 7,
  "PCC_BIND_WITHOUT_RESOLVE": 27,
  "EMPTY": 1
}
```

Dominant live classes on this tip: **PCC_BIND_WITHOUT_RESOLVE** (27), **QUIET_META_GAP** (21), **QUIET_PARITY** (20), **LATE_PE_B0_ONLY** (7), **PARK_ESTIMATE_STAMPEDE** (5).

---

## 3. Top 10 concrete failure modes (block + tx + access)

### FM1 — Late PE / wrong-k train (Avoid miss)

- **Block:** 14689597 (fan_out/spine), also 19807137
- **Access:** star \(\ell=85335018835337005\), consumer **tx∈{5,8,11,20,28,29,43,60,71,72,…}**, **`first_program_cross_k=6`** (effect-raw)
- **What ran:** OccKernel UnfencedOcc → validate fail → `note_abort_access(ℓ, …, Some(1))` residual
- **Should:** PE(\(ℓ\), k_class≈4–7) **before** second wave; TryPcc→Bind/ordered_admit at \(a\)
- **Break:** PC (no rem first_k on OccKernel) ⊗ CC (PE gate needs intra + matching k)

### FM2 — B0-only Resolve miss

- **Block:** 19606599 tx**322** (spine tip); 19469097 tx**322**; 597 tx**11/43**
- **Evidence:** `rebind_only≈0`, `rewind_to_cp≈0`, `full_restart=occ_aborts`; identity counters historically high but unused
- **Should:** R1a RebindThis when value-stable; R1b PrefixSkip certified prefix; residual B0 only
- **Break:** OccKernel validate **forbids** Resolve museum; PccKernel rare so Resolve never entered

### FM3 — ESTIMATE park stampede (compute idle, not Fence)

- **Block:** 19807137 (`wait_park_count≈859`, `edge_wait_for=0`, `pcc_fire≈2`); 6196166 (`park≈78`, `pcc_fire=0`)
- **Access:** OCC Blocking on ESTIMATE writers — schedule face
- **Should:** ready-set steal of independent Execute; serial-lane token only on PE RAW; pipeline validate on other core
- **Break:** PC schedule still Block-STM+wave; park counter ≠ PCC WaitFor

### FM4 — Prior PE learned but unused (roi_skip)

- **Mechanism:** `seed_predicted_essential` / prior pack → `predicted_essential=true` but `pcc_makespan_win=false` → `UnfencedOcc{roi_skip:true}`
- **Blocks:** any with `pcc_roi_skip>0` (aggregate 67; focus 19434587 roi=12)
- **Should:** event-driven upgrade when \(e_{vis}\) shows executing writer or repeated abort on same \((\ell,k)\) — not “never Fire on prior”
- **Break:** CC ROI salad protects quiet Bind-tax but starves first-wave Avoid

### FM5 — Kernel upgrade at wrong grain

- **Code:** `KernelTable` per `tx_idx`; `mark_pcc(tx)` upgrades **whole incarnation**; next `begin_execute` resets to Occ unless repair_armed
- **Block:** 17666333 / 15538827 — some PccKernel execs but still B0 (`rew=0`)
- **Should:** access-local mode SM; rem/journal scoped to PE accesses / certified prefix, not tx-kernel fork
- **Break:** plant identity “OccKernel ≡ OCC computer” vs π “mixed verbs in one tx”

### FM6 — Detect without Avoid (meta on quiet/META)

- **Block:** 14029313, 15199017, QUIET_META_GAP cohort
- **Evidence:** detect thousands, `pcc_fire=0`, SF wall > OCC with near-zero Fence
- **Should:** Detect atomic only (OK) **and** SF schedule/validate ≡ OCC when PE empty — quiet≈1.0
- **Break:** leftover meta (bump_k when has_pe; validate paths; wave) even when Fire=0; N=1 quiet variance

### FM7 — Fan-out clique satellites thrash

- **Block:** 14689597 txs **71–74, 60, 43** (tiny gas, high inc)
- **Access:** single program RAW to star at k=6; private ℓ cold rediscovery each B0
- **Should:** Bind star once; certified prefix skip; independents stay in ready set
- **Break:** CC Resolve miss + PC ready-set miss

### FM8 — WaitFor underfire / serial-lane unused

- **Aggregate:** `edge_wait_for=11` vs `edge_bind=3029` vs historical post-subgrain wait~50/block on focus
- **Block:** 19469097 wait=1; 597 wait=0 this tip
- **Should:** WaitFor/serial-lane while writer Executing; Bind on Data; never fleet SoftWait
- **Break:** PCC overlay rarely reached; when reached prefers Bind if Data already present (after abort delay)

### FM9 — validate-first / OccKernel validate tax

- **Evidence:** `occ_kernel_validates` **84 317** vs `occ_kernel_execs` **41 073**
- **Block:** 19606599 validates 2935 vs execs 732
- **Should:** pipeline validate as cheap stage; steal prefers Execute useful_EVM
- **Break:** PC pipeline unused; Block-STM validation-first stampede remains

### FM10 — Cross-block prior / morph unused as structure

- **Evidence:** morph heuristic labels fan_out/spine but PreferAdmit/multi_spine thin; HotSet size observed; sketch `mark_access_class` on abort with wrong k
- **Should:** learning drives **admission structure** (wave edges, serial-lane classes), not only PE bool
- **Break:** learned-but-unused (HotSet, WŜ observe, morph actuator banned — OK — but no replacement structure learner)

---

## 4. Per-block deep dives (focus + worst)

### 14689597 — 597 fan_out — fusion poster child

- **SF/OCC:** 0.606 | morph=spine | cohort=PARK_ESTIMATE_STAMPEDE
- **Kernels:** occ_exec=1166 pcc_exec=1 occ_val=1878 fire_frac=0.0009
- **Access:** detect=7342 unf_fast=7357 pe_hits=1 fire=1 roi_skip=0 bind=1 wait=0
- **Repair:** aborts=79 B0=79 R1a=0 R1b=0
- **Schedule:** park=180 steal=466 prefer=0
- **Late switch:** True
- **PC breaks:** ['idle_or_park_from_OCC_ESTIMATE_Blocking_not_PCC_WaitFor', 'validate_stampede_occ_kernel_validates_gt_execs', 'PccKernel_present_but_Resolve_museum_unused_still_B0']
- **CC breaks:** ['Resolve_miss_wrong_B0_vs_PrefixSkip_or_RebindThis', 'star_RAW_at_k≈6_trained_as_k=1_residual_on_OccKernel_abort']
- **Learning:** should=['PE_undertrained_vs_abort_volume', 'Resolve_R1a_R1b_never_entered_B0_only', 'cross_block_or_effect_RAW_k_unused_for_timely_Avoid', 'effect_RAW_first_cross_k_available_offline_not_online'] unused=[]
- **Effect-raw:** {'max_program_fanout': 448, 'longest_final_rw_chain': 29, 'n_aborts_occ8': 63, 'discovery_incarnation_mean': 2.854609929078014, 'modal_first_program_cross_k': 6, 'modal_first_program_cross_k_count': 473, 'account_grain_would_wait_frac': 0.9350460494425594, 'gap_note_head': 'block 14689597: location_RAW=647 (prog=605 hand=42); acct_grain=1073 (sload=1044 bal=0 ext=29) would_wait=0 (0.000) would_bind=1073; producer_ready_done_frac=1.000 waitish_frac=0.000; gw_p50=0.9427; waw_only_mw=0 / multi_writer=24 spurious_'}
- **Failing txs/accesses:**
  - {'tx': 72, 'inc': None, 'park': 5, 'bind': 25, 'wait': None, 'unf': None, 'source': 'post-subgrain-or-u1-per-tx'}
  - {'tx': 20, 'inc': None, 'park': 3, 'bind': 28, 'wait': None, 'unf': None, 'source': 'post-subgrain-or-u1-per-tx'}
  - {'tx': 29, 'inc': None, 'park': 3, 'bind': 44, 'wait': None, 'unf': None, 'source': 'post-subgrain-or-u1-per-tx'}
  - {'tx': 8, 'inc': None, 'park': 2, 'bind': 50, 'wait': None, 'unf': None, 'source': 'post-subgrain-or-u1-per-tx'}
  - {'tx': 9, 'inc': None, 'park': 2, 'bind': 21, 'wait': None, 'unf': None, 'source': 'post-subgrain-or-u1-per-tx'}
  - {'tx': 28, 'inc': None, 'park': 2, 'bind': 30, 'wait': None, 'unf': None, 'source': 'post-subgrain-or-u1-per-tx'}
  - {'tx': 34, 'inc': None, 'park': 2, 'bind': 39, 'wait': None, 'unf': None, 'source': 'post-subgrain-or-u1-per-tx'}
  - {'tx': 73, 'inc': None, 'park': 2, 'bind': 9, 'wait': None, 'unf': None, 'source': 'post-subgrain-or-u1-per-tx'}
- **Reason hist (process):** {'bind_published': 1836, 'unfenced_after_avoid': 0, 'unfenced_canary': 567, 'unfenced_cold': 1934, 'unfenced_independence': 622, 'unfenced_inversion': 0, 'unfenced_plant_tls': 0, 'unfenced_writer_done': 0, 'wait_for_canary': 0, 'wait_for_prefix': 0, 'wait_for_serial': 30, 'wait_for_writer': 22}

### 19606599 — 599 long_chain

- **SF/OCC:** 0.731 | morph=spine | cohort=PCC_BIND_WITHOUT_RESOLVE
- **Kernels:** occ_exec=732 pcc_exec=92 occ_val=2935 fire_frac=0.1117
- **Access:** detect=9624 unf_fast=9584 pe_hits=94 fire=92 roi_skip=2 bind=92 wait=0
- **Repair:** aborts=160 B0=160 R1a=0 R1b=0
- **Schedule:** park=118 steal=166 prefer=0
- **Late switch:** False
- **PC breaks:** ['idle_or_park_from_OCC_ESTIMATE_Blocking_not_PCC_WaitFor', 'validate_stampede_occ_kernel_validates_gt_execs', 'PccKernel_present_but_Resolve_museum_unused_still_B0']
- **CC breaks:** ['Resolve_miss_wrong_B0_vs_PrefixSkip_or_RebindThis', 'Detect_hit_PE_but_ROI_gate_Unfenced', 'spine_tip_identity_loss_inc_high_without_R1']
- **Learning:** should=['Resolve_R1a_R1b_never_entered_B0_only', 'effect_RAW_first_cross_k_available_offline_not_online'] unused=['prior_or_PE_roi_skip_n=2_decide_ignored_TryPcc', 'PE_true_but_Fire_less_than_hits']
- **Effect-raw:** {'max_program_fanout': 21, 'longest_final_rw_chain': 57, 'n_aborts_occ8': 89, 'discovery_incarnation_mean': 0.8719346049046321, 'modal_first_program_cross_k': 4, 'modal_first_program_cross_k_count': 45, 'account_grain_would_wait_frac': 0.44063492063492066, 'gap_note_head': 'block 19606599: location_RAW=584 (prog=439 hand=145); acct_grain=909 (sload=806 bal=0 ext=103) would_wait=0 (0.000) would_bind=909; producer_ready_done_frac=1.000 waitish_frac=0.000; gw_p50=0.3813; waw_only_mw=0 / multi_writer=87 spurious_h'}
- **Failing txs/accesses:**
  - {'tx': 24, 'inc': None, 'park': 6, 'bind': 20, 'wait': None, 'unf': None, 'source': 'post-subgrain-or-u1-per-tx'}
  - {'tx': 43, 'inc': None, 'park': 5, 'bind': 23, 'wait': None, 'unf': None, 'source': 'post-subgrain-or-u1-per-tx'}
  - {'tx': 51, 'inc': None, 'park': 5, 'bind': 34, 'wait': None, 'unf': None, 'source': 'post-subgrain-or-u1-per-tx'}
  - {'tx': 64, 'inc': None, 'park': 3, 'bind': 13, 'wait': None, 'unf': None, 'source': 'post-subgrain-or-u1-per-tx'}
  - {'tx': 7, 'inc': None, 'park': 2, 'bind': 17, 'wait': None, 'unf': None, 'source': 'post-subgrain-or-u1-per-tx'}
  - {'tx': 52, 'inc': None, 'park': 2, 'bind': 11, 'wait': None, 'unf': None, 'source': 'post-subgrain-or-u1-per-tx'}
  - {'tx': 65, 'inc': None, 'park': 2, 'bind': 12, 'wait': None, 'unf': None, 'source': 'post-subgrain-or-u1-per-tx'}
  - {'tx': 132, 'inc': None, 'park': 2, 'bind': 19, 'wait': None, 'unf': None, 'source': 'post-subgrain-or-u1-per-tx'}
- **Reason hist (process):** {'bind_published': 4321, 'unfenced_after_avoid': 0, 'unfenced_canary': 2480, 'unfenced_cold': 998, 'unfenced_independence': 1170, 'unfenced_inversion': 0, 'unfenced_plant_tls': 0, 'unfenced_writer_done': 0, 'wait_for_canary': 0, 'wait_for_prefix': 0, 'wait_for_serial': 20, 'wait_for_writer': 28}

### 19469097 — 097 long_chain/WAW

- **SF/OCC:** 0.718 | morph=spine | cohort=PCC_BIND_WITHOUT_RESOLVE
- **Kernels:** occ_exec=631 pcc_exec=38 occ_val=1608 fire_frac=0.0568
- **Access:** detect=5270 unf_fast=5328 pe_hits=38 fire=38 roi_skip=0 bind=37 wait=1
- **Repair:** aborts=147 B0=147 R1a=0 R1b=0
- **Schedule:** park=82 steal=112 prefer=0
- **Late switch:** False
- **PC breaks:** ['validate_stampede_occ_kernel_validates_gt_execs', 'PccKernel_present_but_Resolve_museum_unused_still_B0']
- **CC breaks:** ['Resolve_miss_wrong_B0_vs_PrefixSkip_or_RebindThis', 'spine_tip_identity_loss_inc_high_without_R1']
- **Learning:** should=['Resolve_R1a_R1b_never_entered_B0_only', 'effect_RAW_first_cross_k_available_offline_not_online'] unused=[]
- **Effect-raw:** {'max_program_fanout': 20, 'longest_final_rw_chain': 47, 'n_aborts_occ8': 86, 'discovery_incarnation_mean': 0.9345238095238095, 'modal_first_program_cross_k': 5, 'modal_first_program_cross_k_count': 48, 'account_grain_would_wait_frac': 0.5164113785557987, 'gap_note_head': 'block 19469097: location_RAW=410 (prog=341 hand=69); acct_grain=567 (sload=524 bal=0 ext=43) would_wait=0 (0.000) would_bind=567; producer_ready_done_frac=1.000 waitish_frac=0.000; gw_p50=0.6107; waw_only_mw=0 / multi_writer=52 spurious_hot'}
- **Failing txs/accesses:**
  - {'tx': 37, 'inc': None, 'park': 3, 'bind': 18, 'wait': None, 'unf': None, 'source': 'post-subgrain-or-u1-per-tx'}
  - {'tx': 69, 'inc': None, 'park': 2, 'bind': 8, 'wait': None, 'unf': None, 'source': 'post-subgrain-or-u1-per-tx'}
  - {'tx': 73, 'inc': None, 'park': 2, 'bind': 15, 'wait': None, 'unf': None, 'source': 'post-subgrain-or-u1-per-tx'}
  - {'tx': 76, 'inc': None, 'park': 2, 'bind': 13, 'wait': None, 'unf': None, 'source': 'post-subgrain-or-u1-per-tx'}
  - {'tx': 77, 'inc': None, 'park': 2, 'bind': 13, 'wait': None, 'unf': None, 'source': 'post-subgrain-or-u1-per-tx'}
  - {'tx': 309, 'inc': None, 'park': 2, 'bind': 17, 'wait': None, 'unf': None, 'source': 'post-subgrain-or-u1-per-tx'}
  - {'tx': 4, 'inc': None, 'park': 1, 'bind': 10, 'wait': None, 'unf': None, 'source': 'post-subgrain-or-u1-per-tx'}
  - {'tx': 50, 'inc': None, 'park': 1, 'bind': 5, 'wait': None, 'unf': None, 'source': 'post-subgrain-or-u1-per-tx'}

### 2179522 — quiet tail — honesty variance

- **SF/OCC:** 1.354 | morph=quiet | cohort=QUIET_PARITY
- **Kernels:** occ_exec=234 pcc_exec=0 occ_val=304 fire_frac=0.0
- **Access:** detect=55 unf_fast=463 pe_hits=0 fire=0 roi_skip=0 bind=0 wait=0
- **Repair:** aborts=1 B0=1 R1a=0 R1b=0
- **Schedule:** park=0 steal=6 prefer=0
- **Late switch:** True
- **PC breaks:** ['validate_stampede_occ_kernel_validates_gt_execs']
- **CC breaks:** ['Avoid_miss_conflict_known_only_after_abort', 'quiet_N1_wall_variance_meta_detect_vs_OCC_sample']
- **Learning:** should=['abort_without_PE_train_or_PE_wrong_k', 'Detect_atomic_only_no_feature_to_Avoid'] unused=['HotSet_observed_but_not_Avoid_gate']
- **Failing txs/accesses:**
  - {'tx': None, 'access': {'note': 'quiet'}, 'mode': 'OccKernel only', 'why': 'bind=0 pcc=0; N1 wall variance vs OCC'}
- **Reason hist (process):** {'bind_published': 101, 'unfenced_after_avoid': 0, 'unfenced_canary': 1, 'unfenced_cold': 1, 'unfenced_independence': 41, 'unfenced_inversion': 0, 'unfenced_plant_tls': 0, 'unfenced_writer_done': 0, 'wait_for_canary': 0, 'wait_for_prefix': 0, 'wait_for_serial': 0, 'wait_for_writer': 0}

### 19807137 — global worst spine/fan

- **SF/OCC:** 0.315 | morph=spine | cohort=PARK_ESTIMATE_STAMPEDE
- **Kernels:** occ_exec=2702 pcc_exec=2 occ_val=2242 fire_frac=0.0007
- **Access:** detect=12940 unf_fast=12957 pe_hits=2 fire=2 roi_skip=0 bind=2 wait=0
- **Repair:** aborts=778 B0=778 R1a=0 R1b=0
- **Schedule:** park=859 steal=741 prefer=0
- **Late switch:** True
- **PC breaks:** ['idle_or_park_from_OCC_ESTIMATE_Blocking_not_PCC_WaitFor', 'PccKernel_present_but_Resolve_museum_unused_still_B0']
- **CC breaks:** ['Resolve_miss_wrong_B0_vs_PrefixSkip_or_RebindThis']
- **Learning:** should=['PE_undertrained_vs_abort_volume', 'Resolve_R1a_R1b_never_entered_B0_only', 'cross_block_or_effect_RAW_k_unused_for_timely_Avoid'] unused=[]
- **Failing txs/accesses:**
  - {'tx': None, 'access': {'note': 'fan_out/spine mass'}, 'mode': 'OccKernel B0', 'why': 'pcc_fire≈2–3 vs aborts≈778; park≈800'}
- **Reason hist (process):** {'bind_published': 3, 'unfenced_after_avoid': 0, 'unfenced_canary': 0, 'unfenced_cold': 12828, 'unfenced_independence': 0, 'unfenced_inversion': 0, 'unfenced_plant_tls': 0, 'unfenced_writer_done': 0, 'wait_for_canary': 0, 'wait_for_prefix': 0, 'wait_for_serial': 0, 'wait_for_writer': 0}

### 6196166 — park / ESTIMATE stampede

- **SF/OCC:** 0.583 | morph=spine | cohort=LATE_PE_B0_ONLY
- **Kernels:** occ_exec=319 pcc_exec=0 occ_val=207 fire_frac=0.0
- **Access:** detect=2205 unf_fast=2229 pe_hits=0 fire=0 roi_skip=0 bind=0 wait=0
- **Repair:** aborts=79 B0=79 R1a=0 R1b=0
- **Schedule:** park=78 steal=91 prefer=0
- **Late switch:** True
- **PC breaks:** ['idle_or_park_from_OCC_ESTIMATE_Blocking_not_PCC_WaitFor', 'OccKernel_B0_repair_storm_no_PccKernel', 'meta_or_schedule_tax_with_zero_PCC_Fire']
- **CC breaks:** ['Avoid_miss_conflict_known_only_after_abort', 'Resolve_miss_wrong_B0_vs_PrefixSkip_or_RebindThis', 'park_schedule_without_serial_lane_token']
- **Learning:** should=['abort_without_PE_train_or_PE_wrong_k', 'PE_undertrained_vs_abort_volume', 'Detect_atomic_only_no_feature_to_Avoid', 'Resolve_R1a_R1b_never_entered_B0_only', 'cross_block_or_effect_RAW_k_unused_for_timely_Avoid'] unused=['HotSet_observed_but_not_Avoid_gate']
- **Failing txs/accesses:**
  - {'tx': None, 'access': {'note': 'clique'}, 'mode': 'ESTIMATE park', 'why': 'pcc_fire=0; park dominates; PE never arms'}
- **Reason hist (process):** {'bind_published': 0, 'unfenced_after_avoid': 0, 'unfenced_canary': 0, 'unfenced_cold': 2359, 'unfenced_independence': 0, 'unfenced_inversion': 0, 'unfenced_plant_tls': 0, 'unfenced_writer_done': 0, 'wait_for_canary': 0, 'wait_for_prefix': 0, 'wait_for_serial': 0, 'wait_for_writer': 0}

### 17666333 — worst-2 mass Unfenced cold

- **SF/OCC:** 0.331 | morph=spine | cohort=PCC_BIND_WITHOUT_RESOLVE
- **Kernels:** occ_exec=1299 pcc_exec=251 occ_val=2132 fire_frac=0.1619
- **Access:** detect=10009 unf_fast=11415 pe_hits=251 fire=245 roi_skip=6 bind=244 wait=1
- **Repair:** aborts=257 B0=257 R1a=0 R1b=0
- **Schedule:** park=50 steal=72 prefer=0
- **Late switch:** False
- **PC breaks:** ['validate_stampede_occ_kernel_validates_gt_execs', 'PccKernel_present_but_Resolve_museum_unused_still_B0']
- **CC breaks:** ['Resolve_miss_wrong_B0_vs_PrefixSkip_or_RebindThis', 'Detect_hit_PE_but_ROI_gate_Unfenced']
- **Learning:** should=['Resolve_R1a_R1b_never_entered_B0_only'] unused=['prior_or_PE_roi_skip_n=6_decide_ignored_TryPcc', 'PE_true_but_Fire_less_than_hits']
- **Reason hist (process):** {'bind_published': 99, 'unfenced_after_avoid': 0, 'unfenced_canary': 0, 'unfenced_cold': 7238, 'unfenced_independence': 0, 'unfenced_inversion': 0, 'unfenced_plant_tls': 0, 'unfenced_writer_done': 0, 'wait_for_canary': 0, 'wait_for_prefix': 0, 'wait_for_serial': 0, 'wait_for_writer': 3}

### 19434587 — OCC pathology neighbor / SF Bind without Resolve

- **SF/OCC:** 200.529 | morph=spine | cohort=PCC_BIND_WITHOUT_RESOLVE
- **Kernels:** occ_exec=1051 pcc_exec=228 occ_val=2197 fire_frac=0.1783
- **Access:** detect=11939 unf_fast=11751 pe_hits=250 fire=237 roi_skip=13 bind=237 wait=0
- **Repair:** aborts=349 B0=349 R1a=0 R1b=0
- **Schedule:** park=148 steal=236 prefer=0
- **Late switch:** False
- **PC breaks:** ['idle_or_park_from_OCC_ESTIMATE_Blocking_not_PCC_WaitFor', 'validate_stampede_occ_kernel_validates_gt_execs', 'PccKernel_present_but_Resolve_museum_unused_still_B0']
- **CC breaks:** ['Resolve_miss_wrong_B0_vs_PrefixSkip_or_RebindThis', 'Detect_hit_PE_but_ROI_gate_Unfenced']
- **Learning:** should=['Resolve_R1a_R1b_never_entered_B0_only'] unused=['prior_or_PE_roi_skip_n=13_decide_ignored_TryPcc', 'PE_true_but_Fire_less_than_hits']

### Remaining nonempty blocks

Every nonempty block is in `specfence-pc-cc-fusion-per-block-catalog.json` with the same schema (kernels, access_path, repair, schedule, pc/cc breaks, learning audit). Worst15:

19807137, 17666333, 19860366, 14334629, 14396881, 15274915, 8889776, 15538827, 19505152, 14029313, 6196166, 15199017, 16146267, 4864590, 14689597

Deep process digests exist for process_top / focus; grain C (metrics-only) for the rest — **honest limit**, see §8.

---

## 5. Learning audit (cohort-level)

### Should-learn-but-didn't

| Signal | Where visible | Why unused | Cost |
|--------|---------------|------------|------|
| True conflict \(k\) from interpreter ordinal | effect-raw `first_program_cross_k≈6` on 597 | OccKernel abort residual **k=1**; no lightweight k log on Unfenced | PE mismatch → perpetual Unfenced |
| Abort without PE Fire | aborts≫fire on LATE_PE / PARK cohorts | `quiet_fence_off` / `pcc_makespan_win` / wrong k-class | B0 storms |
| Edge visibility Executing writer | MvMemory last_writer + scheduler.is_executing | Only consulted inside `pcc_overlay` after TryPcc | Avoid miss |
| Account-grain would_wait (occ8) | effect-raw `account_grain_would_wait_frac≈0.94` on 597 | Not an online Avoid feature | Fan-out Wait underfire |
| HotSet abort notes | `hotset.note_abort` on OccKernel validate | Banned as Wait OR-door; **no replacement** structure prior | |
| Certified prefix length | rem first_k / CallEntry | Banned on OccKernel → cannot PrefixSkip | R1b dead |

### Learned-but-unused

| State | Updated | Dead w.r.t. live verb |
|-------|---------|------------------------|
| Prior PE seed | `seed_predicted_essential` | `roi_skip` Unfenced until intra abort |
| PE hit counter | `predicted_essential_hits` | Can be >0 with Fire=0 when ROI fails |
| HotSet / WŜ observe | OccKernel finalize observe-only | Not in `decide()` |
| sketch access class | `mark_access_class` on abort | Wrong k; not Avoid actuator |
| park_heat | schedule parks | Used to **disable** PCC when parks≫aborts — anti-fusion on stampede blocks |
| Morph weights | begin_block | Labels only; no admission structure |

---

## 6. PC vs CC — which detail broke

| Face | Symptom | Owner |
|------|---------|-------|
| Idle / park with wait_for=0 | ESTIMATE Blocking fleet | **PC schedule** |
| validates ≫ execs | validation-first | **PC pipeline** |
| OccKernel B0 = all aborts | Resolve never entered | **PC kernel split ⊗ CC Resolve scope** |
| Fire≪abort | PE late / wrong k / ROI | **CC learning + Avoid gate** |
| roi_skip on prior PE | intentional Bind-tax fear | **CC ROI** (over-conservative) |
| mark_pcc tx-global | mixed verbs theory vs kernel fork | **PC plant identity** |
| Detect atomic on every access | quiet meta gap | **PC meta** (small) + schedule |
| Soft=0 / exclude=0 | bans held | neither — hygiene OK |

**Fusion failure is joint:** CC refuses to Fire without post-abort PE; PC refuses rem/first_k on the path that would teach correct \(k\); together they guarantee **late switch at the wrong grain**.

---

## 7. How to raise TPS from block info (not only tx grain)

For each morph, ideal wall ≈ useful_EVM / min(P, wave_width) + PE serial work:

| Morph | Block info | Raise TPS by |
|-------|------------|--------------|
| fan_out (597) | wave≈434, chain≈29, star fanout≈448, first_cross **k≈6** | Early ordered_admit/Bind on \((\ell_\mathrm{star},k∈4..7)\); keep 400+ independents Execute-ready; PrefixSkip satellites; **never** B0 whole tiny txs |
| long_chain (599/097) | chain 47–61, wave 198–261 | Serial-lane on spine ℓ; R1a on tip identity; steal independents off spine |
| park (6196166) | n_tx=108, park≈fleet | Kill ESTIMATE stampede via publication/admit; serial-lane not Blocking park |
| quiet | low RAW | SF ≡ OCC computer — zero PE, zero wave tax |
| META_COLD large n_tx | OCC already fast | Cut detect/schedule meta; no canary; no prior Bind |

Grain ladder that matters: **access \(a\) → edge \(e_{vis}\) → certified frame prefix → schedule slot**. Tx-grain “this tx failed” is a summary, not a control key.

---

## 8. What could NOT be measured (honesty)

| Gap | Why |
|-----|-----|
| Per-access OccKernel vs PccKernel timeline for all 98 | Plant emits incarnation kernel counters + edge aggregates, not full access event logs online (cost) |
| Exact `mark_pcc` timestamp vs conflict existence | Would need instrumented seq on every SLOAD; used effect-raw + abort/Fire ratios as proxy |
| True idle_frac = 1 - useful_EVM/(P·wall) | No cycle-accurate useful_EVM counter on SF path this tip; park_ns / wall used as proxy |
| execute∥validate pipeline occupancy | Pipeline not implemented — cannot measure unused stage steal |
| N≥3 stability on all 98 | Cost; focus N=1 variance shown (597 0.37–0.61; 2179522 flips) |
| Post-subgrain per-tx on **this** tip | Per-tx dumps are prior HEAD; RAW/k still valid; verb mixes (canary) are **pre-parallel-compute** — do not treat canary counts as live on this tip |
| Cross-block prior effectiveness A/B | No held-out prior-off sweep this run |

---

## 9. What v5 must supersede (plant / π)

| Frozen / PC claim | Evidence | v5 change |
|-------------------|----------|-----------|
| Incarnation OccKernel/PccKernel fork as SoT | Fire≪abort; Resolve dead | **Supersede plant:** access-local mode SM; journal scoped to PE/prefix |
| OccKernel abort trains PE with residual k=1 | 597 k≈6 vs train k=1 | **Supersede plant:** lightweight access ordinal already from `bump_k_only` must key abort PE **without** rem |
| `pcc_makespan_win` requires intra abort (prior never Fires) | first-wave Avoid miss | **Supersede gate law:** event-driven Fire on \(e_{vis}\) + prior PE for known star classes; keep Soft=0 |
| Resolve only on PccKernel | R1a/R1b extinct | **Supersede:** Repair stage selectable per fail grain without whole-tx kernel upgrade |
| π fields themselves (\(a\), \(e_{vis}\), PE∨indep) | still right identity | **Keep π identity**; change **actuators/plant** |
| SoftWait / ForcePrefix / canary / inc Avoid / H-OR | exclude=0 | **Keep bans** |

---

## 10. Pointers

- Catalog: `lab/notes/specfence-pc-cc-fusion-per-block-catalog.json`
- Sweeps: `lab/results/parallel-compute-all-blocks-sweep.json`, `parallel-compute-focus-worst-sweep.json`
- v5 SoT: `lab/notes/specfence-complete-architecture-v5-pc-cc-fusion.md`
