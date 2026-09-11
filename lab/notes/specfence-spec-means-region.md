# Spec = Region (not speculate)

**Date:** 2026-09-11  
**Status:** AUTHORITATIVE terminology for SpecFence on this lineage  
**Product name:** SpecFence **stays**. Spec does **not** mean speculate.

---

## Vocabulary

| Word | Means | Does **not** mean |
|------|--------|-------------------|
| **Spec** | **Region** — the concurrency object | speculate / OCC dirty-read / optimism |
| **Region** | SoT identity: \(\ell\) + access \((k,\mathrm{depth})\) + typed edge | hollow `RegionMode::Speculate\|Wait` |
| **Fence** | Barrier **on a Region**: WaitFor, Bind / ReadPublished, chain admission, publish-wake | SoftWait Soft, EV Await door, tip-identity |
| **Unfenced** | Optimistic access with **no** Region barrier (independence / canary / cold) | Spec |

Protocol verb on the Avoid path: **`Unfenced`** (not `SpecRead`, not `Spec`).  
Metric: **`edge_unfenced`** (was `edge_spec`).

```
SpecFence  =  Regions  +  Fences
              (Spec)      (barriers)
```

Optimistic unfenced access is **not** Spec. Calling it Spec collapsed the product name into OCC-retry and made Avoid look like “speculate harder.”

---

## Region (SoT)

Conflict / learn / fence attachment is **not** a flat `(ℓ, reader)` and **not** `RegionMode`:

1. **L_record** — `MemoryLocation` / location hash \(\ell\)
2. **L_access** — `(t, k, depth)` inside the tx
3. **L_edge** — typed `wr | rw | ww` with `unpublished | published-uncommitted | validated`

`EdgeKey` **is** the Region access. Hollow `RegionMode::Speculate|Wait` is PCC/legacy mirror only — **not** the Avoid protocol face. Do not read `RegionMode` to decide Unfenced vs Fence.

---

## Fence (barriers on Regions)

| Fence | When |
|-------|------|
| **Bind / ReadPublished** | MV Data exists (incl. Executed-not-Validated). Install that version. |
| **WaitFor(writer)** | Unpublished essential anti-dep; writer identity known and `w < reader`. |
| **Chain admission** | Essential Fence required but writer not yet resolved → serial-lane pred + `admit_spine` + steal. |
| **Publish-wake** | Data publish wakes hard waiters. SoftWait Soft = 0. |

Hang-freedom is **admission + steal**, not Unfenced on a known essential.

---

## π (`choose_edge_action`)

```
if published Data exists:
    Bind                                      # Fence
elif essential unpublished (H ∪ Avoid ∪ chain ∪ force_prefix)
     and writer w < reader:
    WaitFor(w)                                # Fence
elif essential / Avoid / force_prefix
     and writer = None and reader > 0:
    WaitFor(reader-1) + admit_spine           # Fence (serial lane)
elif independence-certified:
    Unfenced                                  # not Spec
elif canary grant still open:
    Unfenced                                  # first-wave discovery
else:
    Unfenced                                  # cold discovery only
```

**Never** Unfenced as the response to `force_prefix` or Avoid when a Fence can be produced.

---

## Hard bans (unchanged)

SoftWait storms · EV Await doors · tip-identity as Bind gate · OCC-retry as control plane · Storm/Quiet morph-as-protocol.

---

## Errata

Older notes (complete-CC architecture first cut, three-pillar, AEC) write “Spec = optimism / SpecRead.” That is **wrong** on this lineage. Spec = Region. The optimistic verb is **Unfenced**.
