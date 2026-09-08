#!/usr/bin/env python3
"""L3 offline EV laboratory — M-A / M-D / Wait-if-program-fanout / Bind-if-ready.

Consumes:
  - lab/results/effect-raw-deeper-b*.json (preferred richer stats)
  - lab/results/l1l2-b*.json when present
  - lab/results/effect-raw-journal-stream-b*.json as fallback

Scores redo_saved, wait_added, crude makespan proxy per morphology class and block.
"""
from __future__ import annotations

import json
import math
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
RESULTS = ROOT / "lab" / "results"

# Morphology labels from plant measurement (frozen v3 + deeper pass).
MORPH = {
    14689597: "fan_out",
    19606599: "mixed",
    19469097: "long_chain",
    19606598: "quiet",
    19469096: "waw_spine",
    14689599: "quiet",
}

CORES = 8


def load_block_stats(bn: int) -> dict | None:
    """Return normalized per-block features for simulators."""
    deeper = RESULTS / f"effect-raw-deeper-b{bn}.json"
    l1l2 = RESULTS / f"l1l2-b{bn}.json"
    journal = RESULTS / f"effect-raw-journal-stream-b{bn}.json"

    out = {
        "block": bn,
        "morphology": MORPH.get(bn, "unknown"),
        "source": None,
        "n_tx": None,
        "n_raw": 0,
        "n_program": 0,
        "n_handler": 0,
        "max_fanout": 0,
        "program_path": 0,
        "gw_p50": 0.0,
        "gw_p10": 0.0,
        "gw_p90": 0.0,
        "gw_mean": 0.0,
        "n_consumers": 0,
        "ma_redo": 0.0,
        "md_redo": 0.0,
        "redo_saved": 0.0,
        "wait_added": 0.0,
        "edge_ready_done_frac": 1.0,
        "edge_ready_waitish_frac": 0.0,
        "abort_rate_occ8": 0.0,
        "depths": [],  # optional list of consumer gw depths
        "edges": [],  # optional list of edge dicts for finer sim
    }

    if deeper.exists():
        d = json.loads(deeper.read_text())
        s = d.get("summary", d)
        e = s.get("effect", {})
        e8 = s.get("effect_occ8") or {}
        out["source"] = "effect-raw-deeper"
        out["n_tx"] = s.get("n_tx")
        out["n_raw"] = e.get("n_raw_effect_total", 0)
        out["n_program"] = e.get("n_raw_effect_program", 0)
        out["n_handler"] = e.get("n_raw_effect_handler", 0)
        out["max_fanout"] = e.get("max_program_fanout", 0)
        out["program_path"] = e.get("longest_effect_program_path", 0)
        out["gw_p50"] = e.get("gross_work_depth_p50", 0.0)
        out["gw_p10"] = e.get("gross_work_depth_p10", 0.0)
        out["gw_p90"] = e.get("gross_work_depth_p90", 0.0)
        out["gw_mean"] = e.get("gross_work_depth_mean", 0.0)
        out["n_consumers"] = e.get("n_consumers_with_program_cross", 0)
        out["ma_redo"] = e.get("ma_redo_cost", 0.0)
        out["md_redo"] = e.get("md_redo_cost", 0.0)
        out["redo_saved"] = e.get("redo_saved", 0.0)
        out["wait_added"] = e.get("wait_added", 0.0)
        out["edge_ready_done_frac"] = e8.get("edge_ready_done_frac", e.get("edge_ready_done_frac", 1.0))
        out["edge_ready_waitish_frac"] = e8.get(
            "edge_ready_waitish_frac", e.get("edge_ready_waitish_frac", 0.0)
        )
        occ8 = s.get("occ8") or {}
        n = out["n_tx"] or 1
        out["abort_rate_occ8"] = occ8.get("abort_rate", (occ8.get("occ_aborts", 0) / max(n, 1)))
        # sample edges for finer sims
        for edge in d.get("sample_effect_edges") or []:
            out["edges"].append(edge)
        for c in d.get("sample_consumer_first_cross") or []:
            if c.get("depth_frac_gross_work") is not None:
                out["depths"].append(float(c["depth_frac_gross_work"]))
        return out

    if l1l2.exists():
        d = json.loads(l1l2.read_text())
        out["source"] = "l1l2"
        out["n_tx"] = d.get("n_tx")
        l1 = d.get("l1") or {}
        dag = l1.get("dag") or {}
        out["morphology"] = dag.get("morphology", out["morphology"])
        out["n_raw"] = dag.get("n_raw_instances", 0)
        out["max_fanout"] = dag.get("max_program_fanout", 0)
        out["program_path"] = dag.get("program_chain_length", 0)
        l2 = d.get("l2_occ1") or {}
        out["n_program"] = l2.get("n_program", 0)
        out["n_handler"] = l2.get("n_handler", 0)
        out["edge_ready_done_frac"] = (d.get("l2_occ8") or {}).get(
            "ready_for_bind_frac", l2.get("ready_for_bind_frac", 1.0)
        )
        out["edge_ready_waitish_frac"] = 1.0 - out["edge_ready_done_frac"]
        for e in l2.get("edges_sample") or []:
            out["edges"].append(e)
        for c in l2.get("consumer_first_cross_sample") or []:
            if c.get("depth_frac_gross_work") is not None:
                out["depths"].append(float(c["depth_frac_gross_work"]))
        # synthesize ma/md from depths if present
        if out["depths"]:
            n_c = len(out["depths"])
            out["n_consumers"] = n_c
            out["ma_redo"] = float(n_c)
            out["md_redo"] = sum(max(0.0, 1.0 - d) for d in out["depths"])
            out["redo_saved"] = out["ma_redo"] - out["md_redo"]
            out["gw_p50"] = sorted(out["depths"])[n_c // 2]
        return out

    if journal.exists():
        d = json.loads(journal.read_text())
        s = d.get("summary", d)
        e = s.get("effect", s)
        out["source"] = "effect-raw-journal-stream"
        out["n_tx"] = s.get("n_tx")
        out["n_raw"] = e.get("n_raw_effect_total", 0)
        out["n_program"] = e.get("n_raw_effect_program", 0)
        out["n_handler"] = e.get("n_raw_effect_handler", 0)
        out["max_fanout"] = e.get("max_program_fanout", 0)
        out["program_path"] = e.get("longest_effect_program_path", 0)
        out["gw_p50"] = e.get("gross_work_depth_p50", 0.0)
        out["n_consumers"] = e.get("n_consumers_with_program_cross", 0)
        out["ma_redo"] = e.get("ma_redo_cost", 0.0)
        out["md_redo"] = e.get("md_redo_cost", 0.0)
        out["redo_saved"] = e.get("redo_saved", 0.0)
        out["wait_added"] = e.get("wait_added", 0.0)
        return out

    return None


def synth_depths(st: dict) -> list[float]:
    if st["depths"]:
        return list(st["depths"])
    n = max(int(st["n_consumers"] or 0), 1)
    # triangular around p50 with p10/p90 spread when available
    p10, p50, p90 = st["gw_p10"], st["gw_p50"], st["gw_p90"]
    if p50 <= 0 and st["gw_mean"] > 0:
        p50 = st["gw_mean"]
    if p90 <= p50:
        p90 = min(1.0, p50 + 0.15)
    if p10 <= 0 or p10 > p50:
        p10 = max(0.0, p50 - 0.15)
    xs = []
    for i in range(n):
        t = i / max(n - 1, 1)
        if t < 0.5:
            xs.append(p10 + (p50 - p10) * (t / 0.5))
        else:
            xs.append(p50 + (p90 - p50) * ((t - 0.5) / 0.5))
    return xs


def makespan_proxy(n_tx: int, chain: int, fanout: int, waste: float, cores: int = CORES) -> float:
    """Crude: critical path work + waste / cores. Unit = consumer-work."""
    n = max(n_tx or 1, 1)
    # independent wave ≈ n / max(wave-ish,1); use fanout/chain as morphology cues
    crit = max(chain, 1) + max(fanout, 1) * 0.002
    parallel = n / max(cores, 1)
    return max(crit, parallel) + waste / max(cores, 1)


def sim_m_a(st: dict) -> dict:
    depths = synth_depths(st)
    # always speculate: on conflict redo full remaining ≈ 1.0 per consumer with program cross
    redo = float(len(depths))
    wait = 0.0
    waste = redo
    ms = makespan_proxy(st["n_tx"] or 0, st["program_path"], st["max_fanout"], waste)
    return {"policy": "M-A", "redo": redo, "wait": wait, "waste": waste, "makespan": ms}


def sim_m_d(st: dict) -> dict:
    depths = synth_depths(st)
    # discover→decide: redo (1-d); wait from plant wait_added scaled
    redo = sum(max(0.0, 1.0 - d) for d in depths)
    wait = float(st.get("wait_added") or 0.0)
    # if wait_added missing, estimate from unique program pairs lag
    if wait <= 0 and st["n_program"]:
        wait = 0.15 * math.sqrt(st["n_program"])
    waste = redo + wait
    ms = makespan_proxy(st["n_tx"] or 0, st["program_path"], st["max_fanout"], waste)
    return {"policy": "M-D", "redo": redo, "wait": wait, "waste": waste, "makespan": ms}


def sim_wait_if_program_fanout(st: dict) -> dict:
    """Wait when program & (fanout high or d large); else SpecRead like M-A on remainder."""
    depths = synth_depths(st)
    fanout_high = st["max_fanout"] >= 50
    morph = st["morphology"]
    wait_morph = morph in ("fan_out", "long_chain")
    redo = 0.0
    wait = 0.0
    n = len(depths)
    # fraction of edges that would Wait
    for d in depths:
        do_wait = (wait_morph or fanout_high) and (d >= 0.5 or fanout_high)
        if do_wait:
            # saved redo ≈ d, pay wait proportional to (1 - ready)
            waitish = st["edge_ready_waitish_frac"]
            wait += waitish * 0.5 + (1.0 - waitish) * 0.05
            redo += max(0.0, 1.0 - d) * 0.15  # residual miss
        else:
            redo += 1.0  # full abort redo like M-A
    # handler chatter: no extra wait
    waste = redo + wait
    ms = makespan_proxy(st["n_tx"] or 0, st["program_path"], st["max_fanout"], waste)
    return {
        "policy": "Wait-if-program-fanout",
        "redo": redo,
        "wait": wait,
        "waste": waste,
        "makespan": ms,
        "n_consumers": n,
    }


def sim_bind_if_ready(st: dict) -> dict:
    """Bind when producer Data; else SpecRead (M-A redo on miss)."""
    depths = synth_depths(st)
    ready = st["edge_ready_done_frac"]
    waitish = st["edge_ready_waitish_frac"]
    redo = 0.0
    wait = 0.0
    for d in depths:
        # with prob ready: Bind → tiny wait, almost no redo
        # with prob waitish: must Wait or Spec; use Spec → redo (1-d) approx full if late
        # blend
        redo += (1.0 - ready) * max(0.0, 1.0 - d) + ready * 0.02
        wait += ready * 0.02 + waitish * 0.25
    waste = redo + wait
    ms = makespan_proxy(st["n_tx"] or 0, st["program_path"], st["max_fanout"], waste)
    return {"policy": "Bind-if-ready", "redo": redo, "wait": wait, "waste": waste, "makespan": ms}


def vs_ma(policy: dict, ma: dict) -> dict:
    return {
        **policy,
        "redo_saved_vs_MA": ma["redo"] - policy["redo"],
        "wait_added_vs_MA": policy["wait"] - ma["wait"],
        "makespan_delta_vs_MA": policy["makespan"] - ma["makespan"],
        "waste_delta_vs_MA": policy["waste"] - ma["waste"],
        "win_vs_MA": policy["waste"] < ma["waste"] - 1e-9,
    }


def main():
    blocks = sorted(set(MORPH) | {14689597, 19606599, 19469097, 19606598, 19469096})
    per_block = []
    by_morph = defaultdict(list)

    for bn in blocks:
        st = load_block_stats(bn)
        if not st:
            continue
        ma = sim_m_a(st)
        policies = [
            vs_ma(sim_m_d(st), ma),
            vs_ma(sim_wait_if_program_fanout(st), ma),
            vs_ma(sim_bind_if_ready(st), ma),
        ]
        row = {
            "block": bn,
            "morphology": st["morphology"],
            "source": st["source"],
            "n_tx": st["n_tx"],
            "n_raw": st["n_raw"],
            "n_program": st["n_program"],
            "max_fanout": st["max_fanout"],
            "program_path": st["program_path"],
            "gw_p50": st["gw_p50"],
            "edge_ready_done_frac": st["edge_ready_done_frac"],
            "edge_ready_waitish_frac": st["edge_ready_waitish_frac"],
            "abort_rate_occ8": st["abort_rate_occ8"],
            "M-A": ma,
            "policies": {p["policy"]: p for p in policies},
        }
        per_block.append(row)
        by_morph[st["morphology"]].append(row)

    # morphology aggregates
    morph_summary = {}
    for morph, rows in sorted(by_morph.items()):
        agg = {}
        for pname in ("M-D", "Wait-if-program-fanout", "Bind-if-ready"):
            wastes = [r["policies"][pname]["waste_delta_vs_MA"] for r in rows]
            wins = [r["policies"][pname]["win_vs_MA"] for r in rows]
            redo_s = [r["policies"][pname]["redo_saved_vs_MA"] for r in rows]
            wait_a = [r["policies"][pname]["wait_added_vs_MA"] for r in rows]
            ms_d = [r["policies"][pname]["makespan_delta_vs_MA"] for r in rows]
            agg[pname] = {
                "n_blocks": len(rows),
                "mean_waste_delta_vs_MA": sum(wastes) / len(wastes),
                "mean_redo_saved_vs_MA": sum(redo_s) / len(redo_s),
                "mean_wait_added_vs_MA": sum(wait_a) / len(wait_a),
                "mean_makespan_delta_vs_MA": sum(ms_d) / len(ms_d),
                "win_frac": sum(1 for w in wins if w) / len(wins),
                "all_win": all(wins),
            }
        morph_summary[morph] = agg

    # Gate: hot morphology EV win AND quiet not degraded
    hot = {"fan_out", "mixed", "long_chain", "waw_spine"}
    quiet = {"quiet"}
    gate = {
        "hot_morphologies_tested": sorted(hot & set(morph_summary)),
        "quiet_morphologies_tested": sorted(quiet & set(morph_summary)),
        "per_policy": {},
    }
    for pname in ("M-D", "Wait-if-program-fanout", "Bind-if-ready"):
        hot_wins = []
        quiet_ok = []
        for morph, agg in morph_summary.items():
            p = agg[pname]
            if morph in hot:
                hot_wins.append(p["mean_waste_delta_vs_MA"] < 0)
            if morph in quiet:
                # not degraded: waste delta not positive beyond small epsilon
                quiet_ok.append(p["mean_waste_delta_vs_MA"] <= 1.0)
        gate["per_policy"][pname] = {
            "hot_ev_win_any": any(hot_wins) if hot_wins else False,
            "hot_ev_win_all": all(hot_wins) if hot_wins else False,
            "quiet_not_degraded": all(quiet_ok) if quiet_ok else True,
            "implement_choose_action_candidate": (any(hot_wins) if hot_wins else False)
            and (all(quiet_ok) if quiet_ok else True),
        }

    # Prefer Wait-if-program-fanout / Bind-if-ready as v3-aligned; M-D is oracle-ish upper bound
    candidates = [
        p
        for p, g in gate["per_policy"].items()
        if g["implement_choose_action_candidate"] and p != "M-D"
    ]
    decision = {
        "implement_choose_action": len(candidates) > 0,
        "winning_policies": candidates,
        "reason": (
            f"L3 offline: candidates={candidates}. "
            + (
                "Hot morphology EV win without quiet degradation → authorize choose_action v3."
                if candidates
                else "No v3-aligned policy wins on ≥1 hot morphology without quiet degradation → DEFER choose_action."
            )
        ),
    }

    out = {
        "method": {
            "location": "MemoryLocation",
            "warm_policy": "emit_warm_true",
            "primary_depth": "gross_work",
            "primary_depth_formula": "gas_used_so_far/tx_gas_used",
            "raw_instance": True,
            "excluded_from_effective_gstar": ["coinbase_beneficiary", "basic_lazy"],
            "producer_status_sampled_at": "discovering_incarnation",
            "account_grain": "diagnostic_only",
            "schema_version": "l1l2-v1",
            "l3_assumptions": [
                "Consumer-work units; not wall-clock calibrated",
                "Depths from gross-work first-cross (sampled or synthetic from p10/p50/p90)",
                "Wait-if-program-fanout uses morphology + fanout + d>=0.5",
                "Bind-if-ready uses OCC@8 edge_ready_done_frac",
                "M-A = always SpecRead / full redo on conflict consumers",
            ],
        },
        "per_block": per_block,
        "by_morphology": morph_summary,
        "gate": gate,
        "decision": decision,
    }

    RESULTS.mkdir(parents=True, exist_ok=True)
    json_path = RESULTS / "l3-offline-ev.json"
    json_path.write_text(json.dumps(out, indent=2))

    md_lines = [
        "# L3 offline EV lab",
        "",
        f"**Decision:** {'IMPLEMENT' if decision['implement_choose_action'] else 'DEFER'} choose_action",
        "",
        decision["reason"],
        "",
        "## Per-morphology wasteΔ vs M-A (negative = better)",
        "",
        "| Morphology | Policy | mean wasteΔ | redo_saved | wait_added | win_frac |",
        "|------------|--------|------------:|-----------:|-----------:|---------:|",
    ]
    for morph, agg in sorted(morph_summary.items()):
        for pname, p in agg.items():
            md_lines.append(
                f"| {morph} | {pname} | {p['mean_waste_delta_vs_MA']:.2f} | "
                f"{p['mean_redo_saved_vs_MA']:.2f} | {p['mean_wait_added_vs_MA']:.2f} | {p['win_frac']:.2f} |"
            )
    md_lines += ["", "## Per-block headlines", ""]
    for r in per_block:
        md_lines.append(
            f"- **{r['block']}** ({r['morphology']}): raw={r['n_raw']} fanout={r['max_fanout']} "
            f"gw_p50={r['gw_p50']:.3f} ready_done={r['edge_ready_done_frac']:.3f}"
        )
        for pname, p in r["policies"].items():
            md_lines.append(
                f"  - {pname}: redo_saved={p['redo_saved_vs_MA']:.1f} wait_added={p['wait_added_vs_MA']:.1f} "
                f"wasteΔ={p['waste_delta_vs_MA']:.1f} win={p['win_vs_MA']}"
            )
    md_lines += ["", "## Gate", "", "```json", json.dumps(gate, indent=2), "```", ""]
    md_path = RESULTS / "l3-offline-ev.md"
    md_path.write_text("\n".join(md_lines) + "\n")
    print(f"wrote {json_path}")
    print(f"wrote {md_path}")
    print("DECISION:", decision["implement_choose_action"], decision["winning_policies"])


if __name__ == "__main__":
    main()
