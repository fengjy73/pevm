# SpecFence conflict-resolution campaign — retrospective

**Paused tip:** `59754eb` (2026-09-08 Asia/Shanghai)  
**Authority:** `specfence-native-resolve-protocol.md` (not OCC++)  
**CC frame:** `specfence-v5-detect-avoid-resolve.md` — primary failure was **resolve**, not detect/avoid  
**Pause reason:** user chose pause + retrospective over another architectural bet after ~13ms plateau

---

## 1. Arc (what we set out to do)

After V5 shovel + dig: SpecFence lost to OCC because **repair cost grain ≠ concurrency grain** (ForceBind + head reexec; SoftWait scarce and not the hole). User ordered: solve resolve properly; SpecFence is **not** an optimized OCC — 大刀阔斧.

Campaign goal: native resolve (SuffixRepair / Bind / Await) that cuts makespan toward OCC without Wait storms or hung inspect.

---

## 2. Scoreboard (597 @8, representative)

| Era | Tip | SoftWait | wall median (approx) | OCC wall | SF/OCC-ish | Key state |
|-----|-----|--------:|---------------------:|---------:|-----------:|-----------|
| Dig Lean | `3a3e26b` | ~41 | ~36 | ~5.5 | ~0.15 | ForceBind head; fb_reabort~49% |
| Native SuffixRepair (fake resume) | `c13ee14` | ~41 | ~46 | — | ~0.09 | RewindTo armed, `!lean` blocked resume |
| Real Lean resume | `fbbd321` | ~50 | ~33 | ~3.6 | — | resume_count>0; jump rare |
| Sticky resolve | `91e8d22` | ~38 | ~24 | ~3.5 | ~0.14 | aborts→OCC order; fb_reabort↓ |
| Cold + RebindOnly | `c0117e9` | ~37 | ~22 best | ~3.7 | — | cold_spec_fast; noisy |
| Stabilize median | `1c1ad81` | ~33 | **~29** | ~3.3 | — | multi-iter harness |
| Profile root | `ade501d` | ~40 | ~29–30 | ~3.6 | — | **park idle + maybe_wait**, not EVM |
| Park subtypes | `2e141f2` | ~52 | ~27–30 | — | — | **BlockingOther** owns idle |
| ESTIMATE cascade | `169eece` | ~48 | ~25 best | ~3.4 | — | BO parks 1519→181 |
| BO≈0 (ODR/ESTIMATE→Spec) | `97071cc` | ~125 | **~22** | ~3.4 | — | park 407→19 |
| SoftWait Soft=0 | `c794c67` | **0–1** | ~21 | — | — | Soft was useless |
| maybe_wait OCC-lite | `83412fe` | **0** | **~14** | ~3.6 | — | **biggest single wall drop** |
| sub-10 / fb-loop / prevent | `1dddbce`→`59754eb` | **0** | **~13±1** | ~3.7 | — | **plateau** |

**Net:** wall ~36 → ~13 (−~2.7×); SoftWait storms → **0**; detect/avoid no longer the narrative; OCC still ~3.5–4× faster on 597.

---

## 3. What worked (keep)

1. **CC diagnosis:** resolve cost grain, not “missed conflicts” or “not enough Wait.”  
2. **SpecFence-native verbs:** SuffixRepair-first; FullRestart last resort; SoftWait Soft demoted after profile proved it useless.  
3. **Lean resume bugfix (`fbbd321`):** `rewind_resume` must not be `!lean`-gated.  
4. **Sticky force_bind after reabort (`91e8d22`):** cut abort storms toward OCC abort count.  
5. **ESTIMATE hygiene (`169eece`):** don’t poison certified prefix → fewer BlockingOther.  
6. **OrderedDirtyRead / ESTIMATE→SpecRead (`97071cc`):** BO idle ≈0.  
7. **maybe_wait OCC-lite common path (`83412fe`):** Bind-on-Data / cold SpecRead without DashMap π — largest wall win.  
8. **fb-loop escalate (`9836720`):** stop infinite SuffixRepair↔force_bind_reabort chains.  
9. **Measurement:** multi-iter median; park subtype counters; `SPECFENCE_PROFILE` buckets.

---

## 4. What failed or was reverted (don’t repeat)

| Attempt | Why it died |
|---------|-------------|
| Whole-block `SPECFENCE_ENABLE_INSPECT` | Hang on 599 (V5-P3) |
| Live-capture inspect after fb_reabort | Wall↑ + seq≠par |
| SoftWait→SpecRead / SpecRead-through-writer | Abort storms / M3 breakage |
| SoftWait Soft as resolve tool | wake_ok≪reabort; later SoftWait=0 with flat/better wall |
| Fanout/EV Wait dampening as sticky | SoftWait/park storms |
| Validate micro-opts alone | Plateau; residual = resume/reexec count |
| Escalate FullRestart after fb_reabort | Breaks loop but **wall flat** (same EVM bill) |
| Prevent-first BO Await widen | Small resume↓; wall still ~13 |

---

## 5. Why ~13ms plateau (current physics)

After SoftWait=0 and BO≈0, 597 wall is dominated by:

1. **~80–100 first-class aborts → FullRestart/SuffixRepair EVM** (`fb_reabort≈full_restart` after escalate).  
2. **Residual maybe_wait / rem / validate** on non-cold paths (~20–40ms CPU sum).  
3. **Schedule noise** (±1–2ms median across N=7/11).

OCC pays ~3.7ms with similar *abort count order* at times but **much cheaper** discovery path and no SpecFence rem/π tax. SpecFence’s “native resolve” still often equals **pay an extra EVM incarnation** when Bind-no-park guessed wrong.

**Iron law still holds:** labels (ForceBind, resume_count) without fewer interpreter-seconds on the critical path don’t close the OCC gap.

---

## 6. Architectural bets not taken (paused options)

User declined to pick; recorded for later:

| Bet | Thesis | Risk |
|-----|--------|------|
| A. Conflict-tx serial barrier | One serial pass for hot txs → fewer abort/FullRestart | Throughput on parallel-friendly tails |
| B. Hang-free mid-tx absolute jump | SuffixRepair skips prefix opcodes | Hang history; narrow subset only |
| C. Block-level OCC-fast when quiet | Discovery = OCC; SpecFence only on conflict | “OCC++” optics; engagement tripwire design |

---

## 7. Protocol status at pause

Still authoritative: `lab/notes/specfence-native-resolve-protocol.md`

**Runtime identity at `59754eb`:**
- Common path: OCC-lite SpecRead / Bind-on-Data (no SoftWait Soft).  
- Conflict: sticky force_bind, RebindOnly when value-stable, SuffixRepair once then escalate FullRestart.  
- BlockingOther prefer-steal for unfinished writers (not SoftWait Soft).  
- Inspect/absolute jump: not default; SoftWait Soft ≈ 0.

---

## 8. Recommended next (when unpaused)

1. **Don’t grind more micro-opts** on validate/plan/spin — ROI exhausted.  
2. Pick **one** of A/B/C above with a falsifiable 597 median target (&lt;10 then &lt;8).  
3. Or accept ~13ms / SoftWait=0 as interim SpecFence resolve floor on 597 and shift research to other blocks / paper metrics (absolute TPS, abort taxonomy) rather than SF/OCC ratio chasing.

---

## 9. Key note index

- Dig: `specfence-v5-bottleneck-dig-status.md`  
- Detect/avoid/resolve: `specfence-v5-detect-avoid-resolve.md`  
- Native protocol: `specfence-native-resolve-protocol.md`  
- Profile root: `specfence-resolve-profile-strip-status.md`  
- Plateau tips: `specfence-resolve-mw-strip-status.md`, `specfence-resolve-fb-loop-status.md`, `specfence-resolve-prevent-first-status.md`
