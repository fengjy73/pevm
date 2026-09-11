# Spec=Region / Fence=barrier — implementation report

**Date:** 2026-09-11  
**PR:** https://github.com/fengjy73/pevm/pull/3  
**Branch:** `cursor/specfence-complete-cc-63b0`  
**SoT:** `lab/notes/specfence-spec-means-region.md`  
**Diagnosis:** `lab/notes/specfence-mainnet-exec-process-diagnosis.md`  
**Smoke tag:** `SPECFENCE_G7_TAG=region-fence-align-xblock` N=3 @8 + xblock  
**JSON:** `lab/results/region-fence-align-xblock-{sf-occ,flip,xblock}.json`

---

## Verdict

**Rename is complete on the Avoid path.** Spec = Region. Optimistic access = **Unfenced**. Product name SpecFence stays.

**Fence leaks 1–2 are closed in π:** `force_prefix ∧ writer=None` and `avoid ∧ writer=None` produce `WaitFor(reader-1)` + `admit_spine`, not Unfenced. Bind-on-published-Data kept. SoftWait Soft = **0**. Await@a = **0**.

**Wall did not move toward OCC.** Mean SF/OCC TPS = **0.353** (gaps-closed was 0.426 on this host family). 597 SF wall median **17.5 ms** vs OCC **6.6 ms** (~2.7×). 597 warm WaitFor **49** (was 21) / Unfenced **3726** (was Spec 4117) / Bind **765**. Clique Unfenced did not collapse — remaining Unfenced are independence / canary / inversion. Do not celebrate abort↓ (597 SF abort med **42** vs OCC **82**).

---

## Rename map (Avoid path)

| Old (Spec = speculate) | New (Spec = Region) |
|------------------------|---------------------|
| `EdgeAction::SpecRead` | `EdgeAction::Unfenced` |
| `edge_spec` / `record_edge_spec` | `edge_unfenced` / `record_edge_unfenced` |
| `independent_specs` | `independent_unfenced` |
| `note_spec` / `live_specs` | `note_unfenced` / `live_unfenced` |
| `CLIQUE_SPEC_CAP` | `CLIQUE_UNFENCED_CAP` |
| “Spec freely / mass Spec / never Spec” | Unfenced / mass-Unfenced / never Unfenced on essential |

**Unchanged (not Avoid π):** product `SpecFence`; AEC `ResolveAction::SpecRead` (compiled, unused as wait π); hollow `RegionMode::Speculate\|Wait` (PCC/legacy; documented not protocol face). Region SoT is `EdgeKey` (ℓ + k + depth + typed edge).

**rg:** Avoid-path (`edge.rs`, `sketch.rs`, `maybe_wait_specfence`, harness `edge_*`) has no Spec-as-speculate.

---

## Semantic π

```
Bind if published Data                         # Fence (A3)
WaitFor(w) if essential ∧ w < reader           # Fence (D6)
WaitFor(reader-1)+admit_spine
  if (force_prefix | Avoid | essential) ∧ writer=None ∧ reader>0
                                               # serial-lane Fence
Unfenced                                       # independence / canary / cold / inversion
```

Hang-freedom = `admit_spine` + steal. Residual writer is resolved for all `writer=None` (not force-only). `essential_antidep` is true for `force_prefix` and Avoid even without a writer.

---

## Tests

| Suite | Result |
|-------|--------|
| `cargo test -p pevm --lib` | **119 passed** |
| `cargo test -p pevm --test specfence -- --test-threads=1` | **39 passed**, 20 ignored |
| mocked evm (`raw_transfers`, `mixed`, `small_blocks`, `beneficiary`) | **13 passed** |
| new: `force_prefix_none_writer_serial_lane_fence`, `avoid_none_writer_serial_lane_fence` | pass |
| `complete_arch_edge_pi_seq_eq_par_softwait0`, `gaps_closed_waitfor_avoid_publish_wake` | pass |

---

## Metrics (N=3 @8)

| Block | SF wall med | OCC wall med | SF/OCC TPS | SF abort med | OCC abort med | Soft |
|------:|------------:|-------------:|-----------:|-------------:|--------------:|-----:|
| **14689597** | **17.5** | **6.6** | **0.323** | **42** | **82** | 0 |
| **19606599** | **31.5** | **11.0** | **0.349** | **134** | **78** | 0 |
| **19469097** | **17.2** | **5.5** | **0.316** | **137** | **138** | 0 |
| **19606598** | **3.2** | **1.2** | **0.426** | **11** | **7** | 0 |

Mean SF/OCC = **0.353**. SoftWait Soft = 0. Await@a = 0.

### 597 π (xblock warm)

| | gaps-closed | this (Unfenced name) |
|--|------------:|---------------------:|
| edge_bind | 841 | **765** |
| edge_wait_for | 21 | **49** |
| edge_spec / edge_unfenced | 4117 | **3726** |
| avoid | 916 | **911** |
| SF-warm wall | 42.6 | **17.9** |
| SF-cold wall | 18.7 | **15.4** |
| OCC wall | 5.0 | **5.8** |

WaitFor rose (leaks 1–2 closed). Unfenced still dominates — independence/canary, not writer=None fallthrough. Warm 597 **17.9** vs cold **15.4** (gaps-closed warm was 42.6). Still much slower than OCC.

Flip 598→599: Soft=0 both; `flip_count` 1→2.

---

## Hard bans

| Ban | This cut |
|-----|----------|
| SoftWait storms | Soft=0 |
| EV Await / AdaptiveParams-as-θ | Await@a=0; π is `choose_edge_action` |
| tip-identity as Bind door | Bind on Data |
| Storm/Quiet as protocol | not the wait verb |
| OCC-retry as control plane | discovery / R4 failure only |
| abort↓ while ≪OCC | **called out; not a win** |

---

## Residual

1. **大幅 / wall vs OCC** — still ~2.7–3× on storm cores. Terminology + Fence fallthrough closed; makespan did not.
2. **AEC `SpecRead` still compiled** — unused as wait π. Not Avoid-path.
3. **R2 abs JUMP** — production-OFF. **R4** still happens.
4. Hollow `RegionMode` remains for PCC/legacy; Avoid does not read it.
