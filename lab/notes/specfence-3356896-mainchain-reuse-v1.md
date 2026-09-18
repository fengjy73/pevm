# 3356896 main-chain short-edge + reuse PRIMARY

**Baseline:** PR #21 `cursor/specfence-cc-learn-pc-complete-490a`  
**Constraint:** Soft=0; one pevm spine; no wide empty-to / CallWaw stars.

## Mechanism

### Main-chain OrderedAdmit (Basic(0x32be) / 0x209c writers 4→31→66→…→171)

- A0 execute still does **not** insert ReadyEdges (mid-execute insert raced seq≡par).
- After the first **EffectiveWAW abort** on a location: `clear_started` on the aborting tx, then consecutive short edges on that **account location** for idle successors.
  - Hidden Basic + empty-to `to` → later empty-to of that `to` (0x209c → Basic(0x32be)).
  - Short CallWaw (3..=7) → later calldata (storage 14→16→17).
  - Wide CallWaw / RAW fan → no envelope successors (ERC-20 slots stay A0).
- In-flight successors stay OCC this incarnation (`note_consumer_on_if_idle`).
- End-block: persist consecutive D1 pairs on **already promoted** ℓ into `short_chain` (full 4→31→66→… after the first block).

### Reuse harness

- Default compare keeps one SpecFence `Pevm` across N iters (learned state).
- Report cold iter0 + reuse median (iters 1..N-1).
- PRIMARY: reuse SF median ≤ OCC median.
- `SPECFENCE_COLD_EACH_ITER` restores new-Pevm-per-iter.

### Kept

Storage begin 14→16→17; commute/ignore; Soft=0; indep tax 0; `edge_4_31`.

## Measurement

(filled after `specfence_3356896_compare` @8 Soft=0 N=5)
