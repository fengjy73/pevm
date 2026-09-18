# 3356896 main-chain short-edge + reuse PRIMARY

**Baseline:** PR #21 `cursor/specfence-cc-learn-pc-complete-490a`  
**Constraint:** Soft=0; one pevm spine; no wide empty-to / CallWaw stars.

## Mechanism

### Main-chain OrderedAdmit (Basic(0x32be) / 0x209c writers 4→31→66→…→171)

- A0 execute and abort still do **not** insert ReadyEdges (mid-block insert races A0 done-stamp → Estimate leftover / seq≡par).
- After the first **EffectiveWAW abort**: persist consecutive pairs on that **account location** (not an envelope star).
  - Hidden Basic + empty-to `to` → later empty-to of that `to` (0x209c → Basic(0x32be)).
  - Short CallWaw (3..=7) → later calldata (storage 14→16→17).
  - Wide CallWaw / RAW fan → no envelope successors (ERC-20 slots stay A0).
- End-block: persist consecutive D1 pairs on **already promoted** ℓ into `short_chain`.
- Next same-`Pevm` begin plants the stored pairs (`4→31→66→…→171`).

### Reuse harness

- Default compare keeps one SpecFence `Pevm` across N iters (learned state).
- Report cold iter0 + reuse median (iters 1..N-1).
- PRIMARY: reuse SF median ≤ OCC median.
- `SPECFENCE_COLD_EACH_ITER` restores new-Pevm-per-iter.

### Kept

Storage begin 14→16→17; commute/ignore; Soft=0; indep tax 0; `edge_4_31`.

## Measurement

(filled after `specfence_3356896_compare` @8 Soft=0 N=5)
