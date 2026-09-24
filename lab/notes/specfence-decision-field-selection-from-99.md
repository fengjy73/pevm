# SpecFence — decision-field selection from 99 ethereum blocks

**Date:** 2026-09-13 (Asia/Shanghai, UTC+8)  
**Branch / tip at run:** `cursor/specfence-complete-cc-63b0` @ `712db83` (+ this note/results)  
**Status:** **CONFIRMED → FROZEN** — user confirmed field set (`可以，改写吧`); authoritative SoT: `lab/notes/specfence-complete-architecture-v4-frozen-grain.md`. This note remains the **empirical basis** (99-block DecisionFieldAgg); do not re-open exclude set without new evidence.  
**Vocab:** Spec = Region; Fence = Bind / WaitFor / serial-lane / ordered-admission; Unfenced = OCC-cost optimistic for this access.  
**Companion JSON:** `lab/results/decision-field-99-selection.json`, `lab/results/decision-field-99-aggregate.json`, `lab/results/decision-field-99-sf-occ-sweep.json`

---

## Ask

Empirically decide which candidate fields should **enter π** (live Avoid / EdgeVisibility key) vs **observe-only** (learning / metrics) vs **exclude** (noise, overfit, or banned sticky/meta), before freezing v4.1 architecture grain.

---

## Method

1. **Sweep:** SF@8 + OCC@8, N=1, **all 99** ethereum snapshots (`specfence_all_blocks_sweep`).  
2. **Instrumentation (temporary):** `DecisionFieldAgg` records feature × verb contingencies at `choose_edge_action` (every SpecFence `maybe_wait` decision).  
3. **Scores:**  
   - MI(feature; verb∈{Bind,WaitFor,Unfenced}) from aggregated contingencies  
   - Verb-mix by `k` / `depth` / `inc` / writer-status buckets  
   - Block-level Pearson(feature_rate, wall_SF/OCC) on n=80 blocks with decisions  
   - Architecture bans from v4.1 / edge A3 (not overridden by correlation)  
4. **Honesty:** MI is **correlational with the current plant** (which already ORs `force_prefix∨avoid∨essential` into Fence). Not a leave-one-out causal ablation. Access→abort linkage not wired this pass. N=1 noise on pathological OCC rows remains (see prior corrected sweeps).

### Coverage

| Metric | Value |
|--------|------:|
| Candidates / loaded / ok pairs | **99 / 99 / 99** |
| Blocks with ≥1 decision | **80** (19 quiet/empty → 0 decisions) |
| Total decisions | **252 758** |
| Verb mix | Bind **23 877** / WaitFor **72 338** / Unfenced **156 543** |
| Median SF/OCC TPS (raw N=1) | **≈0.319** |
| Quality: `wait_no_writer` | **71 711** |
| Quality: missed_avoid / false_fence_wait | **43 / 0** |

---

## Headline recommendation (pause for confirm)

### Minimal sufficient **π key**

```
a = (t, k, depth, ℓ, mode)           # access-event identity
e_vis = (writer?, published_Data?, edge_kind)   # EdgeVisibility operand
gate  = PredictedEssential(ℓ, k, morph)  ∨  independence_certified
verb  = Bind | WaitFor|serial-lane|ordered_admit | Unfenced≡OCC
        # scoped to THIS a only — never sticky Wait-on-tx
```

### Observe-only

`H` membership · `prior_warm`/`sticky` (as learning inputs, not OR-bools) · gas/opcode class · storage vs account / `is_program` · selector/`to` · morph cluster · park/wait history · validate-fail reason · R1/R2/R4 path rates · `inc` as Resolve context · `writer_validated` as metric · rare edge kinds

### Exclude from π

`inc` as Avoid/sticky key · canary as live verb · **`force_prefix` bool as π** · `H` as Wait OR-door · morph as Fence actuator · `writer_validated` as Bind gate · tx-level sticky Wait · flatten `EdgeKey(ℓ,reader)` without `k`/depth

**One-line rationale:** Verb is already almost completely determined by **PredictedEssential/avoid + published Data + independence**; **`k` and `depth` are the missing identity axes** that make mixed Fence/Unfenced inside one tx possible; **`inc`/`force_prefix`/`canary`/`H-OR` correlate with Fence but are sticky/meta tax** — exclude from live π.

---

## Field table

| Field | Enter π? | Evidence | Why |
|-------|----------|----------|-----|
| **`t` (tx / reader)** | **identity_only** | `EdgeKey.reader`; sticky Wait-on-tx banned in v4.1 | Needed in `a`; must not sticky-Wait whole incarnation |
| **`inc` (incarnation)** | **exclude** | inc0 fence≈7% vs inc1+≈74%; reinc_share↔wall weak (+0.12); ForcePrefix pathology | Repair-state proxy — harmful Avoid key; Resolve context only |
| **`k` (effect ordinal)** | **yes** | k1_3 fence≈19% vs k16p≈48%; focus 597 tx203 essential at \(k{\approx}6\) | Access-class / PredictedEssential\((\ell,k,\mathrm{morph})\); enables mixed verbs in one tx |
| **`depth` (call depth)** | **yes** | d0 fence≈12% vs d4_7≈91%; **deep_share↔wall +0.40** (strongest block corr) | Frame policy; part of `a`; highly verb-discriminative |
| **`ℓ` (MemoryLocation)** | **yes** | All Bind/Avoid keyed by location; hot fan-out ℓ pattern | Primary Region / conflict object |
| **`mode` (R/W)** | **identity_only** | `mode_read` MI=0 (maybe_wait is read-only today) | Keep in `a`; write side is Detect/publish until write Avoid exists |
| **edge kind wr/rw/ww** | **observe_only** | Decision path records Wr only today | Detect completeness; promote when rw/ww actuators live |
| **writer status** | **yes** (visibility) | validated→Bind; none→Wait\|Unfenced; **wait_no_writer↔wall +0.30** | EdgeVisibility for Bind vs WaitFor vs serial-lane — **not** PredictedEssential gate |
| **published Data?** | **yes** | MI(writer_published;verb)≈**0.31**; true→100% Bind | A3 Bind visibility bit |
| **Avoid membership** | **yes** | MI(avoid_broadcast)≈**0.34**; true fence≈1.0 | First-wave / access-class PCC signal (not OR-salad with ForcePrefix) |
| **H membership** | **observe_only** | MI≈0.25 but redundant with avoid/prior; code: H is prior not OR-door | Learning/prior; banned as Wait OR-door |
| **canary** | **exclude** | MI≈0.16 but true→mostly Unfenced discovery; v4.1 deletes canary class | Meta tax on ¬PredictedEssential |
| **independent flag** | **yes** | MI≈0.08; true→100% Unfenced | A4 Unfenced / OCC baseline certificate |
| **gas / opcode class** | **observe_only** | Not in EdgeView; depth/gross-work proxy in effect-raw | Learning / depth proxy |
| **storage vs account** | **observe_only** | `is_program` MI≈**0.005** (noise) | Diagnostic filter; account grain already diagnostic_only |
| **selector / to** | **observe_only** | Not on 99-block decision path | Inter-block morph prior only |
| **morph cluster** | **observe_only** | Banned Storm/Quiet actuator | Prior decay / warm-start only |
| **prior warm features** | **observe_only** | MI(prior_warm)≈0.34 ≈ avoid — redundant OR-bool | Feeds PredictedEssential learning; not separate π bit |
| **park / wait history** | **observe_only** | wait_frac↔wall +0.30; 6196166 park lesson | Schedule learning; prefer serial-lane over fleet Wait |
| **validate fail reason** | **observe_only** | Resolve learning (value_stable / true_suffix) | R1a/R1b/E1/B0 selection — not Avoid key |
| **R1/R2/R4 path** | **observe_only** | Resolve ladder / rewind_to_cp on fan_out worst | Falsifier + Resolve actuator — not Avoid π |
| **force_prefix bool** | **exclude** | MI≈**0.40** (strong!) but true→95% Wait; rate↔wall **+0.35**; v4.1 ban | Sticky tx-grain — **correlation ≠ license**; metrics→0 |
| **essential / PredictedEssential** | **yes** | MI≈**0.66** (strongest binary); true→100% Fence | Primary learned Avoid predicate at \((\ell,k,\mathrm{morph})\) |
| **writer_validated as Bind gate** | **exclude** | MI≈0.22 but A3: Data→Bind even if !validated | Tip-identity class Bind delay — exclude |

---

## Binary MI ranking (feature ; verb)

| Rank | Feature | MI | Fence rate F→T |
|-----:|---------|---:|----------------|
| 1 | `essential_antidep` | **0.663** | 0.00 → 1.00 |
| 2 | `force_prefix` | 0.399 | 0.17 → 1.00 (**exclude anyway**) |
| 3 | `avoid_broadcast` | 0.338 | 0.20 → 1.00 |
| 4 | `prior_warm` | 0.337 | 0.20 → 1.00 (redundant) |
| 5 | `writer_published` | 0.313 | 0.32 → 1.00 |
| 6 | `writer_present` | 0.304 | 0.31 → 1.00 |
| 7 | `in_hot_set_H` | 0.250 | 0.03 → 0.66 (observe) |
| 8 | `writer_validated` | 0.216 | 0.33 → 1.00 (not Bind gate) |
| 9 | `clique_gated` | 0.171 | — |
| 10 | `canary_ok` | 0.156 | 0.54 → 0.03 (**exclude**) |
| … | `independence_certified` | 0.080 | 0.45 → 0.00 |
| … | `is_program` | 0.005 | noise |

---

## Bucket portraits (why `k` / `depth` enter; `inc` does not)

| Bucket | Fence rate | Read |
|--------|----------:|------|
| depth **d0** | ~12% | Cold / shallow → OCC Unfenced |
| depth **d4_7 / d8p** | ~90%+ | Deep frames dominate Wait/Bind |
| k **1–3** | ~19% | Early effects often non-essential |
| k **16+** | ~48% | Late / multi-touch more Fence |
| inc **0** | ~7% | First incarnation mostly Unfenced |
| inc **1 / 2+** | ~74% | Reexec already in repair — **sticky ForcePrefix smell**, not essentialness |

**Plant smell:** `wait_no_writer` = **71 711 / 252 758** decisions — WaitFor/serial without addressable writer. Strong wall correlate (+0.30). Supports v4.1 serial-lane / ordered-admit with writer identity, not more Wait OR-bools.

---

## Relation to v4.1 finegrain SoT

This empirical pass **supports** the v4.1 access-event sketch \(a=(t,\mathrm{inc},k,\mathrm{depth},\ell,\mathrm{mode})\) **with one correction for freeze:**

- Keep **`inc` out of the Avoid π key** (identity for Resolve / reincarnation bookkeeping only).  
- Live Avoid key ≈ \((t,k,\mathrm{depth},\ell,\mathrm{mode})\) + EdgeVisibility + PredictedEssential\((\ell,k,\mathrm{morph})\).  
- Do **not** promote `force_prefix` / canary / H-OR despite high MI.

**SoT rewrite:** done in `specfence-complete-architecture-v4-frozen-grain.md` (follow-up commit after confirm).

---

## Artifacts

| Path | Role |
|------|------|
| `lab/results/decision-field-99-sf-occ-sweep.json` | Full SF/OCC@8 rows + per-block `decision_fields` |
| `lab/results/decision-field-99-aggregate.json` | Merged contingencies + binary MI |
| `lab/results/decision-field-99-selection.json` | Field table + recommendation machine-readable |
| `lab/results/decision-field-99-sf-occ-sweep.run.log` | Run log |
| `crates/pevm/src/specfence/decision_field.rs` | Temporary aggregator (lab) |

```bash
SPECFENCE_ALL_ITERS=1 SPECFENCE_ALL_PROCESS_TOP=10 \
  SPECFENCE_ALL_OUT=lab/results/decision-field-99-sf-occ-sweep.json \
  ./target/release/examples/specfence_all_blocks_sweep
```

---

## Next (after freeze)

1. ~~Confirm π field set~~ — **done**.  
2. ~~Freeze v4.1 architecture grain / EdgeKey SoT~~ — **done** (`specfence-complete-architecture-v4-frozen-grain.md`).  
3. Optional follow-up: access→abort linkage + leave-one-out ablations; drop/gate temporary `DecisionFieldAgg` once coding starts.  
4. **Pause before protocol coding** until explicit go-ahead.
