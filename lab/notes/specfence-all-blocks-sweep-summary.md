# SpecFence all-blocks SF vs OCC@8 sweep

**Date:** 2026-09-13 (Asia/Shanghai, UTC+8)  
**HEAD:** `f74f875` on `cursor/specfence-complete-cc-63b0`  
**Vocab:** Spec = Region; Fence = Bind / WaitFor / serial-lane+admit; Unfenced = optimistic  
**Binary:** `specfence_all_blocks_sweep` (release, LTO off)  
**Hard bans:** SoftWait Soft storms, EV Await doors, tip-identity Bind gate, OCC-retry as π, block-specific hardcodes — **held** (`soft_wait_arms=0`, `await_at_a_arms=0` on every SF row).

## Artifacts

| Path | Role |
|------|------|
| `lab/results/all-blocks-sf-occ-sweep.json` | One row per (block, mode); N=1 @8 for **all** ethereum snapshots + corrected overlay |
| `lab/results/all-blocks-sf-occ-n3-outliers.json` | N=3 @8 validation on worst/anomalous + focus family |
| `lab/results/all-blocks-sf-occ-sweep-corrected-summary.json` | n_tx>0 + N3 overlay distribution |
| `lab/results/all-blocks-process-{bn}.json` | ProcessTrace digests for N=1 worst-10 (pre-correction ranking) |
| `lab/results/all-blocks-sf-occ-sweep.run.log` | Full run log |

```bash
cargo build -p pevm --release --config 'profile.release.lto=false' --example specfence_all_blocks_sweep
SPECFENCE_ALL_ITERS=1 SPECFENCE_ALL_PROCESS_TOP=10 \
  ./target/release/examples/specfence_all_blocks_sweep
```

## Coverage

| Set | n |
|-----|--:|
| ethereum block dirs | **99** |
| loaded (block.json + pre_state) | **99** |
| skipped | **0** |
| SF@8 + OCC@8 ok pairs (N=1) | **99** |
| empty (`n_tx=0`, bn **19910734**) | 1 — excluded from ratio stats |
| analyzed ratios (n_tx>0, N3 overlay on 13 real outliers/focus) | **98** |

Rise snapshots were not primary; ethereum only.

## Honesty on wall

- **Mandatory metrics sweep = N=1** across all 99 (fast coverage).  
- **N=1 noise observed:**  
  - **19434587** OCC wall ~2866 ms once (pathological first sample) → SF/OCC “71×” — **N=3 corrects to ~0.23**.  
  - **2179522** one-shot Bind storm (`edge_bind≈12k`) → false worst; **N=3 SF/OCC ≈ 1.07**.  
  - Empty **19910734** → `sf_occ=0` (no txs).  
- **Headline numbers below use corrected set** (drop empty; overlay N=3 medians where validated). Raw N=1 rows remain in JSON for audit.

## SF vs OCC distribution (corrected, n=98)

| Metric | median | p10 | p90 | mean | geo |
|--------|-------:|----:|----:|-----:|----:|
| **SF/OCC TPS** | **0.356** | 0.244 | 1.161 | 0.534 | 0.428 |
| **wall SF/OCC** | **2.79×** | — | 4.09× | 2.73× | — |
| SF TPS | 20029 | — | — | — | — |
| OCC TPS | 56308 | — | — | — | — |
| SF wall ms | 8.4 | — | 21.3 | — | — |
| OCC wall ms | 3.3 | — | — | — | — |

Focus family (N=3) sits **near the pack**, not uniquely pathological:

| bn | morph (L1 prior) | SF/OCC | wall × |
|---:|------------------|-------:|-------:|
| 14689597 | fan_out | 0.239 | 4.18× |
| 19606599 | long_chain | 0.299 | 3.34× |
| 19469097 | long_chain | 0.296 | 3.37× |
| 19606598 | quiet | 0.361 | 2.77× |

**Universal takeaway vs 597/599/097-only digs:** full-set median SF/OCC **~0.36** (wall **~2.8×**) matches the focus-block story — SpecFence still loses ~3× wall to OCC on typical mainnet morphs; the focus set was representative, not cherry-picked worst.

## Morph clusters (metric heuristic)

Heuristic from SF counters (not full L1 DAG on all 99): `quiet` / `quiet_ish` / `mixed` / `fan_out` / `spine`. Over-labels some L1-quiet blocks as `fan_out` when residual Bind is non-zero — treat as **counter morph**, not gold L1.

| Cluster | n | median SF/OCC | Portrait |
|---------|--:|--------------:|----------|
| **fan_out** | 63 | **0.335** | High `edge_bind`, Rewind/SuffixRepair, often park idle; owns the left tail |
| **quiet** | 26 | **1.125** | Small/early or low-contention; SF ≈ or **beats** OCC (meta cheap when Fence barely fires) |
| **mixed** | 5 | 0.324 | Moderate Bind + low Wait; still ~3–5× wall |
| **quiet_ish** | 3 | 0.275 | Borderline |
| **spine** | 1 | 0.297 | Wait/park heavy |

**Quiet vs fan_out split is the main axis:** quiet blocks already clear SF/OCC≥1 often; parallel failure is concentrated in **fan_out / hot multi-writer** morphs.

## Worst parallel failures (corrected top 10)

| rank | bn | SF/OCC | wall × | n_tx | dominant metric (SF) | notes |
|-----:|---:|-------:|-------:|-----:|----------------------|-------|
| 1 | **19807137** | **0.090** | 11.1× | 712 | SuffixRepair R2 `rewind≈2002` | Global worst; Bind≈9k; rebind=113 still ≪ rewind |
| 2 | **6196166** | 0.155 | 6.4× | 108 | park_idle≈0.93 | WaitFor schedule tax |
| 3 | 14029313 | 0.204 | 4.9× | 724 | R2 rewind (low absolute) | Wide-ish; meta/cold gap |
| 4 | 19434587 | 0.229 | 4.4× | 390 | R2 rewind=360 | N=3; was N=1 OCC spike |
| 5 | 14334629 | 0.234 | 4.3× | 819 | R2 rewind | mixed |
| 6 | 4330482 | 0.235 | 4.3× | 237 | R2 | N=1 only |
| 7 | 19933612 | 0.237 | 4.2× | 130 | park_idle | N=1 |
| 8 | **14689597** | 0.239 | 4.2× | 564 | park_idle≈0.45 | Focus fan_out |
| 9 | 15199017 | 0.240 | 4.2× | 866 | R2 (tiny rewind) | Heuristic quiet; still wall gap |
| 10 | 19860366 | 0.241 | 4.2× | 430 | park_idle | hot |

ProcessTrace on worst family (N=1 digests): `unfenced_writer_done=0`, `unfenced_after_avoid≈0` — residual Bind path holds; wall owned by **R2 SuffixRepair + cold/canary Unfenced + Wait park**, same class as post-subgrain multiblock note.

## Commons vs outliers

**Commons (IQR SF/OCC ≈ 0.29–0.61, n≈50):** mostly heuristic `fan_out`; mean `rewind_to_cp≈56`, mean `edge_bind≈1000`. Soft=0. SF wall typically **3–4×** OCC without catastrophic rewind.

**Outliers left tail:** `19807137` (rewind thousands + Bind thousands), `6196166` (park dominates 8·wall), then a band of hot post-Merge blocks (1943xxxx / 198xxxxx / 597-family) at SF/OCC **0.20–0.25**.

**Outliers right tail:** early/tiny quiet blocks with SF/OCC **>1** (often n_tx≪100). Do **not** treat as CC wins for production TPS — OCC absolute wall is already sub-ms; claim only that Fence tax ≈ 0 when Regions stay cold.

## Updated general TPS/CC conclusions (full set)

1. **Across all 98 loadable non-empty ethereum snapshots, median SpecFence@8 / OCC@8 ≈ 0.36 (wall ≈ 2.8×).** Not a 597-only artifact.  
2. **SoftWait Soft and Await@a stay at 0** on the full set — post-subgrain residual-Bind SoT did not reintroduce banned Soft storms.  
3. **Parallel failure mode is morph-conditional:** quiet → SF competitive; fan_out/hot → SuffixRepair R2 + Bind residual + park idle. RebindOnly remains rare vs Rewind (`rebind ≪ rewind` even when `writer_identity_preserved` is live).  
4. **OCC still sets the TPS ceiling** on contended mainnet blocks; SpecFence’s remaining gap is **repair/schedule makespan**, not writer_done Unfenced leaks.  
5. **N=1 alone is insufficient for ranking extremes** — always N≥3 (or median) before claiming a new global-worst block.

## Ranked next fixes (validated across all blocks)

| Pri | Fix | Why full-set |
|----:|-----|--------------|
| **P0** | **RebindOnly / identity-cheap repair replacing R2 body** on residual Bind | Worst + IQR commons: `rewind_to_cp` dominates; `19807137` rewind≈2k with rebind only ~5% of that |
| **P0** | **WaitFor park → steal/ready width** without SoftWait Soft | `6196166`, `597`, `19860366`: park_idle 0.3–0.9 of 8·wall |
| **P1** | **Cold/canary Unfenced rediscovery tax** cut (keep Avoid→Done∅ closed) | Process reasons: cold+canary still large share of Unfenced after residual Bind |
| **P1** | **Hot fan-out SuffixRepair absorb / fanout_fr_collapse** generalize beyond focus | `19807137` / `19434587` / `14545870` share R2+full_restart pattern |
| **P2** | **Quiet morph: keep Fence off** (already mostly OK) | 26 quiet blocks median SF/OCC>1 — protect this; don’t plant Bind priors that flip quiet→fan_out |
| **P2** | **Metric-morph vs L1 DAG** calibration on full set | Heuristic over-calls fan_out; need cheap L1 morph label for all 99 before morph-gated π |

**Do not:** re-enable SoftWait Soft storms, EV Await doors, tip-identity Bind gate, OCC-retry-as-π, or 597-only hardcodes.

## One-paragraph universal conclusion

Running SpecFence@8 and OCC@8 on every loadable local mainnet ethereum block (99/99, soft=0) shows a **full-set median SF/OCC TPS ≈ 0.36 (≈2.8× wall)**, with quiet morphs often ≥1 and fan_out/hot morphs clustering at 0.20–0.35 — the same regime previously reported on 597/599/097. After N=3 correction of N=1 spikes, the global worst is **19807137 (~0.09)** driven by SuffixRepair R2 rewind and Bind residual, not banned SoftWait paths. The universal CC conclusion: **SpecFence’s remaining gap to OCC is repair/schedule makespan on contended Regions; access-face writer_done leaks are closed; next wins must cheapen residual Bind repair and Wait park across the fan_out majority, while preserving quiet-block parity.**
