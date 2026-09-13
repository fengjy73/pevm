# Post-U1 multiblock parallel diagnosis (597 / 599 / 097 @8)

**Date:** 2026-09-11 (Asia/Shanghai)  
**HEAD:** `b50f336` + per-tx ProcessTrace instrumentation (this commit)  
**Branch:** `cursor/specfence-complete-cc-63b0`  
**Vocab:** Spec = Region; Fence = WaitFor / Bind / serial-lane+admit; Unfenced = optimistic  
**Fresh artifacts:**  
- `lab/results/post-u1-per-tx-{597,599,097}-c8.json`  
- `lab/results/exec-process-{14689597,19606599,19469097}-post-u1.json`  
- `lab/results/post-u1-sf-occ.json`, `post-u1-multiblock-parallel-summary.json`  
**Do not trust:** pre-U1 `exec-process-*-c8.json` (force_prefix Spec still live there).

---

## Honesty gate

| Claim | Fresh @8 fact |
|-------|----------------|
| U1 `force_prefix ∧ writer=None` Unfenced = 0 | **Confirmed 0** on 597 / 599 / 097 (`force_prefix_none_unfenced`, reason `unfenced_after_avoid` verb = 0) |
| Wall still ~3× OCC | **Yes — worse on this host run:** SF/OCC wall **4.5× / 3.3× / 3.2×** (597/599/097). Pre-U1 rich trace ~19.6 ms SF wall on 597; post-U1 ~18.0 ms — **U1 did not buy makespan**. |
| What replaced force_prefix Spec as dominant | **`unfenced_writer_done` fallthrough** (589 / 3237 / 1595) + **R2/R4 repair meta** (`rewind_to_cp` 88/170/211; work inflation 1.29/1.62/1.78×). Named class still **`Region_spine_admit_and_repair_identity`**. |

Hard bans unchanged (SoftWait Soft=0, Await@a=0, no 597 hardcodes).

---

## Method

1. Rebuilt ProcessTrace to aggregate **per-reader** bind / WaitFor / Unfenced reasons + parks (`process.rs`; `reader` was previously unused).  
2. `SPECFENCE_G7_TAG=post-u1` g7 smoke @ concurrency 8 with finegrain incarnation snapshot.  
3. Crossed with L1 DAG (`l1l2-b*.json`: wave / chain / fanout / morph) and SF vs OCC walls.  
4. Classified failures: **parallel compute/scheduler** vs **CC Region/Fence/Resolve** with file:fn.

---

## Per-block process (reason + util)

| Block | morph (L1) | wave / chain / fanout | bind / wait / unfenced | writer_done | u_aa_total | park_ns (Σ) | steal | abort | rewind | identity | multi_spine |
|------:|------------|----------------------:|-----------------------:|------------:|-----------:|------------:|------:|------:|-------:|---------:|------------:|
| **597** | fan_out | 434 / 29 / 448 | 760 / 72 / 3898 | 589 | 128 | 86.7 ms | 112 | 47 | 88 | 92 | 9 |
| **599** | long_chain | 261 / 61 / 14 | 550 / 55 / 8130 | **3237** | **1124** | 48.1 ms | 63 | 159 | 170 | 166 | 24 |
| **097** | long_chain≈WAW spine | 198 / 47 / 6 | 462 / 45 / 4466 | 1595 | 790 | 15.6 ms | 53 | 203 | 211 | 217 | 27 |

Histograms (per tx, mean / p90 / max):

| Block | wait | unfenced | bind | park | work_inflation (Σ(inc+1)/n) | max_inc |
|------:|------|----------|------|------|----------------------------:|--------:|
| 597 | 0.13 / 0 / 4 | 7.0 / 18 / 83 | 1.4 / 2 / 15 | 0.13 / 0 / 4 | 1.29× | 5 |
| 599 | 0.16 / 0 / 5 | 23.6 / 81 / 240 | 1.6 / 5 / 40 | 0.16 / 0 / 5 | 1.62× | **15** |
| 097 | 0.15 / 1 / 4 | 15.0 / 43 / 155 | 1.6 / 4 / 17 | 0.15 / 1 / 4 | **1.78×** | 9 |

Est. core idle from Σpark/(8·wall): **597 ~60%**, 599 ~18%, 097 ~10%. Steal ≫ WaitFor count → workers not stuck forever empty-ready, but **597 parks burn wall**.

Hot fan-out ℓ: **Fence-cover holds** — `unfenced_after_fence_on_hot_l = 0`; bind_after_avoid dominates star ℓ (597: 538 binds on ℓ `8533…`).

`prefer_admit` ≈ 0/1 — S1 almost never sees Ready (writer already Executing or absent).

---

## Concrete txs that failed to parallelize

### 14689597 (fan_out) — top fail_score

| tx | bind | wait | unf | u_aa | park | final_inc | Dominant verbs | Class |
|---:|-----:|-----:|----:|-----:|-----:|----------:|----------------|-------|
| **5** | 10 | 0 | 78 | 23 | 0 | 2 | writer_done 44, cold 19 | CC Avoid→UnfencedWriterDone |
| **8** | 9 | 1 | 69 | 14 | 1 | 2 | writer_done 28 | CC + light Wait |
| **11** | 14 | 2 | 78 | 10 | 2 | 3 | writer_done 40 | CC + park |
| **29** | 11 | 3 | 67 | 7 | 3 | 4 | wait serial+writer | CC repair + Wait park |
| **28** | 11 | 2 | 67 | 7 | 2 | 3 | canary/cold heavy | CC |
| **71** | 5 | 1 | 58 | 4 | 1 | **5** | cold 30, writer_done 20 | CC reexec×N |
| **30** | 15 | 4 | 83 | 0 | 4 | 4 | wait_for_writer 4 | **COMPUTE WaitFor** + reexec |
| **3** | 4 | 1 | 44 | 8 | 1 | 1 | writer_done 23 | CC |

Clique band **tx 3–36** carries parks + writer_done; independents elsewhere stay Unfenced (S2 OK — no over-Fence sample).

### 19606599 (nested / long_chain)

| tx | unf | u_aa | writer_done | final_inc | Class |
|---:|----:|-----:|------------:|----------:|-------|
| **187** | 240 | 81 | 147 | 3 | CC writer_done storm |
| **63** | 109 | 63 | 85 | **7** | CC reexec |
| **42** | 174 | 50 | 107 | 5 | CC |
| **203** | 112 | 30 | 80 | **15** | CC repair identity lost / cascade |
| **28** | 185 | 48 | 148 | 4 | CC |
| **45 / 31 / 50** | … | … | … | 4–5 | CC |
| **43** | 67 | 22 | 33 | 6 | CC + Wait park (wait=4) |

Almost no COMPUTE-primary (over-Fence indep ≈ 1 tx). Wall is **abort/repair + writer_done Unfenced**, not idle.

### 19469097 (WAW spine)

| tx | unf | u_aa | writer_done | final_inc | Class |
|---:|----:|-----:|------------:|----------:|-------|
| **34 / 36** | 155 / 85 | 36 / 32 | 102 / 50 | 4 | CC secondary-spine Unfenced |
| **320 / 322** | 79 / 20 | 28 / 18 | 54 / 18 | 4 / **9** | CC spine tip reexec |
| **224** | 74 | 19 | 35 | 6 | CC + Wait (park=4) |
| **25 / 27 / 29** | … | … | … | 2–3 | CC |
| **223 / 227 / 248 / 287** | 30 | 15 | 21 | 5 | CC multi-spine cohort |

`multi_spine_admit=27` is live; still high incarnation on secondary writers → **S4 admit ≠ resolved identity**.

---

## Conflicts: not Avoided vs not Resolved

### Not Avoided (should Fence/Bind, got Unfenced)

| Symptom | Evidence | Mechanism (file:fn) |
|---------|----------|---------------------|
| Avoid on ℓ, later access Unfenced | `unfenced_after_avoid_total` 128/1124/790 while reason verb `unfenced_after_avoid=0` | Loc counter: **avoid=true ∧ UnfencedWriterDone** — not the banned U1 fallthrough |
| Writer finished, no Data to Bind | reason `unfenced_writer_done` 589/3237/1595 | `vm.rs::fence_wait_for` — unfinished_exec=None, no `last_data_before`, serial pred done → Unfenced |
| Cold first wave on hot clique readers | `unfenced_cold` high on failing txs | Canary/cold before Avoid publish; then writer_done on repair incarnations |

These are **CC Region visibility** misses, not scheduler under-admission of independents.

### Not Resolved (abort → expensive R2/R4 / identity thin)

| Symptom | Evidence | Mechanism (file:fn) |
|---------|----------|---------------------|
| Suffix repair thrash | `rewind_to_cp` 88/170/211 | `rem.rs` / pevm validate → R2 SuffixRepair |
| Full restart | `full_restart` 5/31/24; `force_bind_reabort` 11/56/45 | R4 escalate; U4 preserves ℓ→writer counts (92/166/217) but **does not cheapen redo** |
| Incarnation inflation | max 5/15/9; work 1.29–1.78× | Repair re-arms force_prefix (now Fenced) yet still reexec body |
| Value-stable R1 rare | `rebind_only` 0/8/4 | R1 path underused vs rewind |

**Avoid** works on star ℓ (Bind-after-Avoid). **Resolve** after abort remains the wall — same `Region_spine_admit_and_repair_identity` named in pre-land notes, with U1 bare leak removed.

---

## Compute vs CC split

| Class | 597 | 599 | 097 | Verdict |
|-------|----:|----:|----:|---------|
| CC `unfenced_writer_done` / Avoid residual | **dominant** | **dominant** | **dominant** | CC Region/Fence |
| CC abort/reexec (inc≥2) | 52 txs | 59 | 85 | CC Resolve |
| COMPUTE WaitFor park (idle while others work) | 18 txs; Σpark ~60% core·time | 15; ~18% | 5; ~10% | **597 secondary compute/schedule tax** |
| Wrongly Fenced independents | ~0 | ~1 | ~0 | Not the wall (S2 holds) |
| U1 force_prefix None leak | **0** | **0** | **0** | Closed |

**597:** parallel **schedule** (WaitHard park + steal) is a real secondary tax on a fan-out morph that *should* keep P≈8 busy on independents — `scheduler` / `fence_wait_for` Blocking + wave park table.  
**599/097:** almost pure **CC** — repair identity + writer_done Unfenced; chain length already limits useful P.

---

## Ideal vs actual schedule → TPS recipe

### From L1 DAG

| Block | Ideal useful P | CP constraint | Actual SF behavior | TPS lever (native, not if-else) |
|------:|---------------:|---------------|--------------------|----------------------------------|
| **597** fan_out | **8** (wave 434≫8) | chain 29 | Parks on clique readers; independents OK Unfenced; repair on early band | (1) **Version-visibility Bind** when writer done but value stable (promote R1 / publish residual). (2) Keep Fence on star ℓ (already OK). (3) Park must not pin cores — admit/steal already; cut **false Wait** that becomes writer_done Unfenced after wake. |
| **599** long_chain | ≤8 but CP=61 bites | program chain 26 + handler nesting | writer_done×3k; inc up to 15 | (1) **Region ℓ→writer SoT through R2/R4** so repair Fences the same spine tip (U4 complete → cheap R1). (2) Do not Unfence writer_done on Avoid ℓ. (3) More P than 8 returns little. |
| **097** WAW spine | P limited by spine depth 47 | prog_chain=chain=47 | multi_spine admit live; secondary writers reexec | (1) **Multi-spine Region pipeline**: every unfinished writer on ℓ stays addressable across repair (S4+U4). (2) Structural serialization stays — TPS↑ = less redo on spine, not wider P. |

### Cross-block commons vs morph-specific

| Common (all three) | Morph-specific |
|--------------------|----------------|
| U1 leak **gone**; hot ℓ Fence-cover **holds** | 597: park idle tax + huge fanout Bind success |
| Dominant waste = **writer_done Unfenced** + **R2 rewind** | 599: nested long_chain, extreme writer_done count, max_inc 15 |
| Soft/Await banned paths stay 0 | 097: WAW spine, multi_spine_admit highest, abort highest |
| `prefer_admit`≈0 | 598 quiet neighbor: SF/OCC ~0.32, tiny parks — prior revoke (U6) not the wall |

### Method update post-U1

Pre-U1 bottleneck ranking put bare `force_prefix∧None` Spec as #0. **That counter is dead.**  
Next bottleneck to attack is still **`Region_spine_admit_and_repair_identity`**, now visible as:

1. `fence_wait_for` → **UnfencedWriterDone** on Avoid/essential ℓ (visibility hole, not hang-freedom U1).  
2. R2/R4 **rewind / reexec** despite `writer_identity_preserved` counters.  
3. On fan_out only: WaitHard **park Σ time** as compute tax.

Do **not** chase SoftWait / Await / morph Storm / 597 index doors. Prefer native: Region table as SoT, Fence = version barrier, Resolve = R1 when value-stable, spine admit as scheduler law.

---

## Answers to the task questions

1. **Top failing txs:** 597: 5,8,11,29,28,71,30,3; 599: 187,63,42,203,28,45,326,31; 097: 34,36,320,322,224,29,25,27.  
2. **Compute vs CC:** CC-dominant on all; 597 adds secondary WaitFor-park compute tax.  
3. **Avoid misses:** UnfencedWriterDone under Avoid; **Resolve misses:** R2 rewind / high incarnation (R1 rare).  
4. **TPS recipe:** Bind/visibility on writer-done Avoid ℓ + cheap identity-preserving Resolve; fan_out also needs park not to serialize independents; spine morphs need less redo not more P.  
5. **Next bottleneck still `Region_spine_admit_and_repair_identity`?** **Yes** — U1 cleared the force_prefix leak; wall ~3–4.5× remains that class (writer_done + repair meta).

