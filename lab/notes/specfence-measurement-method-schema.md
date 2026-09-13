# SpecFence measurement method schema (frozen)

**Date:** 2026-09-07 (Asia/Shanghai)  
**Status:** FROZEN in code as `MeasurementMethod` (`crates/pevm/src/specfence/finegrain.rs`)  
**Authority:** `lab/notes/specfence-historical-block-instrumentation-methodology.md` §4

Every finegrain collector export embeds a `method` object with these fields.

## Frozen fields

| Field | Value | Meaning |
|-------|-------|---------|
| `location` | `"MemoryLocation"` | pevm `MemoryLocation` (Basic / Storage / CodeHash / …) |
| `warm_policy` | `"emit_warm_true"` | Warm journal re-reads of the same ℓ within an incarnation **are emitted** with `warm=true` |
| `primary_depth` | `"gross_work"` | Primary depth identity |
| `primary_depth_formula` | `"gas_used_so_far/tx_gas_used"` | Never gas/limit as primary |
| `raw_instance` | `true` | No `(p,c,ℓ)` dedupe on RAW edges |
| `excluded_from_effective_gstar` | `["coinbase_beneficiary","basic_lazy"]` | Dropped from effective DAG / filter_effect_edges |
| `producer_status_sampled_at` | `"discovering_incarnation"` | Not final-success-only |
| `account_grain` | `"diagnostic_only"` | Never primary Wait key |
| `schema_version` | `"l1l2-v1"` | Bump when semantics change |

## Warm policy rationale

Chose **`emit_warm_true`** over `omit_warm` so instance RAW counts stay comparable to interpreter SLOAD/BALANCE/EXT* traces, while analysis can filter `warm=false` for cold-miss-only graphs.

## L1 effect log

Append-only `(tx, effect_k, op_class, location, R|W, gas_used_so_far, opcode_steps, call_depth, warm)` via `FineGrainSnapshot.effect_log` when `set_finegrain_journal(true)`.

## L2 observe fields

Same as L1 observe on RAW edges, plus:

- `producer_status` ∈ `{Data, Estimate, Running, Absent}` (canonical)
- `producer_ready` / `producer_mv` (fine-grain labels, backward compatible)
- `ready_for_bind` = (`producer_status == Data`)
- `consumer_incarnation`

Sampled at the discovering incarnation on OCC@1 and OCC@8 under the same schema.

## Code entry points

- `MeasurementMethod::frozen()`
- `FineGrainSnapshot.method` / `.effect_log`
- `l1_dag_summary(&snap)`
- `producer_status_canonical(ready, mv)`
