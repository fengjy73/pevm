# SpecFence v8 — all-blocks process mishandle (below-tx)

**Date:** 2026-09-14 (Asia/Shanghai, UTC+8)  
**Tip:** `bb67ff7` Soft=0 PC⊗CC  
**Primary autopsy:** `lab/notes/specfence-v8-waitfor-abort-r1-autopsy.md`  
**Codepath (5 mechanisms folded):** `lab/notes/specfence-v8-waitfor-r1-codepath.md`  
**Catalog:** `lab/notes/specfence-v8-all-blocks-catalog.json`  
**Evidence:** N=1/N=3 sweeps under `lab/results/v8-waitfor-autopsy/`; per-tx process dumps; effect-raw `lab/results/effect-raw-deep-b14689597.json`

**Mishandle = Avoid/Resolve/Detect/schedule stage that failed the SoT verb** (should WaitFor but Spec; useless WaitFor; Resolve miss B0 despite cert; wrong Bind-after-Done; ProducerStage/ready refuse wrong). Soft=0 held.

---

## 0. All-blocks summary (98 nonempty)

| Signal | Value |
|--------|------:|
| WaitFor total | **2260** |
| Bind total | **1895** |
| R1a / R1b | **4 / 0** |
| aborts SF / OCC | **3578 / 3272** (≈) |
| Soft≠0 blocks | **0** |
| Median SF/OCC | **0.728** (prior bar 0.744 — miss) |
| WaitFor>0 blocks | 70/98 |
| Process top-worst morph | spine / park_idle / full_restart |

**Cohort read (catalog `cohort`):**
- **quiet_occ_identity** — empty PE, Wait=Bind=0, SF≈OCC (carries median).
- **wait_up_abort_flat** — WaitFor↑ but |ab_SF−ab_OCC|/OCC <15% (core pathology).
- **bind_heavy** — Done→Bind or Bind-rare spray after PE.
- **wait_heavy** — Wait≫Bind; still R1=0 (19807137 class).
- **spine / fan_out** — named tails.

Process-trace reason totals (15 worst SF/OCC process files): `wait_for_writer` **856**, `bind_published` **453**, `unfenced_cold` **256**, **`wait_for_serial` 0** — SerialLane Ready path records WaitForSerial rarely in worst set; Executing SerialLane collapses into `wait_for_writer`.

---

## 1. Below-tx truth — 14689597 star

Effect-raw:
- `max_program_fanout` **448**; consumers with program cross **476**
- Dominant producer **tx 38**; location **`85335018835337005`**
- First-cross **k=6** (473/473 on star); depth_frac ≈ **0.86** (late-in-tx → WaitFor Aborting throws almost-finished work)

Process one-shot Soft=0 @8:
- hot_fanout_l: **WaitFor 45 / Bind 490 / Unfenced 0** on the star ℓ
- metrics: Wait 60, Bind 518, aborts 44, **R1a=0 R1b=0**, Soft=0
- `mixed_verb_intra_tx` **2** (rare in aggregate; sibling Spec often via cold Unfenced flush undercounted vs Bind path)

**Class counts on 473 star consumers (joined effect-raw × per_tx):**

| Class | n | Stage failure |
|-------|--:|---------------|
| **BIND_AFTER_PRODUCER_DONE** | **442** | Avoid too late: producer Done → `pcc_wait_for_writer` Bind+`occ_unfenced` (or decide Bind). Should have WaitFor while Executing / schedule-refused earlier. |
| WAIT_AND_BIND_MIXED | 27 | Park then later Done→Bind on same/other accesses; Aborting-shaped WaitFor (M1). |
| WAITFOR_BUT_SPEC_SIBLINGS | 1 | Resolve: cert ℓ + Spec sibling → `covers_all` fail → B0 (M2). |
| BIND_PLUS_SPEC_SIBLINGS | 1 | Bind without covering Spec residual. |
| WAITFOR_ONLY_ABORTING_SHAPED | 1 | Pure WaitFor still FullRetry; R1 dead (M1+M3). |
| NO_PROCESS_TRACE | 1 | Consumer in effect-raw without process slot. |

**Dominant mishandle:** not “forgot WaitFor verb in decide,” but **timing** — satellites arrive after producer Done so Avoid becomes Bind theater; the 45 early WaitFors still do not cut aborts (Aborting+B0). First wave before PE remains Spec (M4).

---

## 2. Top 10 mishandled txs/processes

| # | block | tx | access / note | class | stage | wait/bind/unf/park |
|--:|------:|---:|---------------|-------|-------|--------------------|
| 1 | 14689597 | **29** | ℓ star `8533…7005` k=6 producer 38 | WAITFOR_BUT_SPEC_SIBLINGS | Validate/Resolve (`covers_all` fail) | 3/2/1/3 |
| 2 | 14689597 | **40** | same star k=6 depth 0.86 | BIND_PLUS_SPEC_SIBLINGS | Execute/Validate | 0/2/1/0 |
| 3 | 14689597 | **9** | same star | WAITFOR_ONLY_ABORTING_SHAPED | Execute→Validate (Aborting+FullRetry; R1 dead) | 2/0/0/2 |
| 4 | 14689597 | **25** | same star | WAIT_AND_BIND_MIXED | Execute (park + Done→Bind) | 5/2/0/5 |
| 5 | 14689597 | **26** | same star | WAIT_AND_BIND_MIXED | Execute | 5/2/0/5 |
| 6 | 14689597 | **28** | same star | WAIT_AND_BIND_MIXED | Execute | 5/2/0/5 |
| 7 | 14689597 | **36** | same star | BIND_AFTER_PRODUCER_DONE | Execute/Avoid too late | 0/6/0/0 |
| 8 | 14689597 | **11** | same star | BIND_AFTER_PRODUCER_DONE | Execute/Avoid too late | 0/3/0/0 |
| 9 | **6196166** | **91** | worst SF/OCC N=1 (0.295) | WAITFOR_BUT_SPEC_SIBLINGS | Validate/Resolve | 2/1/2/2 |
| 10 | **6196166** | **26** | same block | WAITFOR_BUT_SPEC_SIBLINGS | Validate/Resolve | 1/1/2/1 |

**Also severe (19807137 — Wait↑ abort≫OCC):**
- UNF_ONLY Avoid-fail cohort n≈73 (e.g. tx 148,154,195): `SHOULD_WAITFOR_BUT_SPEC` — PE hit cold Unfenced, no Wait/Bind.
- WAIT+UNF n≈58 (e.g. tx 231,261,303): WaitFor then Spec sibling → B0 (M2); R1a only **1–5** on whole block vs Wait **409**.

Producer **tx 38** on 14689597 is the RAW hub — not “mishandled consumer,” but schedule must PreferAdmit/ProducerStage it before satellite Execute; plant still lets satellites enter and Spec/Bind-race.

---

## 3. Stage taxonomy (what failed)

| Stage | Failure mode | Evidence |
|-------|--------------|----------|
| **Detect / PE** | First wave empty PE → no decide | quiet_occ; aborts train PE; WaitFor post-abort (M4) |
| **Execute / Avoid** | Should WaitFor but Spec (`UnfencedOcc`) | Ready/!executing fallthrough `occ_unfenced`; EV/quiet brake; 19807137 UNF_ONLY |
| **Execute / Avoid** | WaitFor decided but Done→**Bind** | hot ℓ Bind 490 vs Wait 45; BIND_AFTER_PRODUCER_DONE ×442 |
| **Execute / WaitFor** | Park useless / Aborting-shaped | M1; wait_park≫useful R1; tx9 Wait-only still B0 |
| **Validate / Resolve** | Cert present, B0 | M2 covers_all; M3 value_stable; R1a=0 on 14689597 |
| **Repair R1b** | CertifiedPrefixSkip never | `rewind_to_cp=0` all blocks; SuffixRepair not default |
| **ProducerStage / ready** | Refuse wrong / too weak | Edges reserved on vis/hot but consumer still Executes; SerialLane Ready = Spec canary not park (`pcc_serial_lane`); `wait_for_serial≈0` in worst process set |

---

## 4. Mechanism → mishandle map (codepath five)

1. **M1 Aborting WaitFor** → WAITFOR_ONLY_ABORTING_SHAPED, WAIT_AND_BIND_MIXED parks that still FullRetry.  
2. **M2 Spec siblings** → WAITFOR_BUT_SPEC_SIBLINGS, BIND_PLUS_SPEC_SIBLINGS; R1a dead.  
3. **M3 value_stable incarnation** → explains R1=0 even when covers_all would pass on single ℓ.  
4. **M4 Quiet/EV late WaitFor** → first-wave Spec; BIND_AFTER_PRODUCER_DONE volume (Avoid after the fact).  
5. **M5 inc==0 cert clear** → Resolve miss after same-incarnation Blocking retry.

---

## 5. Worst tails (N=1 Soft=0)

| block | sf_occ | Wait | Bind | ab SF/OCC | dominant mishandle class |
|------:|-------:|-----:|-----:|----------:|--------------------------|
| 6196166 | 0.295 | 50 | 91 | 79/72 | Wait+Unf + Bind tax; park_idle |
| 19807137 | 0.315 | 462 | 290 | 821/599 | Wait heavy, abort **≫** OCC; Unf cold |
| 15274915 | 0.386 | 26 | 29 | 47/29 | full_restart; Wait not cutting |
| 14689597 | 0.437 | 24 | 9 | 33/51 | star Bind-after-Done (N=1 low Bind race) / N=3 Bind flood |
| 19469096 | 0.434 | 96 | 50 | 145/123 | Wait↑ abort≈OCC |

---

## 6. Not measured

- Per-access decide reason when falling Unfenced (roi_skip vs !predicted vs !ev_win).  
- Exact producer status (Executing/Done/Ready) at each star consumer’s first cross.  
- Refuse_count / ProducerStage promote latency in sweep metrics.  
- Which invalid locs failed covers_all vs value_stable on each B0.  
- Stable N=3 **median** Bind/Wait (sweep last-iter only) — 14689597 Bind 7 (prior) vs 500 (last-iter) is race, not two plants.

---

## 7. Verdict

**Below-tx: the star is fenced too late and too narrowly.** 442/473 satellites Bind-after-Done; the minority WaitFor path is Aborting-shaped and cannot R1 past Spec siblings. All-blocks: WaitFor↑ is compatible with abort≈OCC and R1 death — process mishandle is schedule/Avoid timing + Resolve protocol, not missing `edge_wait_for` telemetry.
