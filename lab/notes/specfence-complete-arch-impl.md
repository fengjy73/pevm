# SpecFence complete CC architecture — implementation report

**Date:** 2026-09-10  
**Branch:** `cursor/specfence-complete-cc-63b0`  
**SoT:** `lab/notes/specfence-complete-cc-architecture.md`  
**Smoke tag:** `SPECFENCE_G7_TAG=complete-arch-xblock` N=3 @8 cores + xblock  
**JSON:** `lab/results/complete-arch-xblock-{sf-occ,flip,xblock}.json` (gitignored dir; copies forced into this PR)

---

## Verdict (read this first)

**大幅 did not move.** Mean SF/OCC TPS on the four architecture cores is **0.375** (last-iter). Three-pillar on this lineage was **0.449** (different host noise; ratio still ≪1). Block **14689597** SF wall median **14.5 ms** vs OCC **5.1 ms** (~2.8×). SoftWait Soft = **0** everywhere.

Do **not** treat abort↓ / park↓ as a win: 597 SF abort median **58** vs OCC **50** (closer than three-pillar 87 vs 65) while wall stays ~3× OCC. That is the banned celebration.

**Recommend: human confirm this cut.** Next work should not add knobs. If anything: hang-free R2 under a Ready spine (WaitFor still Specs when the writer is not Executing), and stop chasing abort counts.

`m123` is **not in this tree** (no `lab/notes` / results tag). Comparison below uses **three-pillar** (`lab/notes/specfence-three-pillar-impl.md`) as the last complete drop + this-run OCC.

---

## What landed (protocol, not stubs)

Family: preset-order hybrid OCC. Control plane is `choose_edge_action`, not AEC / Storm Await@a / OCC-retry.

| Mech | Roadmap | Behavior |
|------|---------|----------|
| A1 | D1 | `HotSketch` H + chain templates; clique Spec cap after canary; spine = **lowest** writer |
| A2 | D2 | one canary Spec / hot ℓ; first wr/publish **broadcasts Avoid** immediately |
| A3 | D3 | published MV Data → **Bind** (no `writer_done`∨Validated door) |
| A4 | D4 | independence-certified (∉H, no Avoid, no template) Specs freely |
| A5 | D5 | `EdgeTable` key `(ℓ, reader, k, depth)`; R1 value-stable RebindOnly (not Storm-gated); R2 SuffixRepair / RewindTo / journal FF / ResumePath `aj`; R3 selective invalidate; **R4 FullRestart = failure** |
| A6 | D7 | `seed_from_prior` + `decay_warm_failures` / flip decay |
| — | D6 | WaitFor unpublished essential **only if** `w < reader` **and** `is_executing(w)`; BlockingOther + steal; SoftWait Soft=0 |

Hang fix (`1283b1c`): first drop WaitFor'd a **later** predicted publisher (`add_dependency(early, late)`). That deadlocked `complete_arch_edge_pi_seq_eq_par_softwait0`. Spine is now min-index; unadmitted writers Spec (work-conserving). This is a **bounded-optimism leak** vs the SoT “never Spec known essential” — documented under Gaps.

---

## File:fn map (A1–A6 / D1–D7)

| ID | File | Symbol |
|----|------|--------|
| A1/D1 | `crates/pevm/src/specfence/sketch.rs` | `HotSketch::{seed_from_prior, note_hot, clique_gated, note_writer}` |
| A1/D1 | `crates/pevm/src/specfence/edge.rs` | `choose_edge_action` (clique / H → WaitFor) |
| A2/D2 | `sketch.rs` | `try_canary`, `broadcast_avoid` |
| A2/D2 | `crates/pevm/src/vm.rs` | `Vm::execute` publish loop → `sketch.broadcast_avoid` |
| A3/D3 | `edge.rs` | `choose_edge_action` Bind-on-`bind_version` / `writer_published` |
| A3/D3 | `vm.rs` | `maybe_wait_specfence` → `bind_on_data_lite` |
| A4/D4 | `sketch.rs` | `independence_certified` |
| A4/D4 | `edge.rs` | `EdgeAction::SpecRead` |
| D5 Detect | `edge.rs` | `EdgeKey`, `EdgeTable::record` / `accesses_of` |
| A5/D5 R1 | `crates/pevm/src/pevm.rs` | value-stable RebindOnly collapse (not Storm-gated) |
| A5/D5 R2 | `specfence/rem.rs`, `boundary.rs` | SuffixRepair / RewindTo / FF / ResumePath |
| A5/D5 R3–R4 | `pevm.rs` validate abort | selective invalidate; `LeanAbortRepair::FullRestart` |
| D6 | `edge.rs` + `vm.rs` `maybe_wait_specfence` | WaitFor → `ReadError::Blocking` + BO steal |
| A6/D7 | `pevm.rs` parallel setup / teardown | `seed_from_prior`, `decay_warm_failures` |
| A6/D7 | `specfence/learner.rs` | `take_last_flipped`, `abort_rate_of` |

π is **not** `choose_resolve` / `choose_action` (AEC still compiled, unused on the wait path).

---

## Tests

| Suite | Result |
|-------|--------|
| `cargo test -p pevm --lib --release` | **112 passed** |
| `cargo test -p pevm --test specfence --release -- --test-threads=1` | **38 passed**, 20 ignored (inspect/JUMP digs, pre-existing) |
| mocked evm (`raw_transfers`, `mixed`, `small_blocks`, `beneficiary`) | **13 passed** |
| `complete_arch_edge_pi_seq_eq_par_softwait0` | pass (was hung before `1283b1c`) |
| ethereum/tests general-state | **not run** (submodule not required for this cut) |
| Alchemy RPC fetch | **not needed** (local snapshots present, including `19606600`) |

---

## Metrics vs OCC and three-pillar

Host: this cloud VM, N=3, 8 cores. Ratios are **last-iter TPS** (harness). Walls are **median**.

### Architecture cores (`complete-arch-xblock-sf-occ.json`)

| Block | SF wall med | OCC wall med | SF/OCC TPS | SF abort med | OCC abort med | Soft | wait_hard* | park_ms* | aj* | rebind* | FR* |
|------:|------------:|-------------:|-----------:|-------------:|--------------:|-----:|-----------:|---------:|----:|--------:|----:|
| **14689597** | **14.5** | **5.1** | **0.335** | **58** | **50** | 0 | 8 | 1.1 | 19 | 2 | (last-iter FR in JSON) |
| **19606599** | **24.3** | **10.4** | **0.428** | **135** | **91** | 0 | 8 | 2.3 | 13 | 10 | 13 |
| **19469097** | **16.1** | **6.0** | **0.359** | **155** | **95** | 0 | 5 | 1.6 | 3 | 15 | — |
| **19606598** | **2.9** | **1.1** | **0.378** | **16** | **4** | 0 | 2 | 0.0 | 0 | 0 | — |

\*last-iter. mean SF/OCC TPS = **0.375**. Await@a arms = **0** (replaced by edge π). SoftWait Soft = **0**.

### vs three-pillar (N=5, other host)

| Block | 3P SF/OCC | this SF/OCC | 3P SF wall | this SF wall |
|------:|----------:|------------:|-----------:|-------------:|
| 597 | 0.36 | **0.335** | 18.0 | **14.5** |
| 599 | 0.49 | **0.428** | 27.5 | **24.3** |
| 097 | 0.43 | **0.359** | 16.4 | **16.1** |
| 598 | 0.52 | **0.378** | 2.9 | **2.9** |
| mean | **0.449** | **0.375** | | |

Absolute SF wall on 597 improved vs three-pillar; **OCC also faster here**. Ratio did not approach 1. Tip / pause-rethink 597 was already ~0.35 SF/OCC.

### xblock contiguous (`complete-arch-xblock-xblock.json`)

Warm = one `Pevm` (inter prior / H). Cold = fresh `Pevm` per block.

| Family | Block | SF-warm wall | SF-cold wall | OCC wall | warm≥cold? |
|--------|------:|-------------:|-------------:|---------:|:-----------|
| 597 | 14689597 | 16.9 | 15.1 | 6.1 | **no** (warm slower) |
| 597 | 14689596 | 2.4 | 2.6 | 0.8 | yes (tiny) |
| 599 | 19606599 | 30.7 | 24.6 | 11.6 | **no** |
| 599 | 19606598 | 3.3 | 2.7 | 1.5 | no |
| 097 | 19469097 | 21.9 | 16.1 | 5.9 | **no** |

A6 warm-start **does not beat cold** on the three storm cores. Decay path is wired; it is not yet a makespan win.

597 warm π (per-block metrics; fresh `MetricsInner` each execute): `edge_bind=824`, `edge_wait_for=11`, `edge_spec=4016`, `avoid=917`, `canary=12`, `indep=3018`, `spine_waits=11`, `hot=971`. Bind and Avoid fire. WaitFor is rare vs Spec (hang-freedom filter).

Flip 598→599: SoftWait Soft=0 both; `flip_count` 1→2.

---

## Gaps (honest)

1. **大幅 / wall vs OCC** — still 2.5–3× on 597/599/097. Protocol shape moved; makespan did not.
2. **WaitFor admission leak** — essential unpublished + Ready/higher writer → Spec. Required for hang-freedom; violates SoT “never Spec known essential” until the spine is Executing.
3. **R2 “real opcode skip”** — production ResumePath already reports `aj` (597 last-iter **19**). Concurrent Bind-snap JUMP stays env/ignored (Iter21+ hang history). No new JUMP door in this cut.
4. **R4 still happens** — 599 last-iter `full_restart=13` plus `force_bind_reabort`. Ladder prefers R1/R2; FullRestart is not gone.
5. **AEC / morph still in the tree** — `choose_action`, Storm/Quiet engagement, Await@a helpers. Not the wait π. Morph still seeds engagement at block start (prior only).
6. **Warm ≥ cold fail** on storm blocks — A6 decay is not enough.
7. **`choose_resolve` dead** on the hot path (warning). Fine; do not rewire AEC as π.

---

## Hard-ban checklist

| Ban | This cut |
|-----|----------|
| SoftWait storms | Soft=0 on all smoke rows |
| EV Await / AdaptiveParams-as-θ | Await@a arms=0; π is `choose_edge_action` |
| tip-identity as Bind door | Bind on Data; tip hygiene unchanged |
| Storm/Quiet as protocol | not the wait verb |
| OCC-retry as control plane | discovery / R4 failure only |
| abort↓ while ≪OCC | **called out; not a win** |
| fine grain = knobs | identity is `(ℓ,reader,k,depth)` + verbs |

---

## Next cut (only after human confirm)

Do not add θ. If continuing: admit the spine so WaitFor does not need the Ready→Spec leak (work-conserving ready queue that **runs the writer** without `add_dependency` inversion); measure whether that cuts 597 wall toward OCC. Do not ship another abort-only story.
