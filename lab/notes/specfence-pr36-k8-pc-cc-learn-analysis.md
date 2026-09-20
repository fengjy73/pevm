# PR #36 slowest 8: PC / CC / learn residual (professional terms)

**Baseline:** PR #36 tip `c0638440813484b1f55abe4351383a4e8c9f8110` · Soft=0 · OptimisticRead / OrderedAdmit  
**This land:** mid-band over-admission + under-covered conflict spine + terminology.  
**PC / CC are analysis lenses, not split modules.** Instant-off is the PRIMARY wall.

## Residual after PR36

Lazy-update 4–25× large-block tail is gone. Corpus Soft=0: `n≥512` and SF/OCC≥4× went from 5 blocks to **0**; max ratio 27.45× → **3.61×** (`14396881`).

Remaining slowest is not “the same lazy-update tail”:

1. **Under-covered conflict spine is still the wall champion.** `19807137` Instant-off reuse ~4.1×. Storage 571 writers cannot be covered; unfenced stays 246–595. Ungated OCC task selection is already thousands; the loss is Detect / arm width, not the issue switch.
2. **Mid-band real-spine Detect + n<512 over-admission OrderedAdmit.** Absolute-wall top outside the under-covered spine is n=341–430 mixed spines: `19434587` / `19606599` / `19716145` / `19860366`. Ungated OCC selection is hundreds, but wait-set 49–108 (the n≥512 cap missed them) and `end_block` 2.1–4.3 ms sit on the wall.
3. **Lazy-update leftover is 2.7–3.5× scheduler/validate/end-block overhead**, not a 25× mode switch. `14396881` still leads on ratio; OCC abort is single-digit; ungated OCC selection is hundreds–thousands.

## Next cut (this package)

1. Wait-set soft-cap by **predicate** (wait-set size / cover_window inflation), covering `19716145` (108) / `19860366` (76) / `19434587` (50).
2. `end_block` lean from large+lazy-seen to mid-band stable D1.
3. Under-covered conflict spine: no Full/Seg hard-order; ĉ may prefer OptimisticRead / OCC abort; stop learning-uphill on 19807137-class.
4. Keep PR36: lazy-update chain never OrderedAdmit; ungated OCC task selection; Done-on-success; Soft=0.

Do not: widen OrderedAdmit to chase 3356896; re-admit a 95-wide wait-set for `15274915` unfenced; treat SF≤OCC **count** as the north star.
