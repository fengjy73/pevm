# PC utilization × CC fine-grain × learn in place — complete-land design

**Baseline:** PR #40 `cursor/specfence-midband-spine-tps-041c` @ `a756267`  
**Evidence:** `lab/notes/specfence-pr40-tps-losers-optimal-vs-overhead.md`  
**User:** learning not fully in place; CC not fine-grained enough; PC not fully utilized — complete design then full-package land  
**Terms:** OrderedAdmit wait-set / ungated OCC task selection / cover_window / under-covered conflict spine / lazy-update chain / over-admission OrderedAdmit / Detect+Resolve double charge  
**Soft=0; `select_arm` is the only mouth; no staging**

---

## 0. Three-side gap (design target)

| Side | Gap at PR #40 | This package |
|------|---------------|--------------|
| **Learn** | `cover_proven_cheaper` never lights when cover was never tried → sticky Opt deadlock; proxy still unfenced-heavy | Probeable, provable cover; reward aligns wall/TPS; lazy never ordered |
| **CC** | wait-set soft-cap=8 cannot cover L=33–56; Full can still land on lazy thousand-writers; cover vs Opt is bipolar | Cover by spine length/morph; wrong object cleared; real spine may be segmented |
| **PC** | NEAR ungated already open, execute/validate shell still 2–3× OCC; FAR gates shrink ungated | OCC-equivalent ungated hot path; order constrains edges only; width filled |

---

## 1. Learn (L)

| ID | Content |
|----|---------|
| **L1 break cover deadlock** | Cold/crisis may **probe cover** (short cover_window). Update ĉ from wall-clock consequences. Sticky only on wall success; else OptimisticRead. Ban “never tried ⇒ never proven”. |
| **L2 reward** | Primary signal = block/ℓ wall vs OCC abort counterfactual. Ban unfenced-only window growth. |
| **L3 morph** | lazy-update / near-independent: candidates are OptimisticRead (+Defer) only. Do not inherit real-spine Win. |
| **L4** | Hot sticky on a proven policy; cold probe is budgeted; Instant idle ↛ ĉ. |

## 2. Concurrency-control fine-grain (C)

| ID | Content |
|----|---------|
| **C1 segmented/sliding cover** | For L∈[20,64] real Basic/storage: absorb the critical path with a segmented or sliding cover_window. **Not** default whole-spine Full, **not** default forever Opt. |
| **C2 ultra-long spine** | L≫64 (e.g. 571): ĉ compares ordered prepaid vs whole-spine OptimisticRead; default yields to OCC abort. Ban empty Win_1/Seg uphill. |
| **C3 wrong object** | Full/Win **must not** land on lazy-update thousand-writers. 15274915-class gates only the real Basic spine. |
| **C4 wait-set** | Soft-cap is decoupled from cover depth: when deeper cover is needed, grow by segments — not “cap=8 still loses → total withdraw”. |

## 3. Parallel-compute utilization (P)

| ID | Content |
|----|---------|
| **P1 OCC-equivalent ungated** | Wait-set empty or short-chain: execute+validate is byte-level OCC (expand skip path tax). Target: NEAR wall → OCC. |
| **P2 edges, not a mode switch** | While an OrderedAdmit wait-set exists, ungated txs still use ungated OCC task selection. Measure ungated_occ ≈ n − wait_set. |
| **P3** | Near-independent large blocks must not fill non-critical wait-set slots (drop 4–8 holes if they are not critical edges). |

## 4. Acceptance

1. **FAR mid-band reps** (19716145, 19860366, 8889776, 16146267): probeable cover with better wall/TPS than PR40 sticky Opt, **or** Opt wall ≈ OCC.
2. **NEAR reps** (14396881, 13217637): clear SF/OCC ratio drop (shell ↓).
3. **15274915:** no Full on lazy thousand-writers.
4. **99-block Soft=0 TPS:** SF TPS≥OCC **>23/98** and median > PR40 0.832; no 4–27× lazy fat-tail return.
5. Soft=0; iter11; erc20; professional terms.

## 5. One PR
