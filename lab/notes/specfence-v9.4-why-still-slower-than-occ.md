# SpecFence v9.4 — WHY wall still loses to OCC (Soft=0 honesty)

**Date:** 2026-09-14 (Asia/Shanghai, UTC+8)  
**Tip:** `4b76dab` (PR #11; plant `c42f96a`)  
**Honesty:** nonempty median SF/OCC **0.703**; 14689597 N=3 **0.449**; WaitFor 4875; Bind **0**; pin **5158** / aborting **177**; schedule_refuse **1540**; R1 **3/2439**; Soft=**0**.  
**Beat prior wall** 0.685 / 0.348 — **still ≪ OCC and ≪ product bars** (median≥0.95, fan≥0.90, R1≥50%).  
**Posture:** ruthless root-cause. Celebrate neither pin↑ nor refuse↑. No plant code changes in this note.  
**Evidence base:** `lab/notes/v9.4-sot-partial-sweep-summary.json` + honesty MD (this tip); raw sot-partial JSON not in tree — abort/EVM/resume shape from prior Soft=0 all-blocks `lab/results/v9.4-full-land-*-sweep.json` @ `3687da6` as **proxy**, labeled where used.

---

## 0. One-line verdict

**We swapped AbortingThrow theater for PinHold/refuse theater without cutting the OCC abort class or converting certs into R1.** Wall is still `repair ≈ OCC B0` + **park/meta tax OCC never pays**, so SF/OCC sticks ~0.7.

SoT falsifier still live: **WaitFor↑ ∧ abort≈OCC ∧ R1≈0**.

---

## 1. Wall budget: useful_EVM vs idle vs repair vs meta

SoT (`v9.1`): `wall = useful_EVM + idle + repair + meta`. Product needs useful_EVM fraction up; this tip did not.

| Bucket | What it is on this plant | Evidence | vs OCC |
|--------|---------------------------|----------|--------|
| **useful_EVM** | First successful EVM of each tx that commits | Proxy full-land: median `evm_entries/n_tx ≈ 1.26` on SF (OCC row leaves `evm_entries=0` in harness — compare via abort/reexec) | OCC finishes more txs per wall ms → SF useful fraction **low** |
| **repair** | Validate fail → B0 FullRestart / suffix reexec | Proxy: SF `occ_aborts` **3673** vs OCC **3362** (n=98); median SF/OCC abort ratio **≈0.96**; **69/76** blocks with SF aborts ≥0.8×OCC; median `reexec/abort ≈ 2.7` | **Still OCC abort class** (order-of-magnitude equal, often worse on fan) |
| **idle** | PinHold/WaitFor park; refuse spin on Executing producer | This tip: WaitFor **4875**, pin **5158**; named **19807137** N=3 wait **891** / pin **971**; full-land `wait_park_count` large, **`resume_count` sum = 0** | OCC has **no** PinHold park; SF pays park wait **then** often head reexec |
| **meta** | admit / ready_edge / Bayes decide / cert strips / R1 attempts that fall through | R1 **3/2439** (rate **0.0012**); bind_after_done **214**; schedule_refuse **1540**; PE/decide on every PE hit | Pure tax when Resolve does not convert |

**Named block read (this tip N=3 @8):**

| bn | sf_occ | Wait | pin | aborting | R1 | refuse | Story |
|---:|-------:|-----:|----:|---------:|---:|-------:|-------|
| 14689597 | **0.449** | 246 | 274 | 4 | 0/74 | **160** | refuse helps a little; still mid-tx Wait + R1 dead |
| 19807137 | **0.231** | **891** | **971** | 0 | 0/537 | **0** | **no schedule-first**; park-idle dominates |
| 19606599 | 0.831 | 147 | 153 | 6 | 0/56 | 0 | closer; still R1 0 |
| 19469097 | 0.508 | 160 | 161 | 3 | 0/49 | 0 | same shape |

Quiet morph can look ≥1 (proxy quiet median ~1.06) while **spine median ~0.64** — the wall is fan/spine repair+park, not quiet cold.

**Causal chain (incomplete land, not “need new architecture”):**

```
admit under-covers storage RAW / ReadyCanary when w !Executing
  → mid-tx WaitFor PinHold (5158)  [AbortingThrow mostly killed: aborting 177]
    → wake same-incarnation but prefix resume ≈ never (resume_count≈0)
      → head reexec ≈ FullRetry tax  +  validate R1 theater (3/2439)
        → fallthrough validate_occ_kernel B0  ≈ OCC abort volume
          → useful_EVM↓ + idle↑ + repair≈OCC + meta↑  → SF/OCC ~0.70
```

Pin↑ / aborting↓ / refuse↑ moved **labels**, not the wall equation.

---

## 2. Top wrong / fake / counterproductive lands (TPS damage rank)

Ranked by how much they keep SF slower than OCC. Each is a **claimed SoT land that does not do SoT duty** (or actively adds tax).

| # | Damage | file:fn | Why slower than OCC |
|---|--------|---------|---------------------|
| **1** | **★★★★★** | `vm.rs:pcc_wait_for_writer` + `rem.rs:try_arm_park_resume_at_k` + `pevm.rs` Blocking/`add_pin_hold` | **PinHold without prefix resume.** SoT PinWithoutThrow = park **keeping rem/PC**; live park sets `ParkKind::PinHold` + `note_fence_success`, but WaitFor does **not** arm EffectBoundary/checkpoint. Wake → `try_apply_park_resume` → almost always **FullRetry** (`resume_count`/`rewind_to_cp` ≈ 0 on Soft=0 sweeps). Net: **OCC-class head reexec + park idle OCC never pays**. |
| **2** | **★★★★★** | `executor.rs:validate_specfence` → `validate_occ_kernel` | **R1 path is theater.** Attempts **2439**, wins **3**. `repair_grain` / `covers_all` fail on Spec-sibling invalids; `value_stable` rarely true; R1b still `try_validation_abort` then suffix. Fallthrough = **same B0 abort class as OCC**, plus cert/Bayes meta first. |
| **3** | **★★★★☆** | `scheduler.rs:try_execute_ready` (Executing-only refuse) + `fence_act.rs:act_wait_for` **ReadyCanary** | **Schedule-first Avoid incomplete.** Refuse only while `is_executing(w)`; if producer Ready/Validated/Aborting → **fall through canary Execute** → mid-tx PinHold or Unfenced Spec → abort. SoT: known consumers must not start. Canary is the leak that keeps WaitFor volume high after refuse “landed”. |
| **4** | **★★★★☆** | `admit.rs:admit_seed_begin_block` (Basic(addr) @ `FAN_STAR_K=6` only) | **true-k fake for storage stars.** Hints seed account Basic PE; hot RAW is often **storage**. PE miss → `decide_queried` UnfencedOcc → Spec → **OCC abort**. Refuse cannot fire (no consumer edge / no PE). Explains **19807137 refuse=0** with pin **971**. |
| **5** | **★★★☆☆** | `vm.rs:pcc_wait_for_writer` `note_fence_success` before park; DoneUnfenced cert | **Cert strips without covers_all.** WaitFor/DoneUnfenced write strips; sibling Spec locations stay uncertified → R1 attempts increment, win≈0. Strip survival (M5) landed; **Resolve conversion did not**. |
| **6** | **★★★☆☆** | Bind-rare land (`fence_act::bind_ev_from_query`) | **Correct volume fix, wrong wall.** Bind **2208→0** good; replaced by WaitFor/pin mass **without abort↓**. SoT explicitly bans celebrating WaitFor↑∧abort≈OCC. |
| **7** | **★★★☆☆** | `computer.rs:next_sf_task` + refuse while head of `execution_idx` | **schedule_refuse can idle.** Refuse returns `None` without advancing collaborative index (wave Some head path) → yield until producer moves; useful only if ProducerStage/steal fills cores. On blocks with refuse=0, this verb never Avoids. |
| **8** | **★★☆☆☆** | `access_policy.rs:decide_queried` `ev_pin` / `known_star` | **decide←Bayes opens PinHold doors that Resolve cannot cash.** `ev_pin_beats_abort \|\| depth_frac≥0.50 \|\| known_star` arms WaitFor; without live R1 / prefix resume, Bayes EV is a **tax actuator**. |

Honorable mention (still OCC-equal): ESTIMATE `BlockingOther` residual (`estimate_park_kind(false)`) — aborting **177** left; small vs pin but still AbortingThrow class.

---

## 3. schedule_refuse **1540** — useful Avoid or idle/starvation?

**Mixed; on the wall blocks, mostly incomplete Avoid → residual idle/park, not a win.**

| Signal | Reading |
|--------|---------|
| Verb fires (0 → **1540**) | Code path live: `ReadyEdgeTable::defer` + `next_sf_task` `record_schedule_refuse_n` |
| Gate | `try_execute_ready`: refuse **only if** `!may_execute` **and** `is_executing(w)` |
| If `w` not Executing | **ReadyCanary fallthrough** — refuse does not run; consumer Executes |
| 14689597 N=3 refuse **160** | Some Avoid; still Wait **246** / R1 **0/74** / sf_occ **0.45** — refuse insufficient |
| **19807137 refuse 0** | Worst block: **no** schedule-first; Wait/pin dominate → refuse metric **does not explain** that wall |
| Head refuse without `execution_idx` advance | Worker can **yield-spin** on a refused Ready head until producer progresses (ProducerStage promote mitigates; not proven dominant) |

**Verdict:** refuse is **directionally SoT** when it keeps known consumers out **before** mid-tx Fence. Today it is **under-triggered** (Executing-only + under-seeded edges) and **over-counted relative to abort cut**. Treat **1540 as scaffolding**, not proof of Avoid. Useful Avoid ⇒ refuse ≫ mid-tx WaitFor **and** abort≪OCC; we have refuse≪Wait on the worst block and abort≈OCC.

---

## 4. PinHold **5158** with R1≈0 — pin without resolve win = tax?

**Yes. PinWithoutThrow without Resolve/prefix-resume payoff is a tax.**

Prior wall (`3687da6`): pin **2061** / aborting **3280**.  
This tip: pin **5158** / aborting **177**.

What changed: pevm Blocking arm uses `add_pin_hold` (no `Aborting`) for `ParkKind::PinHold`; ESTIMATE PE-known → PinHold. **AbortingThrow mass dropped.**

What did **not** change enough:

1. **No prefix resume on product Soft=0 path** — `resume_count` / `rewind_to_cp` ≈ 0 (proxy all-blocks). Wake is same-incarnation Ready + head reexec (FullRetry), not SoT “keep rem/PC”.
2. **R1 does not absorb** pin-certified fails — **3/2439**.
3. Park idle + steal/meta still paid; OCC speculative readers often finish and abort cheaper than park→wake→reexec.

**SoT falsifier:** PinHold volume ↑ while abort≈OCC and R1≈0 = **non-land** of PinWithoutThrow (status bit fixed; cost model not).

---

## 5. What still equals the OCC abort class after “all this”

Despite Bind rare, PinHold, refuse, decide←Bayes, cert survival:

| Mechanism | Why it is still OCC abort |
|-----------|---------------------------|
| `validate_specfence` fallthrough | Almost every cert-bearing fail → `validate_occ_kernel` → `try_validation_abort` + `convert_writes_to_estimates` + `record_occ_abort` / `full_restart` |
| UnfencedOcc Spec reads | PE miss / quiet_off / roi_skip / independence → same MV conflict detect as OCC |
| ReadyCanary | Spec execute against unfinished producer → invalid RS → B0 |
| PinHold wake FullRetry | Same-incarnation **head** reexec; MV still Estimate-converted on later validate abort |
| R1b “win” | Still `try_validation_abort` + selective invalidate — abort-class repair, not rebind-in-place |
| Proxy abort counts | SF aborts **≈** OCC aborts (median ratio ~1.0; often SF **>** OCC on fan) |

**PinHold removed Aborting *status* from WaitFor; it did not remove abort-*class* repair volume.** That is why pin↑ can coexist with sf_occ≪1.

---

## 6. Concrete fix list — still “land SoT correctly” (no new architecture)

Ban: SoftWait Soft, new π, dual computer, patch-salad metrics. Do: finish call-order duties already named in v9.1/v9.3/v9.4.

| Order | Fix | Where | Done when (Soft=0 JSON) |
|------:|-----|-------|-------------------------|
| 1 | **Arm rem checkpoint / certified prefix before PinHold park** so wake ResumeAtK (not FullRetry) | `vm.rs:pcc_wait_for_writer` (EffectBoundary + certified loc); ensure `try_apply_park_resume` sees intent after `wake_writer_done` | `resume_count` / `park_resume_at_k` ≫ 0; `park_resume_full_retry` ≪ pin |
| 2 | **Refuse known consumers while producer unfinished — not only Executing** *without* reintroducing v6 yield-spin: if `w` Ready → **prefer-admit ProducerStage(w)** and keep consumer deferred (no ReadyCanary Execute) | `scheduler.rs:try_execute_ready`; `fence_act::act_wait_for` ReadyCanary path | mid-tx WaitFor ≪ schedule_refuse on fan; **19807137 refuse ≫ 0** |
| 3 | **admit_seed true-k for storage RAW stars**, not only Basic(addr)@k≈6 | `admit.rs` + feeder/hints | PE-on at real conflict ℓ; fan PE hit correlates with refuse not Unfenced |
| 4 | **R1a must convert when fenced RAW covers fail set + tip snap/value-stable**; stop counting attempts that always B0 | `executor.rs:validate_specfence`; identity/snap install on WaitFor Bind path | R1 win **≥50%** cert-bearing attempts; abort ≪ OCC on fan |
| 5 | **Do not `note_fence_success` on DoneUnfenced / empty-progress Wait** unless strip will cover validate fails; cert only successful Fence that constrains RS | `vm.rs` DoneUnfenced / WaitFor | `r1_attempt` drops or win rate rises; bind_after_done not R1 bait |
| 6 | **R1b SuffixRepair only when EV beats B0 *and* prefix preserved**; if R1b == abort+reexec with no prefix skip, it is B0 — do not call it a win for bars | `executor.rs` R1b arm | `reexec_entries` / `occ_aborts` clearly below OCC on named fan |
| 7 | **Kill ReadyCanary Spec against known star edges** (canary discovery only when edge unknown) | `vm.rs` SerialLane/WaitFor ReadyCanary; `act_wait_for` | canary_probes on known consumers → 0 |
| 8 | **Honesty gate on land** | sweep harness | Ship only if: Soft=0 ∧ abort≪OCC on fan ∧ R1≥50% ∧ median→0.95 — **not** if only pin↑/refuse↑/Bind↓ |

Already OK / do not re-land: Soft=0; Bind rare EV (`known_star`≠Bind); cert strip survival across same-incarnation; spine unity (`next_sf_task`); dual-π hot delete.

---

## 7. Top 8 wrong places (quick index)

1. `crates/pevm/src/vm.rs` — `pcc_wait_for_writer` (PinHold park + cert-before-park, no rem checkpoint)  
2. `crates/pevm/src/specfence/rem.rs` — `try_arm_park_resume_at_k` → FullRetry default on product parks  
3. `crates/pevm/src/specfence/executor.rs` — `validate_specfence` R1 theater → `validate_occ_kernel`  
4. `crates/pevm/src/scheduler.rs` — `try_execute_ready` Executing-only refuse + ReadyCanary hole  
5. `crates/pevm/src/specfence/admit.rs` — `admit_seed_begin_block` Basic-only true-k  
6. `crates/pevm/src/specfence/fence_act.rs` — `act_wait_for` ReadyCanary  
7. `crates/pevm/src/specfence/access_policy.rs` — `decide_queried` pin doors without Resolve cash  
8. `crates/pevm/src/specfence/computer.rs` — `next_sf_task` refuse accounting without abort cut  

---

## 8. Essence

**User is right:** still slower than OCC because many “lands” are **wrong or fake welds** — PinHold/refuse/Bind-rare moved counters while **repair stayed OCC-shaped** and **Resolve stayed token**.  

Next honest move is not more WaitFor volume or pin theater: **prefix-resume PinHold + schedule refuse that blocks canary + storage true-k admit + R1 that actually wins**, measured by abort≪OCC and R1≥50% under Soft=0 — not by pin/refuse trophies.

