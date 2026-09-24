# Mainnet serial + OCC fine-grain analysis (baseline plant)

**Date:** 2026-09-06 (Asia/Shanghai)  
**Checkout:** `/workspace/specfence` tip `96c0d7a` (specfence) + fine-grain tracer  
**Blocks:** 19807137, 14683600, 13217637, 14383540, 15199017, 14029313, 19434587  
**Tooling:** `cargo run -p pevm --release --config 'profile.release.lto=false' --example specfence_finegrain_analysis`  
**Artifacts:** `lab/results/mainnet-serial-occ-finegrain.{json,csv}` + deep `…-b{19807137,15199017,19434587}.json`

Do **not** treat this as a SpecFence CC redesign implementation — analysis only. Production paths unchanged when `Pevm::set_finegrain_trace(false)` (default).

---

## Method

1. **Serial:** `force_sequential=true` wall time / TPS (interpreter baseline).
2. **OCC:** Block-STM @1 and @8 with opt-in `FineGrainCollector`:
   - final per-tx RW hashes (last incarnation ≈ true \(G^\star\) under serializability)
   - location kinds from MV `MemoryValue` (`basic` / `basic_lazy` / `storage` / `code_hash`)
   - per-tx final incarnation + abort events (ESTIMATE cascade width)
3. **DAG proxies** from sequential RW:
   - **Effective \(G^\star\):** exclude coinbase + `basic_lazy` (LazySender/LazyRecipient). These are deferred to block-end evaluation and are **not** on the speculative critical path (same reason pevm introduced lazy transfers).
   - **Raw/lazy \(G^\star\):** include lazy WW chains (often looks “totally ordered” but OCC does not serialize on them mid-block).
4. **v7 loss context:** cite `lab/notes/mainnet-sweep-v7-status.md` / `lab/results/mainnet-sweep-v7.json` (inspect tax / WaitHard / zero jumps / lean=0).

Gas / abort / `evm_entries` used as cost proxies (no per-phase steal timers beyond existing metrics).

---

## Headline findings

### 1. Two plant regimes (must not use one CC policy)

| Regime | Blocks (examples) | Effective DAG | OCC@8 abort | What wins |
|--------|-------------------|---------------|-------------|-----------|
| **Wide / lean** | 13217637, 15199017, 14383540, 14029313 | longest chain **6–21**, indep **0.84–0.95**, max wave **643–1060** | abort_rate **0.6–5%**, reexec_frac **0.04–0.44** | Lean OCC. Any default inspect/WaitHard is pure tax. |
| **Hot chain / storm** | **19807137**, **19434587**, (14683600 mid) | longest **62–571**, indep **0.13–0.48** | abort_rate **0.42–1.14**, reexec_frac **0.72–0.83**, max_inc **12–13** | Still OCC today (workers stay busy). SpecFence only wins if it **cuts interpreter-seconds on the hot chain** without inspect/livelock. |

### 2. Top bottleneck is **one (or few) multi-writer storage/basic locations**, not “tx-level” opacity

- **19807137:** a single **storage** location with **571 writers and 571 readers** (txs 140…710). Effective longest chain = **571**, conflict component = **571**, WAW edges dominate (628 WAW vs 9 RAW). Incarnation histogram: only 111/712 txs succeed first try; mass at inc 4–8; max_inc=13. OCC@8 still ~48k TPS because ESTIMATE+steal keeps cores fed — waste is **~5.9× interpreter entries** (`evm_entries=4238` vs `n_tx=712`, reexec_frac≈0.83).
- **19434587:** storage chain writers=62 + hot basic (55 writers / 86 readers); abort_rate 0.42; reexec_frac 0.72; max_inc=12.
- **Wide blocks:** after dropping `basic_lazy`, effective conflicts collapse (e.g. 13217637 lazy longest **934** → effective **6**). Lazy CEX/airdrop WW is **not** the speculative bottleneck — matching pevm’s lazy design.

### 3. OCC abort structure

| block | occ8 aborts | abort_rate | txs_with_abort | max_inc | evm_entries | reexec_frac | cascade proxy |
|-------|-------------|------------|----------------|---------|-------------|-------------|---------------|
| 19807137 | 814 | 1.14 | 601 | 13 | 4238 | 0.83 | classic full suffix on each abort |
| 14683600 | 70 | 0.11 | 119 | 6 | 996 | 0.34 | moderate |
| 13217637 | 7 | 0.006 | 17 | 2 | 1167 | 0.06 | negligible |
| 14383540 | 35 | 0.05 | 43 | 5 | 941 | 0.23 | mild |
| 15199017 | 11 | 0.013 | 25 | 3 | 901 | 0.04 | negligible |
| 14029313 | 28 | 0.039 | 54 | 4 | 1292 | 0.44 | mild–mid |
| 19434587 | 164 | 0.42 | 165 | 12 | 1385 | 0.72 | heavy |

OCC@1: **0 aborts** on all seven (in-order single worker) — useful calibration that final RW = \(G^\star\) without storm noise.

**Abort→reexec wall contribution (proxy):** on hot blocks, majority of `evm_entries` are reincarnations. Useful work share ≈ `n_tx / evm_entries` (19807137 ≈ 17%; 15199017 ≈ 96%). SpecFence v7 did **not** reduce this ratio; it added inspect steps on top.

### 4. Why SpecFence v7 lost (cite v7 status)

From `mainnet-sweep-v7-status.md` (SF@8 vs OCC@8):

| block | SF/OCC v7 | wait_hard | inspector_steps | lean_mode_txs | absolute_jump_applied |
|-------|-----------|-----------|-----------------|---------------|------------------------|
| 19807137 | **0.085** | 7292 | 1.47M | **0** | **0** |
| 14683600 | 0.135 | 1034 | 1.27M | 0 | 0 |
| 13217637 | 0.101 | 113 | 0.41M | 0 | 0 |
| 14383540 | 0.123 | 800 | 0.81M | 0 | 0 |
| 15199017 | 0.067 | 270 | 0.51M | 0 | 0 |
| 14029313 | 0.045 | 167 | 0.44M | 0 | 0 |
| 19434587 | 0.044* | 2185 | 1.69M | 0 | 1 |

\*SF@8 hung (inspect livelock); SF@4 used as proxy.

**Attribution (aligned with this fine-grain pass):**

1. **`lean_mode_txs = 0` on every block** — even wide DAG blocks (indep≥0.94) paid full plant.
2. **`inspector_steps` 0.4M–1.7M** — dominant new cost vs OCC; jumps did not amortize (`absolute_jump_applied≈0`).
3. **WaitHard storms on hot blocks** (7292 / 2185) without reducing `evm_entries` below OCC — wait idle + meta + same head reexecs.
4. Hang surface at width 8 — cannot claim TPS if SF@8 does not complete.

Mean SF/OCC@8 fell from ~0.30 (v5) to ~0.09 (v7). Plant v2 moved the **wrong** way relative to the iron law: it increased \(T_{\mathrm{waste}}\) (inspect) without cutting critical-path interpreter-seconds.

### 5. Cost breakdown (proxies)

| Component | Wide blocks | Hot blocks |
|-----------|-------------|------------|
| Execute (useful) | ≈ `n_tx` EVM entries | minority of entries |
| Reexec / ESTIMATE cascade | small (`reexec_frac`≪0.2) | dominant (`reexec_frac`>0.7) |
| Validate / scheduler steal | cheap vs EVM (OCC stays ahead) | still cheap vs EVM; SpecFence Wait/inspect not |
| SpecFence inspect (v7) | always-on tax | tax **plus** WaitHard |

---

## Summary tables

### Serial vs OCC TPS

| block | n_tx | serial TPS | OCC@1 TPS | OCC@8 TPS | OCC8/serial |
|-------|------|------------|-----------|-----------|-------------|
| 19807137 | 712 | ~250† | 27769 | 48322 | ~193×† |
| 14683600 | 660 | 48967 | 33356 | 98920 | 2.0× |
| 13217637 | 1100 | 153232 | 98657 | 218334 | 1.4× |
| 14383540 | 722 | 65927 | 43968 | 104476 | 1.6× |
| 15199017 | 866 | 107872 | 57672 | 188692 | 1.7× |
| 14029313 | 724 | 109966 | 55134 | 183212 | 1.7× |
| 19434587 | 390 | 19369 | 14971 | 35129 | 1.8× |

†First-block serial includes cold bytecode/cache effects; use OCC@1 as serial-ordered EVM proxy when comparing absolute serial ms on 19807137.

### Effective DAG shape (beneficiary + basic_lazy excluded)

| block | longest | indep_frac | max_wave | multi_writer_locs | max_writers/loc | max_conflict_comp | lazy_longest |
|-------|---------|------------|----------|-------------------|-----------------|-------------------|--------------|
| 19807137 | **571** | 0.13 | 106 | 30 | **571** | **571** | 571 |
| 14683600 | 83 | 0.64 | 449 | 87 | 83 | 85 | 83 |
| 13217637 | **6** | **0.95** | **1060** | 28 | 5 | 22 | 934 |
| 14383540 | 21 | 0.88 | 655 | 44 | 19 | 32 | 436 |
| 15199017 | **7** | **0.94** | **830** | 26 | 7 | 13 | 459 |
| 14029313 | 14 | 0.84 | 643 | 40 | 14 | 14 | 146 |
| 19434587 | **62** | 0.48 | 207 | 69 | 62 | 91 | 62 |

### Location kind mix (written locations observed in MV)

Hot/conflicted blocks are **storage-heavy** (19807137: 1006 storage / 745 basic / 12 lazy). Wide transfer blocks are **basic_lazy-heavy** in the raw map but those edges vanish in effective \(G^\star\).

---

## Deep dives

### 19807137 — pathological hot storage chain

- One storage location serializes **~80% of the block** (writers 140…710).
- OCC abort storm: 814 aborts, 601 txs reincarnated, max_inc=13, `evm_entries/n_tx ≈ 6.0`.
- SpecFence v7: SF/OCC=0.085, WaitHard=7292, inspector_steps=1.47M, lean=0, jumps=0.
- **Implication:** only location-local Wait/Bind on **that** slot (and its RAW readers) can help; whole-tx inspect and block-wide WaitHard cannot beat ESTIMATE on a length-571 chain unless reincarnation count drops sharply.

### 15199017 — wide DAG, OCC almost free

- Effective longest=7, indep=0.94, abort_rate=1.3%, reexec_frac=3.9%.
- Raw lazy longest=459 (CEX/airdrop illusion).
- SpecFence v7 still SF/OCC=0.067 with 0.51M inspector steps and lean=0.
- **Implication:** default path must be **lean OCC**; full plant engagement is a regression.

### 19434587 — mid/hot + hang risk

- Effective longest=62; storage+basic hotspots; abort_rate=0.42; reexec_frac=0.72.
- v7: SF@8 **livelock**; SF/OCC proxy 0.044; inspector_steps=1.69M; jumps≈1.
- **Implication:** hang-free lean path first; any WaitHard must be confined to measured hot locations; inspect cannot be default-on.

---

## Redesign recommendations

### Architecture (concurrency object, engagement, resolution)

1. **Concurrency object = location** (already correct). Promote **storage slots** and contended **basic** accounts; never treat lazy transfer addresses as speculative Wait targets.
2. **Engagement (must-have):** binary (or graded) mode:
   - **Lean OCC** when prior/intra-block signals say low abort (indep high, no mega multi-writer) — **zero inspector**, zero Bayes WaitHard.
   - **Full plant** only when a hot location is confirmed (multi-writer ≥ K or abort_rate mid-block ≥ τ).
3. **Resolution:** on hot locations prefer **Wait-for-writer / Bind** over whole-tx abort; on cold locations keep ESTIMATE. Do **not** cascade-validate independents (fence already helps; keep it).
4. **What NOT to do (EVM plant limits):**
   - Default-on Inspector / absolute PC jump chasing mainnet TPS (v7 proof).
   - Global WaitHard from account hints alone (creates idle without cutting `evm_entries`).
   - Claiming PartialRetry/RewindTo wins without measuring `evm_entries` and hang rate.
   - Treating raw lazy WW chains as critical-path evidence.

### Learner (features / priors from these traces)

Train / seed from fine-grain traces:

| Feature | Why |
|---------|-----|
| Multi-writer count / chain length per location (esp. storage) | Separates 19807137-class from 15199017-class |
| Effective indep_frac / max_wave / longest_chain (lazy excluded) | Engagement prior |
| Per-block abort_rate + max_incarnation + reexec_frac | Online escalate lean→full |
| Kind mix (storage vs basic_lazy fraction) | Ignore lazy heat for Wait |
| Hot location top-K stability across recent blocks | Inter-block Bind/Wait seed |
| `evm_entries / n_tx` | Objective for repair quality |

Priors should output **per-location** Wait/Bind probability, not a block-global “use SpecFence” bit alone. Block-level engagement = OR/threshold over hot locations.

### Runtime flow (execute / validate / repair without default inspect tax)

```
admit tx:
  if engagement=lean: OCC execute (no inspect, no WaitHard)
  else: WaitHard/Bind only on locations with prior≥τ; else SpecRead

validate:
  exact RW origins (unchanged TCB)
  on fail:
    if only hot-loc mismatch and writer known → Wait/Bind repair without inspect
    else ESTIMATE + head reexec (OCC)
  never enable Inspector unless a measured repair path needs PC resume AND hang-free gate is green

steal:
  keep OCC collaborative scheduler; park only on confirmed WaitHard writers (M2 idea), never spin
```

**Success metrics for next redesign (gates):**

1. Hang rate @8 = 0 on all 7 blocks.  
2. `lean_mode_txs / n_tx` high on wide blocks (13217637/15199017 class).  
3. On hot blocks: `evm_entries` ≤ OCC `evm_entries` (or clearly fewer reincarnations on the hot chain).  
4. SF/OCC@8 ≥ 1.0 on ≥1 hot block **and** ≥ 0.95 on wide blocks (no inspect regression).  
5. `inspector_steps = 0` on lean path; jumps only behind hang-free + net `evm_entries` win.

---

## Tooling landed

- `crates/pevm/src/specfence/finegrain.rs` — opt-in collector + DAG/hot helpers  
- `Pevm::set_finegrain_trace` / `take_finegrain_snapshot`  
- Example: `crates/pevm/examples/specfence_finegrain_analysis.rs`  
- Results: `lab/results/mainnet-serial-occ-finegrain.*`  

Sequential equivalence of production paths: tracer off by default; OCC/SpecFence logic unchanged aside from optional abort event recording when enabled.
