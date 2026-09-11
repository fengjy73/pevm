# SpecFence complete CC architecture (SoT)

**Date:** 2026-09-10 (errata 2026-09-11)  
**Status:** AUTHORITATIVE protocol for this cut — implement end-to-end, not a subset  
**Terminology:** Spec = **Region** (not speculate). Fence = barriers on Regions.
Optimistic access = **Unfenced**. Product name SpecFence stays.
See `specfence-spec-means-region.md`.  
**Family:** preset-order hybrid OCC with ahead-of-time dependency sketch, ordered
chain admission / version-read pipeline, bounded Unfenced, fine-grained
Detect→Avoid→Resolve, event-driven first-wave learning; early-visible MVCC with
piece-restricted abort; work-conserving schedule.

This note is the design SoT reconstructed onto `specfence` when the original
`~f32eed3` file was not present in the checkout. The protocol below is the
complete A1–A6 / D1–D7 contract — not AEC, not Storm/Quiet morph, not OCC-lite
as the control plane.

---

## 0. Hard bans

| Ban | Why |
|-----|-----|
| SoftWait storms | Wake≪reabort; serializes 597 fan-out |
| EV Await doors / AdaptiveParams-as-Await-θ | Makespan EV is a feature, not the verb |
| tip-identity gates | Bind/jump refuse-if-stale is plant hygiene, not π |
| Storm/Quiet morph-as-protocol | Morphology is a prior, not a mode switch that *is* CC |
| OCC-retry / bare Block-STM reincarnation as **control plane** | Discovery only; not contention response |
| Celebrating abort↓/park↓ while ≪OCC | Wall/TPS vs OCC is the bar |
| Fine grain = more knobs | Fine grain = identity + verbs, not extra θ |

Unfenced is **bounded**. Known/predicted essential anti-deps → Bind / WaitFor /
serial-lane admission. **Never** Unfenced+retry as the contention response.
**Never** call Unfenced “Spec” — Spec is the Region.

---

## 1. Mechanisms (A) ↔ roadmap (D)

| Mech | Roadmap | Verb |
|------|---------|------|
| A1 | D1 | Ahead sketch + ordered admission |
| A2 | D2 | First-wave intra learning (immediate Avoid broadcast) |
| A3 | D3 | Early-visible reads (Bind published Data; no writer_done∨Validated gate) |
| A4 | D4 | Spine serialize; independence-certified Unfenced freely |
| A5 | D5 | Fine Detect + Resolve ladder R1→R4 (R4 = failure) |
| A6 | D7 | Inter learning: warm-start H + chain templates; decay on flip / warm≥cold fail |
| — | D6 | Essential wait-for + work-conserving ready queue |

---

## 2. Conflict identity (D5 Detect)

Three layers, not a flat `(ℓ, reader)`:

1. **L_record** — `MemoryLocation` / location hash (conflict object).
2. **L_access** — `(t, k, depth)` inside the tx (multi-touch / frames).
3. **L_edge** — typed `wr | rw | ww` with state
   `unpublished | published-uncommitted | validated`.

`EdgeTable` key is `(ℓ, reader, k, depth)` (plus writer + kind). Flattening to
`(ℓ, reader)` is illegal when the same reader touches ℓ twice or across frames.

---

## 3. Control-plane π: `choose_edge_action`

Not AEC argmin EV. Not Storm Await@a. One function:

```
if published Data exists (incl. Executed-not-Validated tip):
    Bind that version                          # A3 — drop writer_done∨Validated
elif essential unpublished anti-dep
     (H ∪ Avoid-broadcast ∪ predicted chain ∪ force_prefix)
     and writer w < reader:
    WaitFor(writer)                            # D6 — BO + steal, not SoftWait Soft
elif essential / Avoid / force_prefix and writer = None and reader > 0:
    WaitFor(reader-1) + admit_spine            # serial-lane Fence, not Unfenced
elif independence-certified (ℓ ∉ H, no Avoid, no predicted edge):
    Unfenced                                   # A4 — not Spec
elif canary grant still open (first-wave probe, no Avoid yet):
    Unfenced                                   # A2 discovery
elif ℓ ∈ H / clique and writer known unpublished:
    WaitFor(writer)                            # A1 mass-Unfenced gate
else:
    Unfenced                                   # cold discovery only
```

Unfenced is **not** the response to a known essential anti-dep.
`force_prefix ∧ writer=None` and `avoid ∧ writer=None` must Fence.
Hang-freedom is `admit_spine` + steal, not Unfenced.

**Errata:** earlier text said “SpecRead” / “Spec freely” as if Spec = optimism.
That is wrong. Spec = Region. The optimistic verb is Unfenced.

---

## 4. Ahead sketch + admission (A1/D1)

- Maintain hot set **H** (inter top-ℓ + live first-wave).
- **Chain template** per hot ℓ: predicted wr spine (order = preset tx index).
- Admit work along the spine: later readers of unpublished essential ℓ WaitFor
  the known writer; do not mass-Unfenced the clique.
- Independence-certified txs (no predicted essential edge) Unfenced freely.

---

## 5. First-wave intra (A2/D2)

- One canary Unfenced per hot ℓ before anyone has published.
- On **first confirmed wr/publish** on hot ℓ: **immediately broadcast Avoid**
  for subsequent similar edges (same ℓ, later readers).
- Progressive DAG wake on publish — not block-end EMA only.

---

## 6. Early-visible + piece-restricted abort (A3/D3)

- If MV `last_data_before` returns Data, **install/Bind** it. Writer may still
  be un-Validated. Visibility is safe because abort is piece-restricted:
  R1 rebind / R2 suffix / R3 cascade on the failed access, not whole-tx OCC
  reincarnation as the plan.
- Unfenced only when **no** readable version exists and the Region is not essential.

---

## 7. Resolve ladder (A5/D5)

| Rank | Name | Role |
|------|------|------|
| R1 | value-stable rebind | Same-output origin patch; no interpreter |
| R2 | piece / frame / suffix | Real opcode skip (Bind-snap / RewindTo / FF) |
| R3 | essential cascade | Selective invalidate of dependent edges only |
| R4 | full abort | **Failure mode**, not control plane |

---

## 8. Inter learning (A6/D7)

- Warm-start H + chain-template confidence from the previous block.
- On morphology flip or warm≥cold fail (high abort on warm-seeded ℓ): **decay**
  confidence; do not copy Wait bitmaps; do not flip a Storm/Quiet protocol.

---

## 9. Success bar

Wall / TPS / abort / park vs OCC on continuous 597 / 599 / 097 families.
Honest if 大幅 (SF/OCC → 1) did not move. SoftWait Soft stays ~0.
