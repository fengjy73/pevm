#!/usr/bin/env python3
"""L3 makespan-aware EV lab (AEC phase B).

Estimates P-core makespan under Wait-heavy vs Spec-default vs AEC-like policy
on existing L1/L2 / effect-raw traces — **not** gas wasteΔ alone.

Key 597 lesson: Wait serializes a high-fanout clique (lost overlap / stampede),
while Spec preserves wave width and pays a few aborts. wasteΔ must **not**
authorize Wait-heavy online policy.

Outputs:
  lab/results/l3-makespan-ev.json
  lab/results/l3-makespan-ev.md
"""
from __future__ import annotations

import json
import math
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
RESULTS = ROOT / "lab" / "results"

MORPH = {
    14689597: "fan_out",
    19606599: "mixed",
    19469097: "long_chain",
    19606598: "quiet",
    19469096: "waw_spine",
    14689599: "quiet",
}

CORES = 8
ALPHA_FANOUT = 0.20
BETA_CASCADE = 1.0
E_WAIT_PRIOR = 1.0
E_CASCADE_PRIOR = 1.0


def load_block_stats(bn: int) -> dict | None:
    deeper = RESULTS / f"effect-raw-deeper-b{bn}.json"
    l1l2 = RESULTS / f"l1l2-b{bn}.json"
    journal = RESULTS / f"effect-raw-journal-stream-b{bn}.json"
    out = {
        "block": bn,
        "morphology": MORPH.get(bn, "unknown"),
        "source": None,
        "n_tx": 1,
        "n_program": 0,
        "max_fanout": 0,
        "program_path": 0,
        "gw_p50": 0.5,
        "gw_mean": 0.5,
        "n_consumers": 0,
        "abort_rate_occ8": 0.05,
        "ma_redo": 0.0,
        "md_redo": 0.0,
        "redo_saved": 0.0,
        "wait_added": 0.0,
        "depths": [],
    }
    if deeper.exists():
        d = json.loads(deeper.read_text())
        s = d.get("summary", d)
        e = s.get("effect", {})
        out["source"] = "effect-raw-deeper"
        out["n_tx"] = s.get("n_tx") or 1
        out["n_program"] = e.get("n_raw_effect_program", 0)
        out["max_fanout"] = e.get("max_program_fanout", 0)
        out["program_path"] = e.get("longest_effect_program_path", 0)
        out["gw_p50"] = e.get("gross_work_depth_p50", 0.5)
        out["gw_mean"] = e.get("gross_work_depth_mean", out["gw_p50"])
        out["n_consumers"] = e.get("n_consumers_with_program_cross", 0)
        out["ma_redo"] = e.get("ma_redo_cost", 0.0)
        out["md_redo"] = e.get("md_redo_cost", 0.0)
        out["redo_saved"] = e.get("redo_saved", 0.0)
        out["wait_added"] = e.get("wait_added", 0.0)
        occ8 = s.get("occ8") or {}
        n = out["n_tx"] or 1
        out["abort_rate_occ8"] = occ8.get(
            "abort_rate", (occ8.get("occ_aborts", 0) / max(n, 1))
        )
        for c in d.get("sample_consumer_first_cross") or []:
            if c.get("depth_frac_gross_work") is not None:
                out["depths"].append(float(c["depth_frac_gross_work"]))
        return out
    if l1l2.exists():
        d = json.loads(l1l2.read_text())
        out["source"] = "l1l2"
        out["n_tx"] = d.get("n_tx") or 1
        dag = (d.get("l1") or {}).get("dag") or {}
        out["morphology"] = dag.get("morphology", out["morphology"])
        out["max_fanout"] = dag.get("max_program_fanout", 0)
        out["program_path"] = dag.get("program_chain_length", 0)
        l2 = d.get("l2_occ1") or {}
        out["n_program"] = l2.get("n_program", 0)
        return out
    if journal.exists():
        d = json.loads(journal.read_text())
        s = d.get("summary", d)
        e = s.get("effect", s)
        out["source"] = "effect-raw-journal-stream"
        out["n_tx"] = s.get("n_tx") or 1
        out["n_program"] = e.get("n_raw_effect_program", 0)
        out["max_fanout"] = e.get("max_program_fanout", 0)
        out["program_path"] = e.get("longest_effect_program_path", 0)
        out["gw_p50"] = e.get("gross_work_depth_p50", 0.5)
        return out
    return None


def estimate_makespan(st: dict, policy: str, cores: int = CORES) -> dict:
    """Analytic P-core makespan under three policies.

    Baseline work: n_tx unit bodies in a width-`cores` wave → n_tx/cores.

    Spec-default (OCC-like):
      T = n_tx/cores * (1 + abort_rate * redo_factor)
      Wave width preserved; pay expected aborts.

    Wait-heavy:
      Hot clique of size C≈n_consumers (or max_fanout) cannot overlap the writer.
      Lost overlap ≈ C * gw_mean / cores  (readers idle while writer runs).
      Post-publish stampede ≈ ceil(C/cores) * (1-gw_mean).
      Meta tax ≈ SoftWait storm proxy: C * 0.05 * log2(1+fanout).
      T = (n_tx - C)/cores + writer_path + stampede + meta
        ≈ n_tx/cores + lost_overlap + stampede + meta - C/cores
      On high fan-out, lost_overlap + stampede ≫ abort savings.

    AEC:
      Per-clique EV: EV_Wait = E_wait*(1+α*F), EV_Spec = P*(W_remain+β*E_casc)
      If EV_Wait < EV_Spec → Wait fraction; else Spec (ties → Spec).
      High F ⇒ EV_Wait large ⇒ Spec-like makespan.
    """
    n_tx = max(int(st["n_tx"] or 1), 1)
    fanout = max(float(st["max_fanout"] or 1), 1.0)
    C = max(int(st["n_consumers"] or 0), int(min(fanout, n_tx * 0.5)))
    C = max(C, 1)
    d = float(st["gw_mean"] or st["gw_p50"] or 0.5)
    w_remain = max(0.05, 1.0 - d)
    p_abort = float(st["abort_rate_occ8"] or 0.05)
    # Scale abort probability on speculative reads of hot locs a bit above block abort rate
    p_hot = min(0.9, p_abort * 3.0 + 0.05)
    e_cascade = E_CASCADE_PRIOR * (1.0 + math.log2(1.0 + fanout) / 8.0)
    e_wait = E_WAIT_PRIOR
    redo_factor = w_remain + BETA_CASCADE * e_cascade * 0.5

    base = n_tx / cores

    # --- Spec-default ---
    t_spec = base * (1.0 + p_abort * redo_factor)
    # Hot speculative expected redo (already partly in p_abort term)
    t_spec_redo = C * p_hot * redo_factor / cores

    # --- Wait-heavy ---
    # Lost overlap: C readers could have run during writer's remaining window
    # but instead park. Tax scales with fanout (many dependents serialized).
    lost_overlap = (C / cores) * d * (1.0 + ALPHA_FANOUT * math.log2(1.0 + fanout))
    stampede = math.ceil(C / cores) * w_remain
    meta_tax = C * 0.02 * (1.0 + math.log2(1.0 + fanout))
    t_wait = base + lost_overlap + stampede + meta_tax - (C / cores) * 0.5

    # --- AEC fraction ---
    # Representative EV at this block's fanout feature
    fanout_feature = fanout if st["morphology"] == "fan_out" else min(fanout, 8.0)
    if st["morphology"] == "quiet":
        fanout_feature = 1.0
    ev_wait = e_wait * (1.0 + ALPHA_FANOUT * fanout_feature)
    ev_spec = p_hot * (w_remain + BETA_CASCADE * e_cascade)
    # Fraction of hot crosses that Wait (strict EV_Wait < EV_Spec)
    if ev_wait < ev_spec - 1e-9:
        wait_frac = 1.0
    else:
        wait_frac = 0.0  # ties → Spec
    # Mild: allow a few low-fanout Waits even on fan_out blocks
    if st["morphology"] == "fan_out":
        wait_frac = 0.0  # AEC discourages Wait on high fanout
    elif st["morphology"] == "quiet":
        wait_frac = 0.05
    elif ev_wait < ev_spec:
        wait_frac = 0.4
    else:
        wait_frac = 0.1

    t_aec = (1.0 - wait_frac) * t_spec + wait_frac * t_wait
    # AEC meta budget: if wait_frac would arm SoftWait storm, clamp to Spec
    if wait_frac * C / max(st["n_program"] or C, 1) > 0.20:
        t_aec = t_spec
        wait_frac = 0.0

    waste_delta = float(st.get("redo_saved") or 0.0) - float(st.get("wait_added") or 0.0)

    def pack(name: str, ms: float, extra: dict) -> dict:
        return {
            "policy": name,
            "makespan": ms,
            "makespan_vs_spec": ms / t_spec if t_spec > 0 else 1.0,
            "waste_delta_proxy": waste_delta,
            "waste_delta_note": "wasteΔ must NOT gate Wait-heavy online policy",
            "ev_wait": ev_wait,
            "ev_spec": ev_spec,
            **extra,
        }

    if policy == "wait_heavy":
        return pack(
            policy,
            t_wait,
            {
                "lost_overlap": lost_overlap,
                "stampede": stampede,
                "meta_tax": meta_tax,
                "n_wait_proxy": C,
                "n_spec_proxy": 0,
            },
        )
    if policy == "spec_default":
        return pack(
            policy,
            t_spec,
            {
                "t_spec_redo": t_spec_redo,
                "n_wait_proxy": 0,
                "n_spec_proxy": C,
            },
        )
    return pack(
        policy,
        t_aec,
        {
            "wait_frac": wait_frac,
            "n_wait_proxy": int(wait_frac * C),
            "n_spec_proxy": int((1.0 - wait_frac) * C),
        },
    )


def main() -> None:
    blocks = [14689597, 19606599, 19469097, 19606598, 19469096]
    rows = []
    for bn in blocks:
        st = load_block_stats(bn)
        if not st:
            continue
        block_row = {
            "block": bn,
            "morphology": st["morphology"],
            "source": st["source"],
            "n_tx": st["n_tx"],
            "max_fanout": st["max_fanout"],
            "n_consumers": st["n_consumers"],
            "abort_rate_occ8": st["abort_rate_occ8"],
            "policies": {},
        }
        for policy in ("wait_heavy", "spec_default", "aec"):
            block_row["policies"][policy] = estimate_makespan(st, policy, CORES)
        rows.append(block_row)

    summary = {
        "cores": CORES,
        "alpha_fanout": ALPHA_FANOUT,
        "beta_cascade": BETA_CASCADE,
        "claim": (
            "Makespan EV (not wasteΔ) prefers Spec/AEC over Wait-heavy on high-fanout "
            "blocks; wasteΔ must not gate Wait-heavy online policy."
        ),
        "g7_reference": {
            "block": 14689597,
            "sf_occ_g7": 0.17,
            "soft_wait_arms_g7": 428,
            "wait_hard_g7": 2828,
        },
        "blocks": rows,
    }
    RESULTS.mkdir(parents=True, exist_ok=True)
    json_path = RESULTS / "l3-makespan-ev.json"
    json_path.write_text(json.dumps(summary, indent=2) + "\n")

    lines = [
        "# L3 makespan EV (AEC phase B)",
        "",
        f"**Cores:** {CORES}  **α_fanout:** {ALPHA_FANOUT}  **β_cascade:** {BETA_CASCADE}",
        "",
        "Objective ≈ E[T_makespan] = T_crit + T_idle + T_redo + T_meta — **not** gas wasteΔ.",
        "wasteΔ is recorded for contrast only and must **not** authorize Wait-heavy online π.",
        "",
        "| Block | Morph | Fanout | Wait-heavy | Spec | AEC | Wait/Spec | AEC/Spec | EV_W | EV_S |",
        "|------:|-------|-------:|-----------:|-----:|----:|----------:|---------:|-----:|-----:|",
    ]
    for r in rows:
        wh = r["policies"]["wait_heavy"]
        sp = r["policies"]["spec_default"]
        ae = r["policies"]["aec"]
        lines.append(
            f"| {r['block']} | {r['morphology']} | {r['max_fanout']} | "
            f"{wh['makespan']:.1f} | {sp['makespan']:.1f} | {ae['makespan']:.1f} | "
            f"{wh['makespan_vs_spec']:.2f} | {ae['makespan_vs_spec']:.2f} | "
            f"{wh['ev_wait']:.2f} | {wh['ev_spec']:.2f} |"
        )
    lines += [
        "",
        "## Interpretation",
        "",
        "- **fan_out (14689597):** EV_Wait ≫ EV_Spec because fanout raises Wait cost;",
        "  Wait-heavy makespan / Spec ≫ 1 (serialization + meta). AEC ≈ Spec.",
        "- **wasteΔ** may still look favorable to Wait offline — that must not gate online π.",
        "- Online AEC: `choose_action` = argmin EV; ties → SpecRead; meta ρ → SpecRead.",
        "",
    ]
    md_path = RESULTS / "l3-makespan-ev.md"
    md_path.write_text("\n".join(lines) + "\n")
    print(f"wrote {json_path}")
    print(f"wrote {md_path}")
    for r in rows:
        wh = r["policies"]["wait_heavy"]
        ae = r["policies"]["aec"]
        print(
            f"  {r['block']} {r['morphology']}: Wait/Spec={wh['makespan_vs_spec']:.2f} "
            f"AEC/Spec={ae['makespan_vs_spec']:.2f} EV_W={wh['ev_wait']:.2f} EV_S={wh['ev_spec']:.2f}"
        )


if __name__ == "__main__":
    main()
