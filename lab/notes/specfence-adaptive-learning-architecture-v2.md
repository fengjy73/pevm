# SpecFence Adaptive Learning Architecture v2

**Date:** 2026-09-06 (Asia/Shanghai)  
**Status:** DESIGN — collection hooks only in this task; no full CC rewrite  
**Evidence:** contiguous segments A/B/C fine-grain (`lab/results/contiguous-segments-finegrain.*`, `lab/notes/contiguous-segments-serial-occ-finegrain.md`)  
**Supersedes:** Adaptive CC redesign v1 HotSet thresholds (`H_w`/`H_a`) as *primary* control; sticky block-wide conflict / Wait bit  
**Keeps:** sequential ≡ parallel TCB; opt-in finegrain/inspect research flags off by default; LeanOCC default execute path (`Handler::run`); location as concurrency object

---

## 0. Critique of Adaptive CC v1 (why thresholds ≠ adaptive)

v1 replaced plant-v2’s always-on inspect with **LeanOCC + HotLocal**, gated by HotSet:

| Control | v1 rule | Failure mode |
|---------|---------|--------------|
| HotSet insert | writers ≥ **H_w** (8) or aborts ≥ **H_a** (3) | Hand-tuned constants; same knob for storage storms and basic/handler chatter |
| Engagement | default Lean, escalate on HotSet | Still a **dead threshold ladder** — not EV of wait vs remaining work |
| Regime | Hot if `|HotSet|≥1` + abort rising | Sticky / block-coarse; one hot ℓ can flip policy without per-edge economics |
| WaitHard | only for HotSet | Better than block-wide Wait, but still **location membership**, not per-edge decision |
| R3 result | SF/OCC@8 ≈ 0.325, gates miss | Bind/meta vs OCC; thresholds did not learn morphology across contiguous blocks |

**Thesis:** “Adaptive” must mean **online Bayesian / EV decisions on discovered RAW edges**, using predicted remaining interpreter work vs wait/steal cost — not retuning `H_w`/`H_a` until a gate barely passes.

Contiguous segments A/B/C exist precisely so we can measure **program vs handler dependency morphology** that persists (or flips) across adjacent blocks — something a per-block HotSet bit cannot carry.

---

## 1. Concurrency object (unchanged)

Region access \(a=(t,k,\ell,m)\) on `MemoryLocation`. Scheduling grain remains **tx task** until hang-free `(t,k)` continuation is proven. Learning never commits; wrong Wait/Bind → extra SpecRead/abort, still seq≡par.

---

## 2. Intra-block online learning (edge-local)

### 2.1 Discover RAW edges as effects appear

As writers publish Data / readers touch ℓ:

1. Observe candidate edge \(e = (p \to c, \ell, \mathrm{kind})\).  
2. Classify **program** (`storage` / `code_hash` / `selfdestruct`) vs **handler** (`basic` / account-basic paths). Lazy sender/recipient stay off the speculative critical path (exclude from edge decisions mid-block).  
3. Maintain online features (see §4).

### 2.2 Per-edge action set (no fixed H_w)

For each unresolved edge at consumer read time:

| Action | When EV wins |
|--------|----------------|
| **Bind** | Producer Data already published (and optional prior agrees) |
| **Wait** (+ park/steal) | `E[remaining_work_consumer]` ≫ `E[wait_until_producer]` + steal opportunity; program edges with short producer lag preferred |
| **SpecRead** | Default when wait EV loses or handler edge with weak prior |
| **Abort / ESTIMATE** | Validation failure or deliberate early abort when first stale read is **early** in consumer (high redo savings) |

**Reject:** sticky block-wide `P(conflict)` Wait bit; HotSet membership as the *only* gate for Wait.

HotSet may remain a **feature / cache hint** (which ℓ to track densely), not the control law.

### 2.3 Early discovery / redo savings

If the first cross-tx stale read occurs early in consumer gas (inspect research flag only for measurement), abort+reexec can beat waiting. Proxies without inspect (this dataset): **producer_lag**, **cross_tx_read_frac** on final RW, program-chain length. Learner should eventually use **depth-into-tx of first stale read** when a hang-free probe exists.

---

## 3. Inter-block / contiguous prior

Carry a compact **morphology prior** across adjacent blocks (segments A/B/C are the training substrate):

| Prior | Content |
|-------|---------|
| Program-chain mass | Expected longest program-RAW path, multi-writer storage density |
| Handler/account-basic mass | Basic RAW/WAW intensity (often schedule/CEX noise) |
| Schedule cost proxy | Abort rate vs program-RAW under Cancun-like blocks (segment B) |
| Edge-type mix | program_frac / handler_frac, mean lag |

**Use:** warm-start per-edge priors at block begin (not pre-seed WaitHard); decay if morphology flips (e.g. 19606598 → 19606599 contrast). Never freeze a block-wide conflict bit from prior alone.

---

## 4. Learner features (from RAW traces)

| Feature | Source | Role |
|---------|--------|------|
| `edge_class` | MemoryValue kind → program/handler | Different Wait/Spec priors |
| `kind` | storage / basic / code_hash | Fine type |
| `producer_lag` | \(c - p\) | Wait horizon proxy |
| `chain_length` / depth on program DAG | program-RAW longest path through edge | Critical-path weight |
| `depth_first_stale` | inspect research / future probe | Early-abort EV |
| `abort_count@ℓ`, `writers_so_far@ℓ` | online | Soft features — **not** hard H_w/H_a gates |
| Contiguous prior match | previous block morphology | Inter-block transfer |

Output: soft scores → action EV; TCB remains OCC-correct.

---

## 5. Runtime flow redesign sketch

```
begin_block:
  load contiguous morphology prior (program/handler mix, chain mass)
  HotSet := empty   # optional tracking set only
  default_path := LeanOCC (Handler::run; no inspect)

execute(tx_c):
  for each read ℓ:
    observe online writers < c on ℓ → candidate RAW edge e
    feats := features(e, prior, writers_so_far, abort@ℓ)
    action := argmax_EV { Bind, Wait+park, SpecRead }  # NOT threshold H_w
    apply action; never block-wide Wait bit
  on write ℓ: publish Data; update online writer counts / chain proxies
  on abort: ESTIMATE; record feats ↔ outcome for learner; selective invalidate

worker:
  Wait ⇒ park + steal (never spin)
  inspect/jump only behind research flag

end_block:
  emit RAW/DAG summary; update contiguous prior for block+1
```

**Hard rules:** production `finegrain_trace` / inspect **off**; seq≡par; analysis tooling opt-in only.

---

## 6. What to implement next (phased) — this task stops at collection

| Phase | Work | Out of scope here |
|-------|------|-------------------|
| **P0** (done/this PR) | Contiguous fetch; finegrain RAW edges program vs handler; segment notes; this architecture doc | Full CC rewrite |
| **P1** | Persist per-block RAW morphology prior JSON; unit tests on classify_raw_edges | — |
| **P2** | Online edge feature struct in SpecFence path (behind flag); log EV inputs without changing decisions | — |
| **P3** | Replace HotSet Wait gate with per-edge EV Wait/Bind/SpecRead on program edges only; measure SF/OCC on A/B/C cores | No default inspect |
| **P4** | Contiguous prior warm-start; A/B/C holdout evaluation; kill remaining H_w/H_a as primary control | — |

---

## 7. Success criteria (future)

1. Wide morphology blocks: SF/OCC@8 ≥ 0.95, wait≈0, inspect=0.  
2. Hot program-chain blocks: `evm_entries` ≤ OCC or clear Bind/Wait win on program edges; no livelock@8.  
3. Contiguous prior improves first-N-tx decisions vs cold start on segments A/B/C.  
4. No reliance on sticky block-wide conflict bit; H_w/H_a demoted to optional feature bins.

---

## 8. Evidence pointer

See `lab/notes/contiguous-segments-serial-occ-finegrain.md` for segment A (14689597 deps / early discovery), B (19606599 Cancun program vs handler vs schedule; 19606598 contrast), C (19469097 long chains vs redo; 19469096 long path / fewer txs).


---

## 9. Evidence appendix (contiguous fine-grain, 2026-09-06)

| Core | Signature | Control implication |
|------|-----------|---------------------|
| **14689597** | 449 storage RAW (100% program), fan-out from writers 0…38 → 474 readers; first prog consumer @ tx39 lag1 inc7; reexec≈0.65 | Edge-local Bind/Wait at first program publish; H_w=8 too late |
| **19606599** | 22 prog (lag≈140) + 20 hand (lag≈13); basic 54w/74r | Split class priors; handler lean SpecRead |
| **19606598** | 0 program RAW, indep 0.80, abort 0.11 | Quiet neighbor — sticky Hot bit from 597/599 would tax; both redo&wait cheap |
| **19469097** | prog_chain=4, 16 prog RAW, abort 0.44 | Program-path Bind leverage |
| **19469096** | longest=132, RAW=6 (WAW spine), abort 0.48 | Longest-chain ≠ Wait EV; don’t HotSet on WAW-only |

Full tables: `lab/notes/contiguous-segments-serial-occ-finegrain.md`.
