# Mainnet exec-process: Unfenced leaks (Fence failures)

**Date:** 2026-09-11  
**Branch:** `cursor/specfence-complete-cc-63b0`  
**Vocabulary:** `specfence-spec-means-region.md` — Spec = Region; Fence = barrier; Unfenced ≠ Spec

---

## Symptom

597 warm (gaps-closed): `avoid=916`, `edge_wait_for=21`, `edge_spec=4117`, `edge_bind=841`.  
Avoid fired. Clique Unfenced count did **not** collapse. Wall stayed ~3× OCC.

The process leak is not “too much optimism as a product.” It is **Unfenced fallthrough where a Fence was required**.

---

## Leak 1 — `force_prefix ∧ writer=None`

Repair / certified-prefix (`force_prefix`) is a known essential Region. π did:

```
must_wait = force_prefix || …
if must_wait {
    if let Some(w) = writer { if w < reader { WaitFor(w) } }
}
→ Unfenced   # writer=None fallthrough
```

`HotSketch::essential_antidep` compounded it: `force_prefix` returned **only** `writer_known`, so `writer=None` was not even marked essential.

**Required:** resolve writer (residual / last_writer / spine). If still none and `reader>0`, **serial-lane Fence** `WaitFor(reader-1)` + `admit_spine` + steal. Not Unfenced.

---

## Leak 2 — `avoid ∧ writer=None`

A2 Avoid means a publisher already touched \(\ell\). Later readers must Fence (WaitFor or Bind). Process instead:

- Avoid counter ++
- `writer` filtered or missing → Unfenced
- metric looked like “Avoid worked” while the access was unfenced

**Required:** Bind if Data; else WaitFor resolved / serial-lane writer. Never Unfenced + avoid as the pair.

---

## Leak 3 — hang-freedom via Unfenced

`1283b1c` Spec’d when the writer was not Executing. Gaps-closed WaitFor’d Ready writers and admitted the spine — correct direction. Residual Unfenced on known essentials (leaks 1–2) is the same class of bug: **using Unfenced as hang-freedom**.

Hang-freedom = `admit_spine` + steal. Not Unfenced on a known essential Region.

---

## Not a leak

- **Bind-on-published-Data** (A3): keep. Writer need not be Validated.
- **Inversion** `w ≥ reader`: do not WaitFor a later writer (preset-order). Unfenced here is not an essential anti-dep.
- **Independence-certified** / **canary**: Unfenced is the protocol verb.
- **Writer already `is_done` without Data**: storage-origin discovery; Fence is satisfied.

---

## Hard bans while fixing

No SoftWait Soft storms. No EV Await doors. No tip-identity Bind gate. No OCC-retry control plane. No Storm morph.
