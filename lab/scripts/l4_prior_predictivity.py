#!/usr/bin/env python3
"""L4 contiguous prior predictivity on segments A/B/C.

Feature vector per block; test whether block t-1 predicts block t better than a global prior.
"""
from __future__ import annotations

import json
import math
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
RESULTS = ROOT / "lab" / "results"
NOTES = ROOT / "lab" / "notes"

SEG_A = [14689595, 14689596, 14689597, 14689598, 14689599]
SEG_B = [19606597, 19606598, 19606599]  # 19606600 missing historically
SEG_C = [19469096, 19469097, 19469098, 19469099]

FEATURE_KEYS = [
    "n_tx",
    "program_frac",
    "handler_frac",
    "fanout_max",
    "raw_path",
    "waw_path",
    "gw_p10",
    "gw_p50",
    "gw_p90",
    "abort_at_8",
    "reexec_frac",
    "indep_frac",
]


def load_from_contiguous_aggregate() -> dict[int, dict]:
    path = RESULTS / "contiguous-segments-finegrain.json"
    out = {}
    if not path.exists():
        return out
    d = json.loads(path.read_text())
    for b in d.get("blocks") or []:
        bn = int(b["block"])
        dag = b.get("dag") or {}
        raw = b.get("raw") or {}
        n_raw = raw.get("n_raw_total") or raw.get("n_raw") or dag.get("n_raw") or 0
        n_prog = raw.get("n_raw_program") or raw.get("n_program") or 0
        n_hand = raw.get("n_raw_handler") or raw.get("n_handler") or max(n_raw - n_prog, 0)
        occ_list = b.get("occ") or []
        occ8 = {}
        if isinstance(occ_list, list):
            for o in occ_list:
                if isinstance(o, dict) and o.get("cores") == 8:
                    occ8 = o
                    break
            if not occ8 and occ_list and isinstance(occ_list[0], dict):
                occ8 = occ_list[0]
        elif isinstance(occ_list, dict):
            occ8 = occ_list
        out[bn] = {
            "block": bn,
            "source": "contiguous-segments-finegrain.json",
            "n_tx": float(b.get("n_tx") or 1),
            "program_frac": float(n_prog) / max(n_raw, 1),
            "handler_frac": float(n_hand) / max(n_raw, 1),
            "fanout_max": float(raw.get("max_fanout") or dag.get("max_writers_on_loc") or 0),
            "raw_path": float(dag.get("longest_chain") or 0),
            "waw_path": float(dag.get("n_waw") or 0),
            "gw_p10": 0.0,
            "gw_p50": 0.0,
            "gw_p90": 0.0,
            "abort_at_8": float(occ8.get("abort_rate") or 0),
            "reexec_frac": float(occ8.get("reexec_entry_frac") or 0),
            "indep_frac": float(dag.get("independent_frac") or 0),
        }
    return out


def load_features(bn: int) -> dict | None:
    # Prefer contiguous-segments / deeper / journal / finegrain
    candidates = [
        RESULTS / f"effect-raw-deeper-b{bn}.json",
        RESULTS / f"effect-raw-journal-stream-b{bn}.json",
        RESULTS / f"effect-raw-deep-b{bn}.json",
        RESULTS / f"contiguous-segments-finegrain-b{bn}.json",
        RESULTS / f"l1l2-b{bn}.json",
    ]
    for path in candidates:
        if not path.exists():
            continue
        d = json.loads(path.read_text())
        if "summary" in d:
            s = d["summary"]
            e = s.get("effect") or s.get("effect_occ1") or {}
            e8 = s.get("effect_occ8") or {}
            occ8 = s.get("occ8") or s.get("occ_8") or {}
            n_raw = e.get("n_raw_effect_total") or e.get("n_raw") or 0
            n_prog = e.get("n_raw_effect_program") or 0
            n_hand = e.get("n_raw_effect_handler") or max(n_raw - n_prog, 0)
            n_tx = s.get("n_tx") or d.get("n_tx") or 1
            dag_indep = None
            # contiguous finegrain may have dag stats elsewhere
            return {
                "block": bn,
                "source": path.name,
                "n_tx": float(n_tx),
                "program_frac": float(n_prog) / max(n_raw, 1),
                "handler_frac": float(n_hand) / max(n_raw, 1),
                "fanout_max": float(e.get("max_program_fanout") or 0),
                "raw_path": float(e.get("longest_effect_program_path") or e.get("longest_final_rw_chain") or 0),
                "waw_path": float(e.get("waw_pairs") or e8.get("waw_pairs") or 0),
                "gw_p10": float(e.get("gross_work_depth_p10") or 0),
                "gw_p50": float(e.get("gross_work_depth_p50") or 0),
                "gw_p90": float(e.get("gross_work_depth_p90") or 0),
                "abort_at_8": float(occ8.get("abort_rate") or 0),
                "reexec_frac": float(occ8.get("reexec_entry_frac") or 0),
                "indep_frac": float(dag_indep or e.get("independent_frac") or 0),
            }
        if "l1" in d:
            l1 = d.get("l1") or {}
            dag = l1.get("dag") or {}
            l2 = d.get("l2_occ1") or {}
            l28 = d.get("l2_occ8") or {}
            n_raw = dag.get("n_raw_instances") or l2.get("n_raw") or 0
            n_prog = l2.get("n_program") or 0
            n_hand = l2.get("n_handler") or 0
            return {
                "block": bn,
                "source": path.name,
                "n_tx": float(d.get("n_tx") or 1),
                "program_frac": float(n_prog) / max(n_raw, 1),
                "handler_frac": float(n_hand) / max(n_raw, 1),
                "fanout_max": float(dag.get("max_program_fanout") or 0),
                "raw_path": float(dag.get("program_chain_length") or 0),
                "waw_path": float(dag.get("n_waw_pairs") or 0),
                "gw_p10": 0.0,
                "gw_p50": 0.0,
                "gw_p90": 0.0,
                "abort_at_8": float((l28.get("timing") or {}).get("occ_aborts") or 0)
                / max(float(d.get("n_tx") or 1), 1),
                "reexec_frac": 0.0,
                "indep_frac": 0.0,
            }
        # contiguous finegrain root shape
        if "block" in d or "n_tx" in d:
            e = d.get("effect") or d
            n_raw = e.get("n_raw_effect_total") or e.get("n_raw") or d.get("n_raw") or 0
            n_prog = e.get("n_raw_effect_program") or d.get("n_program_raw") or 0
            return {
                "block": bn,
                "source": path.name,
                "n_tx": float(d.get("n_tx") or 1),
                "program_frac": float(n_prog) / max(n_raw, 1),
                "handler_frac": 1.0 - (float(n_prog) / max(n_raw, 1)),
                "fanout_max": float(e.get("max_program_fanout") or d.get("max_fanout") or 0),
                "raw_path": float(e.get("longest_effect_program_path") or d.get("longest_chain") or 0),
                "waw_path": float(d.get("n_waw") or 0),
                "gw_p10": float(e.get("gross_work_depth_p10") or 0),
                "gw_p50": float(e.get("gross_work_depth_p50") or 0),
                "gw_p90": float(e.get("gross_work_depth_p90") or 0),
                "abort_at_8": float(d.get("abort_rate") or 0),
                "reexec_frac": float(d.get("reexec_frac") or 0),
                "indep_frac": float(d.get("independent_frac") or 0),
            }
    return None


def vec(f: dict) -> list[float]:
    return [float(f[k]) for k in FEATURE_KEYS]


def l2(a: list[float], b: list[float]) -> float:
    return math.sqrt(sum((x - y) ** 2 for x, y in zip(a, b)))


def mean_vec(rows: list[dict]) -> list[float]:
    if not rows:
        return [0.0] * len(FEATURE_KEYS)
    acc = [0.0] * len(FEATURE_KEYS)
    for r in rows:
        v = vec(r)
        for i, x in enumerate(v):
            acc[i] += x
    n = len(rows)
    return [x / n for x in acc]


def evaluate_segment(name: str, blocks: list[int], feats: dict[int, dict]) -> dict:
    present = [b for b in blocks if b in feats]
    pairs = []
    for i in range(1, len(present)):
        prev, cur = present[i - 1], present[i]
        # only adjacent in the original segment numbering
        if blocks.index(cur) != blocks.index(prev) + 1:
            # allow if they are consecutive in present list but check numeric adjacency
            if cur - prev > 2:
                continue
        pairs.append((prev, cur))
    # stricter: numeric adjacency ±1
    pairs = []
    for i in range(len(present)):
        for j in range(i + 1, len(present)):
            if present[j] - present[i] == 1:
                pairs.append((present[i], present[j]))
    global_prior = mean_vec([feats[b] for b in present])
    adj_errs = []
    glob_errs = []
    details = []
    for p, c in pairs:
        e_adj = l2(vec(feats[p]), vec(feats[c]))
        e_glob = l2(global_prior, vec(feats[c]))
        adj_errs.append(e_adj)
        glob_errs.append(e_glob)
        details.append(
            {
                "t_minus_1": p,
                "t": c,
                "err_prior_tm1": e_adj,
                "err_global_prior": e_glob,
                "tm1_better": e_adj < e_glob,
            }
        )
    mean_adj = sum(adj_errs) / len(adj_errs) if adj_errs else None
    mean_glob = sum(glob_errs) / len(glob_errs) if glob_errs else None
    better = (
        sum(1 for d in details if d["tm1_better"]) / len(details) if details else 0.0
    )
    return {
        "segment": name,
        "blocks_present": present,
        "blocks_missing": [b for b in blocks if b not in feats],
        "n_adjacent_pairs": len(pairs),
        "mean_err_tm1": mean_adj,
        "mean_err_global": mean_glob,
        "tm1_better_frac": better,
        "tm1_predicts_better": (mean_adj is not None and mean_glob is not None and mean_adj < mean_glob),
        "pairs": details,
    }


def main():
    all_blocks = sorted(set(SEG_A + SEG_B + SEG_C))
    feats = load_from_contiguous_aggregate()
    for bn in all_blocks:
        f = load_features(bn)
        if f:
            # Prefer richer deeper/journal when available (overwrite aggregate)
            feats[bn] = f

    segs = [
        evaluate_segment("A", SEG_A, feats),
        evaluate_segment("B", SEG_B, feats),
        evaluate_segment("C", SEG_C, feats),
    ]
    # overall
    all_pairs = [p for s in segs for p in s["pairs"]]
    if all_pairs:
        mean_adj = sum(p["err_prior_tm1"] for p in all_pairs) / len(all_pairs)
        mean_glob = sum(p["err_global_prior"] for p in all_pairs) / len(all_pairs)
        better_frac = sum(1 for p in all_pairs if p["tm1_better"]) / len(all_pairs)
        overall_better = mean_adj < mean_glob
    else:
        mean_adj = mean_glob = better_frac = None
        overall_better = False

    out = {
        "method": {
            "features": FEATURE_KEYS,
            "distance": "L2 on raw feature vector (unstandardized)",
            "global_prior": "mean feature vector within segment",
            "test": "err(t-1 → t) < err(global → t)",
        },
        "features_by_block": feats,
        "segments": segs,
        "overall": {
            "n_pairs": len(all_pairs),
            "mean_err_tm1": mean_adj,
            "mean_err_global": mean_glob,
            "tm1_better_frac": better_frac,
            "prior_useful": overall_better,
            "conclusion": (
                "YES — block t-1 predicts t better than global prior; inter-block sticky prior justified."
                if overall_better
                else "NO — t-1 is not reliably better than global prior; prefer intra-block EV over sticky cross-block priors."
            ),
        },
        "gaps": {
            "B_missing_19606600": 19606600 not in feats,
            "note": "Segment B historically incomplete (19606600 not on disk); L4 uses 597–599 only.",
        },
    }

    RESULTS.mkdir(parents=True, exist_ok=True)
    jp = RESULTS / "l4-prior-predictivity.json"
    jp.write_text(json.dumps(out, indent=2))

    md = [
        "# L4 prior predictivity (segments A/B/C)",
        "",
        f"**Overall:** {'YES' if overall_better else 'NO'} — t-1 better than global prior",
        "",
        out["overall"]["conclusion"],
        "",
        f"- pairs={len(all_pairs)} mean_err_tm1={mean_adj} mean_err_global={mean_glob} better_frac={better_frac}",
        "",
        "## Per segment",
        "",
    ]
    for s in segs:
        md.append(
            f"- **{s['segment']}**: present={s['blocks_present']} missing={s['blocks_missing']} "
            f"tm1_better={s['tm1_predicts_better']} frac={s['tm1_better_frac']:.2f} "
            f"err_tm1={s['mean_err_tm1']} err_glob={s['mean_err_global']}"
        )
    md += ["", "## Gaps", "", f"- 19606600 missing: {out['gaps']['B_missing_19606600']}", ""]
    mp = RESULTS / "l4-prior-predictivity.md"
    mp.write_text("\n".join(md) + "\n")
    print(f"wrote {jp}")
    print(f"wrote {mp}")
    print("PRIOR_USEFUL:", overall_better, "better_frac:", better_frac)


if __name__ == "__main__":
    main()
