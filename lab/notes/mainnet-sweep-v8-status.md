# Mainnet sweep v8 — Adaptive CC R3 (HotSet tune + measure)

**Date:** 2026-09-06 (Asia/Shanghai)  
**Branch:** `specfence`  
**Base:** Adaptive CC R0–R2 (`a855e14`) + R3 HotSet / escalate / lazy-recipient fixes  
**Spec:** `lab/notes/specfence-adaptive-cc-redesign-v1.md` §4–5 Phase R3

```
cargo run -p pevm --release --config 'profile.release.lto=false' --example specfence_mainnet_sweep -- \
  --blocks 19807137,14683600,13217637,14383540,15199017,14029313,19434587 \
  --cores 1,2,4,8 --repeats 3 --modes occ,specfence \
  --out lab/results/mainnet-sweep-v8.json
```

## Hang status

| Check | Result |
|-------|--------|
| SpecFence@8 all 7 blocks | **PASS** — 168/168 rows `ok=true` |
| **19434587 SF@8** | **PASS** (3/3 completes, ~20 ms, was hung every attempt in v7) |
| inspector_steps | **0** on all SpecFence rows (inspect still off) |

## Headline SF/OCC @8 (mean of 3 repeats)

| block | SF@8 TPS | OCC@8 TPS | SF/OCC v8 | SF/OCC v7 | SF/OCC v5 | hotset | wait_hard | lean_mode_txs | notes |
|-------|----------|-----------|-----------|-----------|-----------|--------|-----------|---------------|-------|
| 19807137 | 9828 | 51025 | **0.193** | 0.085 | 0.118 | 1056 | 1952 | 3623 | hot; HotLocal live; no hang |
| 14683600 | 42435 | 101696 | **0.417** | 0.135 | 0.477 | 39 | 82 | 924 | mixed |
| 13217637 | 82419 | 281032 | **0.293** | 0.101 | 0.214 | **2** | **0** | 1135 | wide |
| 14383540 | 52308 | 163402 | **0.320** | 0.123 | 0.302 | 9 | 12 | 817 | wide-ish |
| 15199017 | 69416 | 240068 | **0.289** | 0.067 | 0.309 | **5** | **0** | 887 | wide |
| 14029313 | 64709 | 267585 | **0.242** | 0.045 | 0.296 | 3 | 7 | 919 | wide-ish |
| 19434587 | 18648 | 35963 | **0.519** | 0.044* | 0.366 | 602 | 294 | 917 | *v7 SF@4 proxy |

- **Mean SF/OCC @8:** **0.325** (geo **0.310**)
- v7 mean (excl. hung proxy): **~0.092** → v8 is **~3.5×** better
- v5 mean: **~0.297** → v8 ≈ back to v5 wall-clock ratio (inspect tax removed; remaining meta gap)

## R3 HotSet tune (what changed)

| Knob | R1 default | R3 | Why |
|------|------------|----|-----|
| `H_w` | 3 | **8** | Wide G* max_writers≈5–7; avoid mild multi-writer Bind |
| `H_a` | 2 | **3** | Cut abort-noise inserts |
| Process prior seed | begin_block pre-seed @0.35 | **no begin pre-seed**; sticky on write @0.80 after sustained heat + unseen decay | Hot→wide cross-block pollution |
| LazyRecipient | counted in H_w | **excluded from H_w** | Fake multi-writer chains; G* drops basic_lazy |
| Abort escalate | 1/1 ≥0.08 force-insert write-set | **min 4 aborts + 32 started + rate≥0.08** | First-abort Bind tax on wide |
| Abort `note_abort` | conflict locs + full write-set | **conflict locs only** | Same |

## Gates (redesign §4)

| Gate | Status |
|------|--------|
| SpecFence@8 completes all 7 (no inspect livelock) | **PASS** |
| Wide: lean/n ≥0.95, wait_hard≈0 | **PASS** (lean≈all incarnations; wait_hard=0 on 15199017/13217637) |
| Wide: hotset≈0 or tiny | **PASS** (2–5 on target wide blocks) |
| Wide: SF/OCC@8 ≥0.95 | **MISS** (~0.29) |
| Hot: evm_entries_sf ≤1.05×OCC | **PASS** (19807137: 3623/4452 ≈ **0.81×**) |
| Hot: SF/OCC ≥1.0 on ≥1 hot | **MISS** (0.19) |
| inspector_steps=0 default | **PASS** |

## Honest remaining gap (SF/OCC still ≪ 1)

With inspect off, wait_hard≈0 on wide, and hotset tiny, **evm_entries SF ≈ OCC** on wide blocks — the ~3× wall-clock gap is **not** interpreter reexec. Left:

1. **SpecFence meta vs Block-STM OCC** — per-read `HotSet::contains` (DashMap), SpecRead metrics, region/Bayes bookkeeping even on cold path.
2. **Bind tax on residual HotSet** — even hotset=2–5 yields some `prior_bind`/`bind_hits` (popular storage); HotLocal π still heavier than OCC OrderedDirtyRead.
3. **Hot blocks** — WaitHard+park + Bind cut `evm_entries` below OCC but schedule/meta still loses wall-clock; head-of-chain reexec / steal not enough to beat OCC TPS.
4. **Not claimed this round:** location-grain steal, or re-enabling inspect/jump.

## Artifacts

- JSON/CSV: `lab/results/mainnet-sweep-v8.{json,csv}` (168 rows)
- Summary: `lab/results/mainnet-sweep-v8-summary.json`
- Log: `lab/notes/mainnet-sweep-v8.log`
- Smokes: `lab/results/mainnet-sweep-v8-r3-smoke*.json`
