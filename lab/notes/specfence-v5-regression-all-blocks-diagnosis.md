# SpecFence v5 regression — all-blocks diagnosis (PC⊗CC fusion failure)

**Date:** 2026-09-13 (Asia/Shanghai, UTC+8)  
**Tip:** `9a49b5f` (`cursor/specfence-v5-pc-cc-fusion-e28e`, PR #7)  
**Vocab:** Spec=Region; Fence=Bind/WaitFor/SerialLane/OrderedAdmit; Soft=**0** ban; no P0/P1/P2 in design  
**Companion catalog:** `lab/notes/specfence-v5-regression-per-block-catalog.json`  
**Switch-path audit (fold in):** `lab/notes/specfence-v5-mode-a-switch-path-audit.md`  
**v5 SoT (superseded by v6 for next plant):** `lab/notes/specfence-complete-architecture-v5-pc-cc-fusion.md`  
**New arch SoT:** `lab/notes/specfence-complete-architecture-v6-essence.md`  
**Sweeps this tip:**  
- Soft=0 N=1 all 99: `lab/results/v5-regression-all-blocks-sweep.json`  
- Soft=0 N=3 focus8: `lab/results/v5-regression-focus-n3-sweep.json`  
- Process (worst15 + focus8): `lab/results/all-blocks-process-*.json`  
**Committed honesty digest:** `lab/notes/v5-fusion-sweep-summary.json` (median **0.655**)  
**PC baseline:** committed median **0.744** (`lab/notes/parallel-compute-sweep-summary.json`)

**Ruthless bar:** celebrating Mode(a)/true-\(k\)/tests-green while wall lost is forbidden.

---

## 0. Executive answer

### Why OCC⊗PCC are still not better fused

v5 **demoted** the incarnation `Occ|Pcc` fork and landed access-local `decide` + `AccessOrdinalLog` true-\(k\). That fixed the *carrier* wrongness from the PC plant. Fusion still fails because **switch is at mid-Execute access after PE exists**, while wall is decided by **first-wave schedule + repair class**:

1. **First conflict of a class is still abort-then-PE** (S1). Empty PE ⇒ Spec ESTIMATE/B0. Producer publish may seed Avoid for *siblings*, not for the doomed first reader.
2. **`access_vis` starves SoT Bind** (S2). It pushes `last_writer_before` into `unfinished` **without filtering `is_done`**, so `Data ∧ unfinished=0 → Bind` almost never fires from `decide`. Live `edge_bind` is mostly WaitFor→Data conversion / residual Bind — rem certificate without Bind-at-\(a\) clarity.
3. **Prior-PE Fence opened; quiet/makespan kill-switches deleted** (T3). Fires rose (PC ~3.6k → honesty 4.6k / this run 5.0k) and `prefer_admit` ~16k — **more Fence/rem/admit tax** without first-wave fan_out win.
4. **`note_fence` before Bind Data confirm** (T2) can leave a **certificate with no Fire verb** → `may_resolve` → Resolve museum vs OCC bool.
5. **SerialLane Ready head = prefer_admit + Spec continue** (T4) — schedule churn **without** rem/R1 progress; reader still B0.
6. **Schedule is not PE-gated** (switch audit §7). `next_task_with_wave` execute-first helps validation stampede; it does **not** refuse `Execute(t)` when PE unpublished-RAW exists. Fence = mid-execute park/admit, not ready-set admission.
7. **Repair still B0-locked:** this Soft=0 N=1 rerun `full_restart` **6803** ≡ `occ_aborts` **6803**; R1a **23**, R1b **3**. Structure↑ (Fire/Bind/admit) does not cut reincarnation.

**One line (switch audit §8):** v5 correctly demoted `mark_pcc` and fixed true-\(k\), then **opened prior-PE Fence + admit/SerialLane** while **`access_vis` still mis-counts unfinished**, so the wall paid **more Fence/rem/admit tax** without winning first-wave fan_out — honesty median **0.744→0.655**.

### Is the switch timely? Right place?

| Question | Verdict | Evidence |
|----------|---------|----------|
| Timely? | **No for first wave; maybe for reincarnation** | S1 abort-then-PE; 597 `first_avoid_seq` 13–17 after star RAW at \(k≈6\); N=3 14689597 **0.327** |
| Right place (grain)? | **Partial** | Mode(a) is access-local (good); schedule still tx-Execute; Repair grain collapses to B0 |
| Right stage? | **No** | Avoid should arm **before** Execute of doomed consumers (ready-edge); Resolve should arm at fail-\(a\) with certificate; plant arms Fence mid-read and Resolve only if `may_resolve` |
| Right visibility? | **No** | S2 unfinished synthesis; HotSet/WŜ gathered **unused** by `decide` |

### Why structure↑ wall↓ (0.744→0.655)

| Lever v5 added | Intended win | Measured cost |
|----------------|--------------|---------------|
| Mode(a) `decide` + prior PE may Fence | timely Bind/WaitFor | fires↑; quiet p10 **0.559**; Bind theater |
| Delete quiet/makespan/park gates from decide | stop false Spec | T3 tax on quiet-ish / mixed |
| `AccessOrdinalLog` true-\(k\) | PE at \(k≈6\) not residual-1 | always-on HashMap note + PE probe once any PE seeded (T6) |
| SerialLane before Bind | stop stale Data theater | `prefer_admit` 16k without cert (T4) |
| `note_fence` rem/R1 | Resolve when Fenced | museum validate; T2 cert-after-miss |
| execute-first / no burn idx | less validate stampede | still no PE refuse Execute |

**Concrete median math:** committed honesty all-blocks Soft=0 N=1 nonempty **0.655** vs PC **0.744** (−12%). This tip Soft=0 N=1 **remeasure** nonempty median **0.734** (PC file same harness **0.797**; per-block delta median **−0.060**, **67/98** regressed). N=1 walls are noisy — **do not claim 0.734 beat the regression**. Focus N=3 median **0.678** with 14689597 **0.327**, 19807137 **0.301**, 2179522 **1.402** (OCC-slow sample — honest quiet use digest N=3 **0.751** / when OCC fast ≈0.75–0.96).

---

## 1. Method (what was actually run)

| Step | Artifact | Soft | Iters |
|------|----------|-----:|------:|
| Inventory 99 dirs `data/ethereum/blocks` | 98 nonempty; empty **19910734** | — | — |
| `specfence_all_blocks_sweep` @8 | `lab/results/v5-regression-all-blocks-sweep.json` | **0** | 1 |
| Focus8 N=3 | `lab/results/v5-regression-focus-n3-sweep.json` | **0** | 3 |
| Process worst15 + focus8 | `lab/results/all-blocks-process-*.json` | **0** | 1 |
| Effect-raw (prior, still below-tx truth) | `lab/results/effect-raw-deep-b{597,599,097,098}.json` | — | — |
| Switch-path audit | `lab/notes/specfence-v5-mode-a-switch-path-audit.md` | — | — |
| vs PC pairs | `lab/results/parallel-compute-all-blocks-sweep.json` | 0 | 1 |

Exclude-set / SoftWait Soft = **0** on all SF pairs this tip.

---

## 2. Headline numbers (ruthless)

### 2.1 Medians

| Metric | PC committed | v5 honesty digest | **This tip Soft=0 N=1** |
|--------|-------------:|------------------:|------------------------:|
| nonempty median SF/OCC | **0.744** | **0.655** | **0.734** |
| p10 / min | 0.468 / 0.234 | 0.428 / 0.292 | 0.493 / **0.247** |
| quiet median (heuristic) | 1.020 (24/46≥1) | **1.050** (19/36≥1) | **0.994** (18/38≥1) |
| quiet p10 | — | **0.559** | **0.558** |
| ≥0.7 / ≥1.0 | 56 / 27 | 44 / 22 | 56 / 21 |
| Soft / await | 0 / 0 | 0 / 0 | 0 / 0 |

### 2.2 Focus N=3 (8-block)

| Block | Role | N=3 SF/OCC | N=1 this run |
|------:|------|----------:|-------------:|
| 19807137 | fan_out | **0.301** | 0.247 |
| 14689597 | fan_out/spine | **0.327** | 0.371 |
| 6196166 | fan_out | **0.395** | 0.381 |
| 6137495 | spine | 0.410 | 0.521 |
| 19469097 | spine | 0.678 | 0.749 |
| 19606599 | spine | 0.680 | 0.695 |
| 19606598 | quiet | 0.766 | 0.710 |
| 2179522 | quiet | **1.402*** | 0.964 |

\*2179522 N=3 1.402 = OCC wall pathology this sample. Digest honesty **0.751**. **Do not advertise**.

Focus N=3 median **0.678** (honesty digest focus was **0.541**). Still **not** product bar; fan_out named ≪0.85.

### 2.3 Aggregates (98 nonempty, this N=1)

```
detect_accesses           275848
unfenced_occ_fast         296961
pcc_fire_at_a               4954
pcc_roi_skip                 501
predicted_essential_hits   16874
edge_bind                   4798
edge_wait_for                156
prefer_admit               16373
occ_kernel_execs           40830
pcc_kernel_execs            4487
occ_kernel_validates       77324
occ_aborts / full_restart   6803 / 6803
rebind_only / rewind_to_cp    23 / 3
wait_park_count             3531
ready_steal_on_wait         3954
soft_wait_arms / await         0 / 0
```

**Read as fusion autopsy:** Fire≈Bind≫WaitFor; prefer_admit ≫ WaitFor; B0≡aborts; R1 extinct; park≫WaitFor ⇒ ESTIMATE Blocking not PCC serial-lane.

### 2.4 Cohort mix (this catalog)

```
PCC_BIND_WITHOUT_RESOLVE   45
QUIET_PARITY               23
MIXED_WEAK                 10
PARK_ESTIMATE_STAMPEDE      9
QUIET_META_GAP              7
LATE_PE_B0_ONLY             4
EMPTY                       1
```

Dominant live class: **Bind/Fire without Resolve** (45).

### 2.5 vs PC file per-block

67/98 nonempty **regressed** vs PC file; delta median **−0.060**. Named structural drops include 14689597 **−0.235**, 6196166 **−0.202**, 6137495 **−0.214**.

---

## 3. Top 10 failure modes (block + tx + access)

### FM1 — `access_vis` unfinished synthesis starves Bind (S2) — CC vis

- **Where:** `vm.rs::access_vis` — pushes MV `last_writer_before` into `unfinished` **without `is_done`**
- **SoT expected:** `published_Data ∧ unfinished=0 → Bind`
- **Live:** done writer still counts ⇒ decide rarely returns Bind; Spec OCC-reads Data **or** WaitFor→Bind cert path
- **Block/access:** 14689597 star `ℓ=85335018835337005`; process `wait_for`≈0–2 on hot while `bind_published` counted
- **Wall:** rem/`may_resolve` museum without Bind-at-`a` clarity

### FM2 — First-wave abort-then-PE (S1) — PC⊗CC

- **Block:** 14689597; consumers tx∈{3,5,…} `first_program_cross_k=6`, `ℓ=85335018835337005` (effect-raw)
- **What ran:** Spec → B0 → PE(true k) → **next** inc may Fence
- **Should:** ready-edge / SerialLane **before** Execute of doomed consumers
- **N=3:** 14689597 **0.327** (aborts 265); 19807137 **0.301** (aborts 1013, bind 758)

### FM3 — Prior-PE Fence tax after gate deletion (T3) — CC decide

- fire 3.6k→4.6–5.0k; quiet p10 **0.559**; 67/98 lost vs PC file
- fusion hinge without makespan/quiet **observe** brake at Fire

### FM4 — `note_fence` rem tax / cert-before-Data (T1/T2) — CC certificate

- Bind arm `note_fence` **before** `last_data_before`; miss → Unfence **with cert stuck**
- WaitFor→Data Bind inflates `edge_bind` (4798) vs WaitFor (156)
- `may_resolve` → Resolve museum vs OCC bool
- **Block:** 19807137 — fire≈813, bind≈810, wait≈2, full≈1014, rebind≈6

### FM5 — SerialLane prefer_admit without certificate (T4) — PC⊗CC

- `pcc_serial_lane`: admit_spine + ¬executing → Spec continue (no cert)
- prefer_admit **16373** vs wait **156**
- **Blocks:** 14689597 prefer≈904 wait≈2 park≈231; 6196166 prefer≈510 wait≈0

### FM6 — Schedule not PE-gated — PC incomplete

- no PE unpublished-RAW refuse `Execute(t)`
- wait_park **3531** with WaitFor **156** = ESTIMATE Blocking stampede
- **Worst:** 19807137 park≈856–918

### FM7 — B0≡aborts; R1 dead — CC Resolve

- full_restart **6803** = occ_aborts **6803**; R1a **23**; R1b **3**
- Spec miss always B0; tx-global cert from one Fence does not yield access-grain R1

### FM8 — Always-on AccessOrdinalLog + PE probe (T6) — PC meta

- `access_log.note` before empty-PE fast path; once any PE seeded, every access pays detect/k/PE
- QUIET_META_GAP; quiet p10 **0.558**

### FM9 — HotSet / WŜ / independence unused by decide — learning

- gathered in `AccessVis`; **not read** by `decide`
- independence_certified unused — cannot Unfence false PE

### FM10 — Mixed-verb intra-tx; tx-global cert — frame

- 19807137 `mixed_verb_intra_tx` **529**; 14689597 **30**
- one Fence ⇒ `may_resolve` whole tx; sibling Spec still ESTIMATE; fail ⇒ B0

---

## 4. Deep dives (worst + focus)

### 4.1 14689597 — star RAW

| | N=1 | N=3 |
|--|----:|----:|
| SF/OCC | 0.371 | **0.327** |
| bind/wait/park | 80/3/232 | 106/3/206 |
| aborts/B0 | 106/106 | 265/265 |

Effect-raw: max_program_fanout **448**; star `ℓ=85335018835337005`; first cross **k=6**. Process: first_avoid_seq **13–17**. **Not parallelized:** first-wave star consumers. **Not Avoided:** RAW before PE. **Not Resolved:** B0. **Broke:** S1+S2+T4+schedule.

### 4.2 19807137 — Bind storm + B0

N=1 **0.247** / N=3 **0.301**. bind≈750–810, aborts≈1000+, park≈850+, prefer≈2500+, WaitFor≈1–3. Hot `ℓ=6996519588683120047`. Fence width without first-wave win.

### 4.3 6196166 / 6137495

N=3 **0.395** / **0.410**. fan_out/spine Bind/B0; prefer high; wait≈0.

### 4.4 19606599 / 19469097

N=3 ≈0.68. Some WaitFor; still B0-dominant Spec path. Need real OrderedAdmit schedule tokens.

### 4.5 2179522 / 19606598

2179522 fresh process: bind=0 fire=0 — plant≈OCC + meta. Ratio noise. 19606598 N=3 **0.766**.

### 4.6 Worst15 (this N=1)

19807137, 14689597, 6196166, 8889776, 15274915, 19932148, 19860366, 4864590, 14396881, 17666333, 15538827, 19469098, 19505152, 14383540, 12522062 — catalog rows.

---

## 5. Learning audit — produced vs consumed

| Signal | Produced? | `decide()`? | Elsewhere? | Verdict |
|--------|-----------|-------------|------------|---------|
| PE intra true-k | yes | yes | sketch | used (reincarnation only) |
| PE prior seed | yes !quiet | yes may Fence | sketch | **used — regresses (T3)** |
| Serial-lane token | yes | yes | admit | **admit not progress (T4)** |
| unfinished/exec/Data | yes | yes | — | **unfinished wrong (S2)** |
| HotSet | yes | **no** | lean | **learned unused** |
| WŜ | yes | **no** | hot OR | **learned unused** |
| independence | partial | **no** | DecisionField | **unused** |
| DecisionField/morph | yes | **no** | labels | **unused at Fire** |
| quiet/makespan/park | learner | **deleted** | Resolve | **brake removed → tax** |
| AccessOrdinalLog | always | PE train | — | **over-produced quiet (T6)** |

**Should learn/consume:** done-vs-unfinished; PE as **schedule** edge; independence Unfence; Fence-vs-B0 makespan for prior-PE; first-cross k **before** consumer Execute.

---

## 6. Below-tx grain checklist

| Probe | Finding |
|-------|---------|
| decide vs conflict | Fire after PE; conflict often earlier first Spec read |
| true-k train | good first-touch; residual-1 gone; buckets coarse |
| WaitFor/SerialLane/Bind | WaitFor scarce; Bind≈Fire; prefer_admit≫cert |
| B0 vs R1 | B0≡aborts; R1 extinct |
| schedule | ESTIMATE parks; steal; no PE refuse Execute |
| frame | tx-global may_resolve from one Fence (FM10) |

---

## 7. How block info raises TPS (access/edge/frame)

| Morph | Block info | Missing lever |
|-------|------------|---------------|
| fan_out | fanout, first_cross k, star ℓ | ready-edge refuse / exclusive SerialLane permit **before** first read |
| spine | longest_rw_chain | OrderedAdmit tokens; steal off-spine |
| quiet | empty PE | skip AccessOrdinalLog/PE until PE nonempty |
| park-prone | park≫WaitFor | lane tokens replace ESTIMATE parks |
| mixed | mixed_verb_intra_tx | selective R1 at Fence locations only |

---

## 8. What could NOT be measured

1. decide→Bind vs WaitFor→Data Bind vs SerialLane→Spec split counters.  
2. A/B of `access_vis` `!is_done` filter (docs-only).  
3. Offline true DAG makespan vs SF wall.  
4. Cross-block PE seed hit-rate / false Fire provenance.  
5. Quiet N≥10 stable p10.  
6. OCC N=1 multi-second pathology rate.  
7. rem byte tax per `note_fence`.  
8. HotSet→posterior vs PE-only counterfactual.

---

## 9. Verdict → v6

v5 Mode(a) is the **right carrier shape** and the **wrong control loop**. Structure↑ wall↓. Next architecture must: (1) fix unfinished=`!done`; (2) PE-gated ready-edges; (3) no `note_fence` without successful verb; SerialLane = progress token; (4) cost-aware prior-PE Fire; (5) quiet zero-meta until PE; (6) R1 at fail-`a`, kill tx-global cert from one Bind.

**Authoritative:** `lab/notes/specfence-complete-architecture-v6-essence.md`.
