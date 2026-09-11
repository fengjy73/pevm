# Fence-cover hot Region (process evidence)

**Date:** 2026-09-11  
**PR:** https://github.com/fengjy73/pevm/pull/3  
**Branch:** `cursor/specfence-complete-cc-63b0`  
**SoT:** `specfence-spec-means-region.md` — Spec = Region; Fence = barrier; Unfenced ≠ Spec  
**JSON:**
- `lab/results/exec-process-597-fence-cover.json` (597 @8, last SF iter)
- `lab/results/exec-process-597-fence-cover-warm.json` (xblock 597 warm)
- `lab/results/fence-cover-xblock-{sf-occ,flip,xblock}.json`

---

## Verdict

**Hot fan-out Region is Fenced.** On 597 @8 the star ℓ `85335018835337005`:

| | cold SF (process) | warm SF (process) |
|--|------------------:|------------------:|
| Unfenced (canary / first-wave) | **1** | **2** |
| Bind after Avoid | **643** | **581** |
| WaitFor | 0 | 0 |
| Unfenced-after-Avoid | **0** | **0** |

Timeline (cold): canary seq=5 → Avoid seq=30 → first Fence/Bind seq=57. After the first Avoid/publish, every later access on that ℓ is **Bind** (ReadPublished). Unfenced-after-Fence ≈ 0 on the clique.

Independent / handler chatter stays Unfenced (expected). Block-level Unfenced is still large because it is **not** the star.

Wall / TPS vs OCC did **not** improve (secondary). SoftWait Soft = 0. Await@a = 0.

---

## Diagnosed Unfenced predicates (597 @8 after rename)

Reason histogram (cold SF process):

| Reason | Count | Class |
|--------|------:|-------|
| `unfenced_cold` | 2157 | first-wave, writer unknown, not independence |
| `unfenced_independence` | 1251 | A4 certified — allowed |
| `unfenced_writer_done` | 775 | WaitFor target done, no Data, no live spine writer |
| `unfenced_canary` | 714 | A2 one-probe |
| `unfenced_after_avoid` | 265 | **not on the star** (other Avoid ℓ, ESTIMATE / done writer) |
| `bind_published` | 826 | Fence |
| `wait_for_serial` | 36 | Fence (Avoid/repair, no resolved writer) |
| `wait_for_writer` | 8 | Fence |
| inversion / plant TLS | 0 | |

**Leaks that were producing Unfenced on Regions that should Fence:**

1. **Independence short-circuit of the clique gate.** `must_fence` required `clique_gated && !independence`. Cold clique: `!H && !Avoid` ⇒ independence true ⇒ gate ignored even after a writer was known. **Fixed:** clique gate is not independence-shortable when a writer is known.

2. **WaitFor → Unfenced hang-freedom.** `is_done(w)` converted a Fence to Unfenced even when Data had landed or another unfinished spine writer existed. **Fixed:** re-Bind `last_data_before`; else WaitFor `next_unfinished_writer_before`. SoftWait Soft stays 0.

3. **Canary required `in_h`.** First-wave program readers never consumed the canary, so H/clique never formed from live traffic. **Fixed:** program ℓ canary does not require H; `note_hot_touch` on every program access.

4. **`note_hot_touch` only if already hotset/prior/sticky.** Live fan-out never accumulated on a cold star. **Fixed.**

**Not applied (seq≠par on ERC-20 cluster):** serial-lane `WaitFor(reader-1)+admit_spine` on every forming clique with `writer=None`. That serialized non-writers and broke committed state. Pre-publish first-wave without a visible writer **stays Unfenced** (`unfenced_cold`). Hang-freedom is still admission+steal, not Unfenced on a *known* essential (Avoid / force_prefix / live writer).

---

## Hot ℓ vs independent split (597 cold)

| Set | Unfenced | WaitFor | Bind | After Avoid |
|-----|--------:|--------:|-----:|-------------|
| **Star fan-out ℓ** | 1 (canary) | 0 | 643 | Bind 643, Unfenced 0 |
| Other published ℓ (examples 43/43/35 Bind) | 1 canary each | 0 | Bind | Unfenced-after-Avoid 0 |
| Independence chatter (ℓ `9455…`) | 658 | 0 | 0 | no Avoid |
| Post-canary cold (no writer, several ℓ @196–599) | canary+cold+writer_done | 0 | 0 | no Avoid |

Block totals: Unfenced 5162 / Wait 44 / Bind 826. Wait+Bind absorbed the star; remaining Unfenced is independence + first-wave without a writer.

---

## π (this cut)

```
Bind if published Data                         # Fence (A3)
WaitFor(w) if must_fence ∧ w < reader          # clique gate not indep-shortable
WaitFor(reader-1)+admit_spine
  if (force_prefix | Avoid | essential) ∧ writer=None ∧ reader>0
Unfenced                                       # indep / canary / cold first-wave
```

`fence_wait_for`: Data → Bind; unfinished spine writer → WaitFor; else Unfenced only as storage-origin (`writer_done` / residual Avoid-without-Data).

---

## Tests

| Suite | Result |
|-------|--------|
| `cargo test -p pevm --lib` | **124 passed** |
| `cargo test -p pevm --test specfence -- --test-threads=1` | **40 passed**, 20 ignored |
| new: `fence_cover_hot_region_after_canary` | seq≡par, Soft=0, process Bind/Wait |
| `clique_gate_not_shorted_by_independence` | pass |
| `canary_without_h_serializes_second_reader` | pass |

---

## Wall / TPS honesty vs OCC (N=3 @8, this host)

This host is slower than the rename-cut host (OCC 597 median **13.3 ms** vs rename **6.6**). Compare **ratios**, not absolute TPS.

| Block | SF wall med | OCC wall med | SF/OCC TPS | SF abort med | OCC abort med | Soft |
|------:|------------:|-------------:|-----------:|-------------:|--------------:|-----:|
| **14689597** | **63.5** | **13.3** | **0.305** | **162** | **209** | 0 |
| **19606599** | **141.1** | **27.4** | **0.196** | **100** | **73** | 0 |
| **19469097** | **49.7** | **14.1** | **0.379** | **66** | **34** | 0 |
| **19606598** | **8.2** | **6.3** | **0.190** | **2** | **4** | 0 |

Mean SF/OCC = **0.268**. Rename-cut mean on a faster host was 0.353. **Not a win.** 597 SF p90 120 ms — park tax from more WaitFor on non-star Avoid. Do not celebrate abort↓.

### 597 xblock (family ran; 598 sf-cold hung)

| | rename warm | this warm | this cold | OCC |
|--|----------:|----------:|----------:|----:|
| wall_ms | 17.9 | **56.1** | **44.9** | **19.0** |
| edge_bind | 765 | **793** | — | — |
| edge_wait_for | 49 | **92** | — | — |
| edge_unfenced | 3726 | **4467** | — | — |

xblock after 597 OCC: **598 sf-cold spun ~5 min at 100% CPU (1 thread)** — killed. 599/097 families not run. Residual: more serial WaitFor+`admit_spine` on Avoid ℓ can starve the ready queue on quiet follow-on blocks. Not SoftWait (Soft=0).

---

## Hard bans

| Ban | This cut |
|-----|----------|
| SoftWait storms | Soft=0 |
| EV Await / AdaptiveParams-as-θ | Await@a=0 |
| tip-identity as Bind door | Bind on Data |
| OCC-retry as control plane | no |
| Storm morph as π | no |
| serial-all clique without writer | **rejected** (seq≠par) |

---

## Residual

1. **Wall ≪ OCC** — Fence cover on the star is a process win, not a makespan win.
2. **265 Unfenced-after-Avoid** globally — not the star; ESTIMATE/done-writer storage-origin on other Avoid ℓ.
3. **Pre-publish mass Unfenced** on locs with no visible writer (`unfenced_cold`) — serial-all is unsafe.
4. **xblock 598 hang** — investigate `admit_spine` + Avoid serial lane on quiet blocks.
5. Process `hot_fanout_l` ranking is post-Avoid Bind/Wait (independence chatter must not win).
