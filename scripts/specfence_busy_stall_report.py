#!/usr/bin/env python3
"""Read SPECFENCE_BUSY_STALL JSON and print the dig tables.

Probe walls are not a new locked band. Ideal clocks are the locked
15274915 values (L_crit=1.19 ms, Σwork=3.02 ms), not recomputed here.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

IDEAL_LCRIT_NS = 1_190_000
IDEAL_WORK_NS = 3_020_000
BIN_NS = 250_000
KINDS = [
    "evm_first",
    "evm_first_abort",
    "evm_refull",
    "evm_repart",
    "validate",
    "publish",
    "sched",
    "detect",
    "learn",
    "lock",
    "spin",
    "stall_handoff",
    "stall_commit",
    "stall_refuse",
    "stall_waitonce",
    "stall_noready",
    "stall_join",
]
BUSY = {
    "evm_first",
    "evm_first_abort",
    "evm_refull",
    "evm_repart",
    "validate",
    "publish",
    "sched",
    "detect",
    "learn",
    "lock",
    "spin",
}
STALL = {
    "stall_handoff",
    "stall_commit",
    "stall_refuse",
    "stall_waitonce",
    "stall_noready",
    "stall_join",
}
EVM = {"evm_first", "evm_first_abort", "evm_refull", "evm_repart"}


def ms(ns: float) -> float:
    return ns / 1e6


def load(path: Path) -> dict:
    with path.open() as f:
        return json.load(f)


def totals(row: dict) -> dict[str, int]:
    acc = {k: 0 for k in KINDS}
    for w in row["snap"]["workers"]:
        for i, ns in enumerate(w["ns"]):
            if i < len(KINDS):
                acc[KINDS[i]] += ns
    return acc


def life(row: dict) -> int:
    return sum(w["life_ns"] for w in row["snap"]["workers"])


def median(xs: list[float]) -> float:
    """Protocol median: drop nothing here; caller drops the cold iter.

    Even counts use the upper middle value, ``sorted[len // 2]``, not the
    average of the two middle values.
    """
    if not xs:
        return 0.0
    ys = sorted(xs)
    return ys[len(ys) // 2]


def fmt_row(cells: list[str]) -> str:
    return "| " + " | ".join(cells) + " |"


def per_worker(row: dict) -> str:
    lines = [
        fmt_row(["w", "life", "busy", "stall", "evm", "reexec", "val", "pub", "sched", "detect", "lock", "gap"]),
        fmt_row(["---"] * 12),
    ]
    for w in row["snap"]["workers"]:
        ns = w["ns"]
        def g(i: int) -> int:
            return ns[i] if i < len(ns) else 0

        busy = sum(g(i) for i, k in enumerate(KINDS) if k in BUSY)
        stall = sum(g(i) for i, k in enumerate(KINDS) if k in STALL)
        evm = g(0) + g(1) + g(2) + g(3)
        re = g(2) + g(3)
        gap = w["life_ns"] - busy - stall
        lines.append(
            fmt_row(
                [
                    str(w["worker"]),
                    f"{ms(w['life_ns']):.3f}",
                    f"{ms(busy):.3f}",
                    f"{ms(stall):.3f}",
                    f"{ms(evm):.3f}",
                    f"{ms(re):.3f}",
                    f"{ms(g(4)):.3f}",
                    f"{ms(g(5)):.3f}",
                    f"{ms(g(6)):.3f}",
                    f"{ms(g(7)):.3f}",
                    f"{ms(g(9)):.3f}",
                    f"{ms(gap):.3f}",
                ]
            )
        )
    return "\n".join(lines)


def timeline(row: dict, wall_ns: float) -> str:
    """Thread-ns per 0.25 ms bin, grouped."""
    bins = int(row["snap"]["bins"])
    acc = [[0] * bins for _ in range(3)]  # evm, other busy, stall
    names = row["snap"]["kinds"]
    for b in row["snap"]["timeline"]:
        k = names[b["k"]] if b["k"] < len(names) else "?"
        bi = b["bin"]
        if bi >= bins:
            continue
        if k in EVM:
            acc[0][bi] += b["ns"]
        elif k in STALL:
            acc[2][bi] += b["ns"]
        else:
            acc[1][bi] += b["ns"]
    last = 0
    for i in range(bins):
        if acc[0][i] or acc[1][i] or acc[2][i]:
            last = i
    last = min(bins - 1, max(last, int(wall_ns / BIN_NS) + 1))
    lines = [
        fmt_row(["t_ms", "evm_thr", "other_busy_thr", "stall_thr", "thr/4"]),
        fmt_row(["---"] * 5),
    ]
    for i in range(last + 1):
        t = (i + 0.5) * BIN_NS
        mark = ""
        if abs(t - IDEAL_LCRIT_NS) < BIN_NS / 2:
            mark = " Lcrit"
        s = acc[0][i] + acc[1][i] + acc[2][i]
        lines.append(
            fmt_row(
                [
                    f"{ms(i * BIN_NS):.2f}{mark}",
                    f"{ms(acc[0][i]):.3f}",
                    f"{ms(acc[1][i]):.3f}",
                    f"{ms(acc[2][i]):.3f}",
                    f"{ms(s) / 4:.3f}",
                ]
            )
        )
    ov = row["snap"].get("timeline_overflow_ns", 0)
    if ov:
        lines.append(f"\noverflow_ns={ov}")
    return "\n".join(lines)


def critical_path(row: dict) -> str:
    spans = row["snap"]["spans"]
    if not spans:
        return "(no spans)"
    by_w: dict[int, list[dict]] = {}
    for s in spans:
        by_w.setdefault(s["w"], []).append(s)
    for w in by_w:
        by_w[w].sort(key=lambda s: (s["t0_ns"], s["dur_ns"]))

    def end(s: dict) -> int:
        return s["t0_ns"] + max(s["gross_ns"], s["dur_ns"])

    # Last span end across workers is the probe's view of the tail.
    last = max(spans, key=end)
    lines = [f"probe_tail_ms={ms(end(last)):.3f} worker={last['w']} kind={last['kind']} tx={last['tx']}"]
    # Walk the worker that owns the tail, splitting gross EVM around contained stalls.
    w = last["w"]
    chain = by_w[w]
    # Keep segments that are not strictly inside another span of the same worker.
    covered = []
    for s in chain:
        if s["dur_ns"] <= 0 and s["gross_ns"] <= 0:
            continue
        covered.append(s)
    # Collapse to a readable path: merge adjacent same-kind if gap < 20us, else emit gap.
    out = []
    prev_end = 0
    for s in covered:
        t0 = s["t0_ns"]
        dur = s["dur_ns"] if s["dur_ns"] else s["gross_ns"]
        if t0 > prev_end + 20_000:
            out.append(("gap", prev_end, t0 - prev_end, None, None))
        label = s["kind"]
        if s.get("pred") is not None and label.startswith("stall"):
            label = f"{label}->tx{s['pred']}"
        elif label.startswith("evm"):
            label = f"{label}#tx{s['tx']}/inc{s['inc']}"
        out.append((label, t0, dur, s["tx"], s.get("pred")))
        prev_end = max(prev_end, t0 + dur)
    # Print only segments >= 0.05 ms plus a sum of the small ones.
    lines.append(fmt_row(["t0_ms", "dur_ms", "label"]))
    lines.append(fmt_row(["---", "---", "---"]))
    small = 0
    shown = 0
    for label, t0, dur, _tx, _pred in out:
        if dur < 50_000 and label != "gap":
            small += dur
            continue
        if label == "gap" and dur < 50_000:
            small += dur
            continue
        lines.append(fmt_row([f"{ms(t0):.3f}", f"{ms(dur):.3f}", label]))
        shown += 1
        if shown > 40:
            lines.append("| … | … | truncated |")
            break
    lines.append(f"\nsmall_segments_ms={ms(small):.3f} (under 0.05 ms, not listed)")
    # Ordered hop gaps: successor evm start minus pred publish end.
    pub = {}
    for s in spans:
        if s["kind"] == "publish":
            pub[s["tx"]] = max(pub.get(s["tx"], 0), end(s))
    hops = []
    for s in spans:
        if s["kind"] in ("evm_first", "evm_first_abort") and s.get("pred") is not None:
            pe = pub.get(s["pred"])
            if pe is None:
                continue
            gap = s["t0_ns"] - pe
            if gap > 0:
                hops.append(gap)
    if hops:
        hops.sort()
        lines.append(
            f"ordered_or_block_hop_gaps n={len(hops)} sum_ms={ms(sum(hops)):.3f} "
            f"median_us={hops[len(hops)//2]/1e3:.1f} max_ms={ms(hops[-1]):.3f}"
        )
    return "\n".join(lines)


def main() -> None:
    root = Path(sys.argv[1] if len(sys.argv) > 1 else "results/soft0-busy-stall-dig")
    files = sorted(root.glob("*.json"))
    rows = [load(p) for p in files if p.name[0].isdigit()]
    if not rows:
        print("no rows", root)
        return
    print("# bucket sums (thread ms). reuse = iter>0\n")
    header = ["block", "mode", "iter", "wall", "life", "busy", "stall", "evm_first", "abort", "refull", "repart", "val", "pub", "sched", "detect", "learn", "lock", "spin", "handoff", "commit", "refuse", "waitonce", "noready", "join", "gap"]
    print(fmt_row(header))
    print(fmt_row(["---"] * len(header)))
    grouped: dict[tuple, list] = {}
    for row in rows:
        t = totals(row)
        lf = life(row)
        busy = sum(t[k] for k in BUSY)
        stall = sum(t[k] for k in STALL)
        gap = lf - busy - stall
        key = (row["block"], row["mode"])
        if row["iter"] > 0:
            grouped.setdefault(key, []).append((row, t, lf))
        cells = [
            str(row["block"]),
            row["mode"],
            str(row["iter"]),
            f"{row['wall_ms']:.3f}",
            f"{ms(lf):.3f}",
            f"{ms(busy):.3f}",
            f"{ms(stall):.3f}",
        ]
        for k in [
            "evm_first",
            "evm_first_abort",
            "evm_refull",
            "evm_repart",
            "validate",
            "publish",
            "sched",
            "detect",
            "learn",
            "lock",
            "spin",
            "stall_handoff",
            "stall_commit",
            "stall_refuse",
            "stall_waitonce",
            "stall_noready",
            "stall_join",
        ]:
            cells.append(f"{ms(t[k]):.3f}")
        cells.append(f"{ms(gap):.3f}")
        print(fmt_row(cells))

    print("\n# reuse medians (thread ms)\n")
    print(fmt_row(["block", "mode", "n", "wall", "busy", "stall", "evm", "reexec", "detect", "sched", "val", "pub"]))
    print(fmt_row(["---"] * 12))
    for key, items in sorted(grouped.items()):
        walls = [r["wall_ms"] for r, _, _ in items]
        def med_ns(name: str | None, pred=None) -> float:
            vals = []
            for _r, t, _lf in items:
                if name is None:
                    vals.append(pred(t))
                else:
                    vals.append(t[name])
            return median(vals)

        evm = median([t["evm_first"] + t["evm_first_abort"] + t["evm_refull"] + t["evm_repart"] for _, t, _ in items])
        re = median([t["evm_refull"] + t["evm_repart"] for _, t, _ in items])
        busy = median([sum(t[k] for k in BUSY) for _, t, _ in items])
        stall = median([sum(t[k] for k in STALL) for _, t, _ in items])
        print(
            fmt_row(
                [
                    str(key[0]),
                    key[1],
                    str(len(items)),
                    f"{median(walls):.3f}",
                    f"{ms(busy):.3f}",
                    f"{ms(stall):.3f}",
                    f"{ms(evm):.3f}",
                    f"{ms(re):.3f}",
                    f"{ms(med_ns('detect')):.3f}",
                    f"{ms(med_ns('sched')):.3f}",
                    f"{ms(med_ns('validate')):.3f}",
                    f"{ms(med_ns('publish')):.3f}",
                ]
            )
        )

    # Detail the large-block reuse iteration whose SF wall is the median.
    large = [r for r in rows if r["block"] == 15274915 and r["iter"] > 0 and r["mode"] == "specfence"]
    if large:
        large.sort(key=lambda r: r["wall_ms"])
        med = large[len(large) // 2]
        print(f"\n# SF 15274915 median-wall iter {med['iter']} wall {med['wall_ms']:.3f}\n")
        print(per_worker(med))
        print("\n## timeline\n")
        print(timeline(med, med["wall_ms"] * 1e6))
        print("\n## critical path (tail worker)\n")
        print(critical_path(med))
        occs = [r for r in rows if r["block"] == 15274915 and r["iter"] == med["iter"] and r["mode"] == "occ"]
        if occs:
            print(f"\n# OCC same iter {med['iter']} wall {occs[0]['wall_ms']:.3f}\n")
            print(per_worker(occs[0]))
            print("\n## timeline\n")
            print(timeline(occs[0], occs[0]["wall_ms"] * 1e6))
            print("\n## critical path\n")
            print(critical_path(occs[0]))


if __name__ == "__main__":
    main()
