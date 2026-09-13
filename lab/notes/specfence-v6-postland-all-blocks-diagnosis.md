# SpecFence v6 post-land — all-blocks diagnosis (THIS tip `3376ac4`)

**Date:** 2026-09-13 (Asia/Shanghai, UTC+8)  
**Tip:** `3376ac4` — `feat(specfence): live v6 plant — OCC while PE empty, Mode(a) after abort`  
**PR:** #8 `cursor/specfence-v6-essence-af82` (local branch name still `cursor/specfence-v5-pc-cc-fusion-e28e`)  
**Vocab:** Spec=Region; Fence=Bind/WaitFor/SerialLane/OrderedAdmit; Soft=**0**; no P0/P1/P2 in design  
**Companion catalog:** `lab/notes/specfence-v6-postland-per-block-catalog.json`  
**Switch+Bind audit (folded):** `lab/notes/specfence-v6-switch-and-bind-tax-audit.md`  
**Plant SoT (superseded by v7):** `lab/notes/specfence-complete-architecture-v6-essence.md`  
**New arch SoT:** `lab/notes/specfence-complete-architecture-v7-essence.md`  
**Sweeps this tip (remeasured, not v5 copy-paste):**
- Soft=0 N=1 all 99: `lab/results/v6-postland-all-blocks-sweep.json`
- Soft=0 N=3 focus24: `lab/results/v6-postland-focus/focus-n3-summary.json`
- Process (named/worst): `lab/results/v6-postland-focus/process-*.json` + `all-blocks-process-*.json`
- Effect-raw (below-tx): `lab/results/effect-raw-deep-b{14689597,19469097,19606599,…}.json`
- Tip-landed digest (compare only): `lab/notes/v6-essence-sweep-summary.json` (median **0.795**)

**Ruthless bar:** median 0.795 / remasure 1.02 is **not** product victory while fan_out ≪ OCC.

---

## 0. Executive answer

### Why OCC and PCC are still not better fused

v6 **did** land the empty-PE OCC retreat (T6) and Mode(a) after abort heat, with S2 (`unfinished=!done`), cert-after-success Bind, and Soft=0. Fusion still fails because **PC and CC are stitched at the wrong seams**:

| Layer | What PC wants | What CC wants | Live plant |
|-------|---------------|---------------|------------|
| First wave | Ready-set Avoid of doomed consumers | Fence before RAW | **inc‑0 Spec → abort → PE** |
| Schedule | PE-satisfied Execute ∪ Validate ∪ Repair | Refuse Execute(t) on unpublished RAW | **ready-edge observe-only** (refuse deadlocked) |
| Access | Width for independents | Timely WaitFor/lane on star ℓ | **Bind-on-Data spray** (WaitFor≪Bind) |
| Repair | Fail-\(a\) R1 on certified prefix | Spec-only → B0 | **always `validate_occ_kernel` B0** |
| Learning | Edges + true-\(k\) + HotSet→posterior | Consume in admit + decide | **PE class + vis only**; ordinal dead; HotSet unused as verb |

**One line:** v6 fused **computers** (OCC retreat when PE empty) but not **control** — Fence is mid-read Bind theater after abort, not schedule-first Avoid + R1 repair. Median can clear 0.744 while **14689597 stays Bind-taxed**.

### Is the switch timely? Right place?

| Question | Verdict | Evidence |
|----------|---------|----------|
| Timely vs PE existence? | **Yes** | empty PE ⇒ OCC path; `has_any_predicted` flips Mode(a) |
| Timely vs first conflict? | **No** | inc‑0 always Spec; first RAW still B0 then PE (S1) |
| Right grain? | **Partial** | Mode(a) access-local; gate \(k\)=`dominant_k` not live ordinal; Repair still tx-B0 |
| Right stage? | **No** | Avoid should be ready-admit; Resolve should be fail-\(a\); plant Avoid=mid-read, Resolve=never (cert unused) |
| Right visibility? | **S2 yes; consume no** | unfinished=!done fixed; HotSet/WŜ/ready_edge/certs produced, not closing loop |

### Why median ok but fan_out collapsed?

Quiet / low-abort blocks ride **empty-PE OCC identity** → SF/OCC ≈1 (sometimes >1 on N=1 noise). Fan_out stars (**14689597**: effect-raw `max_program_fanout=448`, 476 consumers with program cross, first-cross depth≈0.86) **must** Fence the star **before** Execute of satellites. Plant instead:

1. Spec-stamps the star wave  
2. Aborts (N=3: **429** vs OCC **66**)  
3. PE templates `[1,6,10,20]` + sticky predicted ℓ  
4. Bind-on-Data **182–535** times (cert meta) with WaitFor only **20–41**  
5. Validate still B0 (`rebind=0`, `rewind=0`)  

So quiet wins the median; fan_out pays **meta + more aborts than OCC**.

---

## 1. Method (what was actually run on THIS tip)

| Step | Artifact | Soft | Iters |
|------|----------|-----:|------:|
| Rebuild example @ tip | `target/release/examples/specfence_all_blocks_sweep` | — | — |
| Per-block Soft=0 N=1 all 99 (panic-retry harness) | `lab/results/v6-postland-all-blocks-sweep.json` | **0** | 1 |
| Focus24 Soft=0 N=3 (OCC-slow filter) | `lab/results/v6-postland-focus/focus-n3-summary.json` | **0** | 3 |
| Process-trace named/worst | `lab/results/v6-postland-focus/process-*.json` | **0** | 1 |
| Effect-raw (prior below-tx, still truth) | `lab/results/effect-raw-deep-b*.json` | — | — |
| Switch+Bind static audit | `lab/notes/specfence-v6-switch-and-bind-tax-audit.md` | — | — |

Exclude-set / SoftWait Soft = **0** on all SF pairs.  
Empty block **19910734** dropped from nonempty (98).  
Intermittent `pevm.rs:776 unreachable!` on evaluate — harness retries per block (not a Soft path).

---

## 2. Headline numbers (ruthless)

### 2.1 Medians — tip digest vs remasure

| Metric | PC committed | **v6 tip digest** | **This remasure Soft=0 N=1** |
|--------|-------------:|------------------:|-----------------------------:|
| nonempty median SF/OCC | **0.744** | **0.795** | **1.021** |
| p10 / min | 0.468 / 0.234 | 0.538 / **0.162** | 0.739 / **0.440** |
| quiet median / p10 | 1.02 / — | **1.047 / 0.644** | **1.253 / 0.798** |
| ≥0.7 / ≥1.0 | 56 / 27 | 61 / 26 | 92 / 53 |
| Soft / edge_bind / wait | 0 / — | 0 / 972 / 1047 | **0** / 1080 / 1059 |
| aborts SF / OCC | — | 5474 / 3771 | 4098 / 3523 |
| **R1a / R1b** | — | — | **0 / 0** (B0 ≡ aborts) |

**Honesty rule:** remasure median **1.021** is Soft=0 and real on this run, but N=1 is noisy (OCC-slow inflates mean to **7.23**; named 19807137 N=1 once showed OCC **2777 ms**). Tip-landed digest **0.795** remains the plant-claim bar clear. **Neither** clears fan_out. Quiet p10 remasure **0.798** still **< 0.85** stretch; digest quiet p10 **0.644**.

### 2.2 Focus N=3 (product falsifiers)

| Block | Role | N=3 SF/OCC | bind / wait | ab SF/OCC | B0 / R1 |
|------:|------|----------:|------------:|----------:|--------:|
| **19807137** | spine | **0.225** | 37 / 299 | 858 / 561 | 858 / 0 |
| **14689597** | fan_out Bind tax | **0.336** | 182 / 30 | 429 / 66 | 429 / 0 |
| **15274915** | collapse | **0.387** | 16 / 3 | 42 / 25 | 42 / 0 |
| **6196166** | fan_out-ish | **0.401** | 7 / 6 | 75 / 79 | 75 / 0 |
| **19469096** | spine | **0.466** | 1 / 2 | 184 / 114 | 184 / 0 |
| 19606599 | spine | 0.644 | 1 / 35 | 80 / 83 | 80 / 0 |
| 19469097 | spine | 0.760 | 6 / 40 | 97 / 88 | 97 / 0 |
| 19606598 | quiet | 0.931 | 1 / 0 | 5 / 6 | 5 / 0 |
| 2179522 | quiet | **2.32** | 0 / 0 | 1 / 1 | 1 / 0 |

**14689597 ≥0.85 @8 N≥3: FAIL (0.336).**  
**2179522 N=3 >1 is OCC-slow — do not advertise** (sister OCC-fast historically ~0.24–0.75).

### 2.3 Named Bind-tax autopsy (14689597) — why ~0.16 / 0.34

**Effect-raw structure (below-tx):**
- `max_program_fanout` **448**; `n_consumers_with_program_cross` **476**
- first-cross depth p50 **0.857** (late in consumer effect stream)
- Example: consumer tx **3** first_program_cross_k=**6**, location `85335018835337005`, producer **0**

**Plant chain (file:fn) — audit §2 folded:**

1. `specfence_access_gate` **incarnation==0** → Spec ESTIMATE  
2. RAW → `validate_occ_kernel` B0 → `note_abort_access`  
3. No live ordinal (`access_log.note` **never called**) → fan_out **templates `[1,6,10,20]`** → `has_any_predicted`  
4. Computer flip: `next_sf_task` / Mode(a) for PE ℓ; gate \(k\)=`dominant_k.max(1)` (**wrong grain**)  
5. `decide` Bind when `unfinished==0 ∧ published_data ∧ park_ok`  
6. Bind arm: Data confirm → `note_fence_success` → **OCC continue** (no rem, no pin of later writers)  
7. SpecFence validate **always** `validate_occ_kernel` → B0 again; cert/`covers_all` **unused**

**Numbers:**

| Sample | SF/OCC | SF ms | OCC ms | bind | wait | ab SF | ab OCC |
|--------|-------:|------:|-------:|-----:|-----:|------:|-------:|
| Tip digest N=1 | **0.162** | 28.0 | 4.5 | 535 | 20 | 624 | 66 |
| Remeasure N=1 | **0.440** | 10.7 | 4.7 | 539 | 41 | 179 | 66 |
| Remeasure N=3 | **0.336** | 13.0 | 4.4 | 182 | 30 | 429 | 66 |
| Process N=1 | (wall 17.0) | — | — | 384 | 25 | 385 | — |

Variance is large; **direction is stable**: bind ≫ wait, ab_SF ≫ ab_OCC, B0≡aborts, R1=0.

**Process blindness:** `reason_histogram` shows `bind_published=0`, `wait_for_writer=0` while `edge_bind=384` — Mode(a) Bind does **not** `process.record`; decision_fields stay zero. Learning/telemetry under-counts Fence verbs.

**Concrete wall decomposition (N=3 approx):**
```
OCC useful ≈ 4.4ms for 564 tx
SF wall ≈ 13.0ms ≈ useful_EVM(+reexec) + park_idle + Bind/cert meta + schedule steal
  aborts 429 vs 66 → reexec storm (occ_kernel_execs 4931 on N=3)
  park=210, ready_steal=374 — idle theater without producer pin
  bind=182 certs that do not buy R1
```
**Why 0.16 on digest N=1:** same Bind tax + PE spray, worse abort amplification (624 vs 66 ≈9.5×) and longer wall (28 vs 4.5). Not a Soft storm. Not missing S2. **Fusion failure = Fence without Avoid/R1.**

---

## 3. Parallel-compute vs concurrency-control — detail

### 3.1 PC (what landed vs SoT)

| PC piece | SoT v6 | Live |
|----------|--------|------|
| Empty-PE OCC computer | yes | **yes** (`!has_any_predicted`) |
| Wave park/steal | yes | **yes** |
| Execute-first | yes | **yes** when wave |
| ReadyEdge observe | yes | **yes** |
| PE refuse Execute | yes | **NO** — `computer.rs` `let _ = ready` |
| Stage ready-set | PE∪Val∪Repair | **NO** — Block-STM indices |
| Steal only ready | yes | partial (wave only) |

**Ready-refuse deadlock (audit §3):** defer consumer until `note_producer_done(w)` while `w` not on collaborative index → workers spin. Plant abandoned schedule Avoid; Avoid remains access verb only. **First-wave fan_out cannot win.**

### 3.2 CC (Mode(a) verbs)

| Verb | When decide fires | Live effect | Fan_out win? |
|------|-------------------|-------------|--------------|
| Spec | empty PE / ¬pred / roi_skip / quiet_off | OCC read | width only |
| Bind | Data ∧ unfinished=0 ∧ park_ok | cert strip + OCC continue | **NO** — tax |
| WaitFor | unfinished=1 ∧ executing ∧ ¬quiet_off | park + note_fence | rare (≪Bind) |
| SerialLane | unfinished>1 ∧ park_ok ∧ executing | grant + admit; Ready→**occ_unfenced** | **ban path** |

**SerialLane Ready → admit + Spec continue** still live (`pcc_serial_lane`) — SoT ban not held when head not executing.

### 3.3 Below-tx Mode(a) Spec vs Fence — when switch vs conflict

| Epoch | Spec? | Fence? | Conflict already? |
|-------|------:|-------:|-------------------|
| First Execute of consumer, PE empty | yes | no | **latent** RAW |
| Abort validates | — | — | **yes** (B0) |
| Reincarnation PE ℓ, Data tip ready | maybe | **Bind** | conflict already paid |
| Reincarnation, writer executing | maybe | **WaitFor** | conflict in flight — only useful Fence |
| Validate after Bind cert | — | cert unused | miss → **B0 again** |

**True-\(k\):** effect-raw shows first cross at **k≈6** on star consumers. Plant gate uses `dominant_k` / templates — **learned but unused / wrong grain**.

### 3.4 WaitFor / Bind / SerialLane / B0 vs R1 / schedule idle / empty-PE vs PE-on

| Signal | Remeasure all-blocks | Fan_out 597 N=3 | Meaning |
|--------|---------------------:|----------------:|---------|
| edge_bind | 1080 | 182 | Fence mass |
| edge_wait_for | 1059 | 30 | WaitFor≪Bind on killer |
| full_restart B0 | 4098 | 429 | ≡ occ_aborts |
| rebind R1a | **0** | **0** | cert unused |
| rewind R1b | **0** | **0** | cert unused |
| wait_park | high on spine | 210 | idle |
| prefer_admit | **0** (v6 killed theater) | 0 | good vs v5 |
| detect_accesses | **0** | 0 | Detect museum off |
| empty-PE path | quiet ok | N/A after abort | median helper |
| PE-on path | Mode(a) | Bind tax | fan_out killer |

---

## 4. Learning — produced vs consumed; how learning should work

### 4.1 Map (audit §4 folded + remasure)

| Signal | Produced? | Consumed by decide/schedule/validate? | Gap |
|--------|-----------|--------------------------------------|-----|
| PE intra abort | yes | yes (flip computer + decide) | after first miss |
| PE prior seed | yes if !quiet | prior_pe_fire_wins | EV soft; quiet p10 |
| PE emptiness | yes | hybrid OCC | good |
| AccessOrdinal true-\(k\) | **API exists** | **note never called** | templates / dominant_k |
| \(e_{\mathrm{vis}}\) !done | yes | yes decide | S2 held |
| HotSet | yes | **posterior bump only** | not edges/PE |
| WŜ / rw_prior | yes | feeds hot flag only | unused as Admit |
| independence | sketch | FM9 Unfence | ok small |
| Certificate strips | note_fence_success | **validate ignores** | always B0 |
| ReadyEdge consumer | abort/WaitFor/ESTIMATE | **schedule ignore** | observe-only |
| Lane tokens | grant | partial | Ready→Spec |
| DecisionField / Bayes | lab | **not Fire** | unused |
| Morph fan_out/spine/quiet | learner | quiet_off / park_ok | under-powered EV |

### 4.2 What should be learned but wasn't

1. **True first-touch \(k\)** per (ℓ, consumer) from ordinal during PE-on Execute  
2. **Producer Stage reservation** so refuse is safe (learning of schedule topology)  
3. **EV[Fence vs B0]** calibrated on fan_out stars (Bind-count↑ ∧ abort↓ falsifier)  
4. **Star ℓ identity** from HotSet/fanout for ReadyEdge insert **before** satellite Execute  
5. **Cert coverage** → which fail-\(a\) may R1  

### 4.3 What was learned but unused

1. HotSet sizes (14689597 hot≈20–28) — not driving edges  
2. Certificate strips after Bind — not selecting R1  
3. ReadyEdge consumer bits — not admitting  
4. Effect-raw / DecisionField offline — not live EV priors  
5. `predicted_essential_hits` (215–581 on 597) — opens Bind tax more than Avoid  

### 4.4 How learning **should** work (prescription)

```
observe (abort / finalize / HotSet / WŜ / ordinal)
   → posterior PE(ℓ,k_true) + ReadyEdge(consumer←producer) + morph EV
   → CONSUME at three ports:
        (1) schedule admit: refuse Execute(consumer) iff producer Stage runnable
        (2) decide: WaitFor/lane preferred; Bind only if tip==conflict producer ∧ EV win
        (3) validate: RS_fence covered → R1 at fail-a; else B0 + train true-k
```

**Falsifiers:** Bind↑ without abort↓; PE spray with detect/decide tax on independents; cert without R1; ordinal templates on fan_out; ready observe without admit.

---

## 5. Top 10 failure modes (block + tx + access)

| # | Failure mode | Block | Tx / access | Evidence |
|--:|--------------|------:|-------------|----------|
| 1 | **Bind tax without abort relief** | **14689597** | star consumers after PE; Bind on published tip | N=3 bind=182 ab=429; digest bind=535 ab=624 |
| 2 | **First-wave Spec / no PE refuse Execute** | 14689597 | tx3… RAW at k≈6 vs producer 0 | effect-raw cross; ready refuse off |
| 3 | **True-\(k\) ordinal dead → template PE** | 14689597 | gate dominant_k / templates 1,6,10,20 | audit; access_log.note absent |
| 4 | **Always B0 validate (certs unused)** | all conflict | any Fenced tx | R1a=R1b=0 all remasure |
| 5 | **WaitFor≪Bind; park not pinning** | 14689597 | wait=30 vs bind=182 | N=3; digest wait=20 |
| 6 | **SerialLane admit + Spec continue** | PE multi-writer | Ready head path | `pcc_serial_lane` occ_unfenced |
| 7 | **Spine WaitFor park stampede** | **19807137** | wait=299 park=762 | N=3 sf_occ=0.225 |
| 8 | **HotSet/WŜ unused at edges** | 14689597 | hotset≈24 | note_hot_ws_posterior only |
| 9 | **Quiet p10 / prior-PE EV soft** | quiet cohort | prior PE Fire | digest p10 0.644; remasure 0.798 still <0.85 |
| 10 | **Process/decision telemetry blind** | 14689597 | Mode(a) Bind | reason_histogram bind=0 while edge_bind≫0 |

---

## 6. Conflicts not Avoided / not Resolved; txs that failed to parallelize

### Not Avoided (should have been schedule/access Avoid)

- **14689597** satellites of producer **0** (and other star writers): first-wave Execute Spec → RAW. Effect-raw: 476 consumers with program cross; fanout 448.  
- ReadyEdge `note_consumer` after the fact — **not** refuse Execute.  
- Bind-on-Data does **not** Avoid sibling Spec RS misses.

### Not Resolved (should have been R1)

- Every abort on SpecFence path: **B0 full_restart**. Cert strips from Bind/WaitFor never select rem/R1.  
- 14689597 N=3: 429 B0, 0 R1a, 0 R1b.  
- 19807137 N=3: 858 B0.

### Failed to parallelize (width lost)

- Fan_out: satellites serialized by abort storms instead of lane/WaitFor behind producer Done.  
- Spine (19807137): WaitFor park + steal without OrderedAdmit progress → idle≫useful.  
- Independents after PE spray: pay Mode(a)/cert meta even when Spec (detect=0 helps vs v5, Bind still taxes).

---

## 7. Block info → how to raise TPS (not only tx grain)

| Morph (from block info) | Access / edge / frame action | Expected TPS lever |
|-------------------------|------------------------------|--------------------|
| **fan_out** (597: fanout 448, depth≈0.86) | ReadyEdge refuse satellites until producer Stage Done; SerialLane on star ℓ at **true k**; Bind only tip==producer; R1 satellites | cut aborts toward OCC; kill Bind tax |
| **spine** (097/599/19807137) | OrderedAdmit along longest_rw_chain; steal **off-spine only**; WaitFor single head | cut park_idle |
| **quiet** | keep empty-PE OCC byte-identical; no template PE on lone abort | hold median; raise p10 via prior EV |
| **mixed** | per-class edges; minimal Fence surface | selective R1 |

**Frame ≠ tx:** decide at access \(a\); admit at Stage; repair at fail-\(a\). Tx sticky Fence is forbidden.

---

## 8. Major architecture optimization (bridge to v7)

1. **Producer-runnable PE refuse** (fix deadlock) — first-wave Avoid  
2. **Kill Bind-as-default Fence** — WaitFor/lane pin; Bind rare + EV  
3. **Restore true-\(k\) ordinal only when PE-on**; kill fan_out template spray  
4. **Validate split RS_spec / RS_fence → R1**  
5. **HotSet/WŜ → ReadyEdge + PE posterior** (consume)  
6. **Keep empty-PE OCC retreat** (median wins must not regress)

---

## 9. What NOT measured

- A/B ready-refuse with producer-stage reservation  
- A/B Bind EV gate vs current `park_ok`  
- Live `AccessOrdinalLog.note` restoration wall delta  
- R1 `covers_all` validate path wall on fan_out  
- Cross-block InterPrior quality beyond quiet seed skip  
- `profile_*_ns` breakdown (counters 0 on plant)  
- Finegrain per-access Mode timeline beyond process_summary  
- OCC-fast forced sister for every OCC-slow named sample  
- Causal A/B of HotSet→edge insert  

---

## 10. Story restated

v6 **won empty-PE OCC identity and a Soft=0 median ≥0.744** (digest 0.795; remasure N=1 1.021 noisy). It **lost fan_out**: on **14689597** the plant still **aborts first, sprays PE, Binds published tips, never refuses Execute, never R1s** — N=3 SF/OCC **0.336**, digest N=1 **0.162**. Median without fan_out is not fusion. v7 must hit essence: **wall = useful_EVM + idle + repair + meta** with Fence that wins the **first wave** without Bind-tax apocalypse.
