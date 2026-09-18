# 3356896 main-chain short-edge + reuse PRIMARY

**Baseline:** PR #21 `cursor/specfence-cc-learn-pc-complete-490a`  
**Constraint:** Soft=0; one pevm spine; no wide empty-to / CallWaw stars.

## Mechanism

### Main-chain OrderedAdmit (Basic(0x32be) / 0x209c writers 4→31→66→…→171)

- A0 execute and abort do **not** insert ReadyEdges (mid-block insert races A0 done-stamp → Estimate leftover / seq≡par).
- After the first EffectiveWAW abort: persist the proven pair on that location.
- End-block: persist consecutive D1 pairs on promoted ℓ, skipping writer sets that match a wide empty-to / CallWaw envelope (so Basic(0x209c) is not stored as a star). `4 ∪ 0x209c` on Basic(0x32be) is kept.
- Thin reuse begin: at most `THIN_A1_K` locations, longest chain first; skip envelope pair-sets.

### Reuse harness

- Default compare keeps one SpecFence `Pevm` across N iters (learned state).
- Report cold iter0 + reuse median (iters 1..N-1).
- PRIMARY: reuse SF median ≤ OCC median.
- `SPECFENCE_COLD_EACH_ITER` restores new-Pevm-per-iter.

### Kept

Storage begin 14→16→17; commute/ignore; Soft=0; indep tax 0; `edge_4_31`.

## Measurement (3356896 @8 Soft=0 N=5)

| | OCC | SF cold (iter0) | SF reuse (1..4) |
|---|---|---|---|
| walls ms | 2.421, 0.882, 0.979, 1.165, 0.858 | 1.151 | 1.396, 1.178, 2.308, 1.382 |
| median | **0.979** | 1.151 | **1.396** |
| unfenced | — | 14 | 0, 0, 0, 1 |
| main_inc | — | 14 tail txs | ∅ |
| storage_inc | — | ∅ | ∅ |
| commute / ignore | — | 77 / 77 | 77 / 77 |
| taxed_begin / soft / edge_4_31 | — | 0 / 0 / true | 0 / 0 / true |

PRIMARY `sf_le_occ`: **false** (reuse median 1.396 > OCC 0.979).

Reuse OrderedAdmit fences the 0x32be spine (`begin_blocked` includes 31→171 + storage 16/17; independents 6–13 / 76–86 stay off the tax list). Cold unfenced stays 14 — mid-execute ReadyEdge insert is unsafe on this spine.

A quieter reuse window on the same host (same binary, earlier run) was reuse walls 1.173 / 1.202 / 1.186 / 1.194 (median 1.194) with unfenced=0 and ready_width=156; still above that run’s OCC median 0.872. The 16-writer WAW spine’s admit prepaid exceeds OCC abort on this block.
