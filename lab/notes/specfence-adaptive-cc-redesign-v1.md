# SpecFence Adaptive CC Redesign v1 — from serial+OCC fine-grain evidence

**Status:** FROZEN for implementation 2026-09-06  
**Evidence:** `lab/notes/mainnet-serial-occ-finegrain-analysis.md`, sweep v7  
**Supersedes for engagement/runtime:** plant-v2 default-on inspect, block-wide WaitHard, M4 thresholds that never fired (`lean_mode_txs=0`)  
**Keeps:** sequential ≡ parallel TCB; M2 park/steal idea at tx grain; location as concurrency object; prior Bind when Data ready (M3) without escalate-Wait; optional jump only behind hang-free net-win gate (off by default on mainnet until proven)

---

## 0. Problem statement (from data)

Two regimes on the same seven blocks:

| Regime | Signature | What OCC does | What SpecFence v7 did wrong |
|--------|-----------|---------------|------------------------------|
| **Wide** | indep ≥0.84, longest ≤21 | Almost no aborts; lean Block-STM wins | Paid full inspect + Bayes + Wait paths (`lean=0`) |
| **Hot** | one storage/basic with tens–hundreds of writers | High reexec but cores stay busy | WaitHard storms + inspect + same/worse `evm_entries` + hang |

Iron law unchanged: win only by cutting **interpreter-seconds on the hot chain** and/or **meta/idle**, without destroying wide-block OCC speed.

---

## 1. Architecture

### 1.1 Concurrency object
Still **region access** \(a=(t,k,\ell,m)\) on `MemoryLocation`. Scheduling grain remains **tx task** (pevm Block-STM) until true `(t,k)` continuation is hang-free on mainnet.

### 1.2 Dual-mode plant (engagement)

```
Block start → classify RegimeHint
  Wide  → LeanOCC (default)
  Hot   → HotLocal (full SpecFence only on HotSet locations)
  Unknown → LeanOCC, escalate on evidence
```

| Mode | Execute path | Read resolution | Repair |
|------|--------------|-----------------|--------|
| **LeanOCC** | `Handler::run` (no Inspector) | OCC OrderedDirtyRead / ESTIMATE only | Whole-tx abort + ESTIMATE + fence cascade (existing OCC) |
| **HotLocal** | LeanOCC for cold locs; SpecFence Bind/WaitHard/**park** only when `ℓ ∈ HotSet` | Cold: SpecRead; Hot: Bind if Data ready else WaitHard+park (M2) or SpecRead if wait EV loses | Selective invalidate on hot writers; RewindTo/jump **off by default** |

**Hard rule:** never run default-on `inspect_run` for SpecFence mainnet path. Inspector/jump only behind `SPECFENCE_ENABLE_INSPECT=1` research flag until hang@8=0 and net `evm_entries` win on ≥1 hot block.

### 1.3 HotSet (location-local, not block-wide)

A location enters HotSet when any holds (online, revocable):

1. Observed **≥ H_w writers** in-block so far (default **H_w=8**, R3; LazyRecipient excluded from H_w), or  
2. Abort/ESTIMATE involving `ℓ` ≥ **H_a** times (default **H_a=3**, R3), or  
3. Process prior: historical multi-writer mass for `ℓ` (from fine-grain / prior map) above threshold.

Block RegimeHint = Hot if `|HotSet|≥1` **and** (abort_rate rising **or** prior says storm block); else Wide.

WaitHard / Bayes Wait **forbidden** for `ℓ ∉ HotSet`. This kills block-wide WaitHard storms on wide DAG noise.

### 1.4 Resolution algebra (HotLocal only, hot ℓ)

Priority: **Bind** (published Data + optional prior) → **WaitHard+park** (writer unfinished AND steal-ready work exists OR posterior high) → **SpecRead** → abort/selective invalidate → FullRestart last.

No EarlyVal/inspect-driven cps on the default path.

---

## 2. Learner design

### 2.1 What to learn (from traces)

| Signal | Source | Use |
|--------|--------|-----|
| Multi-writer count / loc | serial G* / online writes | HotSet membership |
| Abort count / loc | OCC ESTIMATE events | escalate HotSet |
| Regime label per block | indep_frac, longest chain proxies | start Lean vs HotLocal |
| Co-access / WŜ | M3 RwPriorMap (keep) | Bind-before-touch when Data ready |
| **Not** sticky block-wide P(conflict) Wait bit | v3 failure mode | — |

### 2.2 What not to learn into TCB
Learning never commits; wrong HotSet → extra SpecRead/abort, still seq≡par.

### 2.3 Engagement learner (replace broken M4 thresholds)

M4 used `last_abort_rate<0.05` & `conflict_mass<0.12` but still `lean_mode_txs=0` — engagement must be **default Lean**, escalate to HotLocal only on HotSet evidence (not “fail to prove quiet”).

```
default = LeanOCC
on first HotSet insert → HotLocal for those locs (rest stay lean reads)
on abort_rate ≥ 0.08 over window → ensure writers’ locs in HotSet
never require “prove quiet” to lean
```

---

## 3. Runtime flow

```
execute(tx):
  if research_inspect: ... else Handler::run
  on each read ℓ:
    if ℓ ∉ HotSet: OCC-style read (no Bayes Wait)
    else: resolve HotLocal (Bind / Wait+park / SpecRead)
  on write ℓ: publish; maybe insert HotSet if writer count↑
  on abort: ESTIMATE; record abort@ℓ; HotSet←ℓ; fence cascade

worker:
  on WaitHard: park + steal (M2) — never spin
  never inspect-step account on default path
```

Block end: update process priors (multi-writer mass, regime); metrics.

---

## 4. Metrics & success gates (implementation)

Required metrics: `lean_mode_txs`, `hot_local_reads`, `hotset_size`, `wait_hard_count` (should be ≈0 on wide blocks), `evm_entries`, hang flag.

Gates before claiming TPS:
1. SpecFence@8 completes all 7 blocks (no inspect livelock).  
2. Wide blocks: `lean_mode_txs / n_tx ≥ 0.95`, `wait_hard≈0`, SF/OCC@8 ≥ **0.95**.  
3. Hot blocks: `evm_entries_sf ≤ 1.05 × evm_entries_occ` or clear Bind/Wait win; SF/OCC ≥ **1.0** on ≥1 hot block **or** documented gap only on remaining head-reexec.  
4. `inspector_steps=0` unless research flag on.

---

## 5. Implementation plan (this freeze)

### Phase R0 — Strip default tax
- SpecFence production execute = `Handler::run` (no inspect) unless `SPECFENCE_ENABLE_INSPECT=1`.
- Disable absolute jump / LogReplay / CallOutcome SC on default path (code may remain behind flag).

### Phase R1 — Engagement + HotSet
- Implement HotSet + default LeanOCC; HotLocal only for HotSet locs.
- Kill block-wide WaitHard; Bayes Wait only for HotSet.
- Fix metrics so lean actually increments.

### Phase R2 — HotLocal policy
- Bind-before-touch (M3) on HotSet when Data ready.
- WaitHard+park only HotSet; SpecRead otherwise.
- Selective invalidate retained.

### Phase R3 — Measure
- Smoke 13217637 + 19807137 @8; then 7-block sweep v8.
- Iterate thresholds H_w/H_a only if gates miss — no new inspect.

### Non-goals this round
- Mid-tx PC jump on ERC-20 (research flag only).  
- `(t,k)` continuation scheduler.  
- Beating OCC via more Bayes τ.

---

## 6. Mapping from old plant v2 milestones

| Old | Fate |
|-----|------|
| M0 metrics | Keep |
| M1* jump/inspect | Research flag off by default |
| M2 park/steal | Keep for HotLocal Wait |
| M3 prior Bind | Keep for HotSet Bind |
| M4 lean thresholds | **Replace** with default-Lean + HotSet escalate |

---

## 7. Bottom line

Adaptive SpecFence = **OCC everywhere cold, location-local SpecFence only on measured hot multi-writer chains**, with **zero default inspect tax**. That matches the fine-grain DAG regimes and explains v7’s regression.
