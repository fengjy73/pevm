#!/usr/bin/env python3
"""Ideal_step(C) from a saved sequential access trace.

Offsets are fractions of the trace-run ExecPhase duration, then scaled onto
the same-timer per-tx cost (OCC workers=1, probes other than this trace off)
so the makespan shares a clock with Ideal_tx.

RAW/WAW: start_i + r_i >= start_j + w_j.
WAR primary: no ordering (version retained).
WAR conservative: start_later_writer >= start_reader + r_reader - w_writer.
"""

from __future__ import annotations

import heapq
import math
import statistics
from collections import defaultdict

KIND = {0: "basic", 1: "storage", 2: "code", 3: "lazy"}
HOTS = (
    ("spine", "abd6bb397881"),
    ("bef034365ca24581", "bef034365ca24581"),
    ("d836a55a84878178", "d836a55a84878178"),
    ("930831d7501a43bf", "930831d7501a43bf"),
)


def loc_hex(loc: int) -> str:
    return f"{int(loc) & 0xFFFFFFFFFFFFFFFF:016x}"


def hot_name(loc: int) -> str | None:
    h = loc_hex(loc)
    for name, prefix in HOTS:
        if h.startswith(prefix):
            return name
    return None


def _pct(xs: list[float], p: float) -> float | None:
    if not xs:
        return None
    ys = sorted(xs)
    i = min(len(ys) - 1, max(0, int(round(p * (len(ys) - 1)))))
    return ys[i]


def _dist(xs: list[float]) -> dict | None:
    if not xs:
        return None
    return {
        "n": len(xs),
        "mean": statistics.fmean(xs),
        "stdev": statistics.pstdev(xs) if len(xs) > 1 else 0.0,
        "p50": _pct(xs, 0.5),
        "p90": _pct(xs, 0.9),
        "min": min(xs),
        "max": max(xs),
    }


def reduce_round(txs: list[dict]) -> dict[int, dict]:
    """tx -> {dur, selector, reads, writes} using the highest incarnation."""
    by: dict[int, dict] = {}
    for txr in txs:
        tx = int(txr["tx"])
        inc = int(txr["inc"])
        prev = by.get(tx)
        if prev is not None and int(prev["inc"]) > inc:
            continue
        dur = int(txr.get("dur_ns") or 0)
        reads: dict[int, dict] = {}
        writes: dict[int, dict] = {}
        writes_first: dict[int, dict] = {}
        for a in txr.get("accesses") or []:
            loc = int(a["loc"])
            ns = int(a["ns"])
            rec = {
                "ns": ns,
                "frac": (ns / dur) if dur else 0.0,
                "pc": int(a.get("pc") or 0),
                "op_index": int(a.get("op_index") or 0),
                "op": int(a.get("op") or 0),
                "code_hash": a.get("code_hash") or "",
                "kind": int(a.get("kind") or 0),
                "addr": a.get("addr") or "",
                "slot": a.get("slot") or "",
            }
            if int(a.get("rw") or 0) == 0:
                old = reads.get(loc)
                if old is None or ns < old["ns"]:
                    reads[loc] = rec
            else:
                old_last = writes.get(loc)
                if old_last is None or ns >= old_last["ns"]:
                    writes[loc] = rec
                old_first = writes_first.get(loc)
                if old_first is None or ns < old_first["ns"]:
                    writes_first[loc] = rec
        by[tx] = {
            "inc": inc,
            "dur": dur,
            "selector": txr.get("selector") or "",
            "reads": reads,
            "writes": writes,
            "writes_first": writes_first,
        }
    return by


def median_maps(rounds: list[dict[int, dict]]) -> dict[int, dict]:
    """Median fraction per (tx, loc) across rounds. PC/code from the middle round."""
    if not rounds:
        return {}
    txs = set()
    for rd in rounds:
        txs.update(rd)
    out = {}
    for tx in sorted(txs):
        durs = [rd[tx]["dur"] for rd in rounds if tx in rd and rd[tx]["dur"] > 0]
        present = [rd[tx] for rd in rounds if tx in rd]
        if not present:
            continue
        mid = present[len(present) // 2]
        sel = mid["selector"]
        locs = set()
        for p in present:
            locs.update(p["reads"])
            locs.update(p["writes"])
            locs.update(p.get("writes_first") or {})
        reads = {}
        writes = {}
        writes_first = {}

        def take(box: str, dest: dict) -> None:
            fracs = [p[box][loc]["frac"] for p in present if loc in p.get(box, {})]
            if not fracs:
                return
            base = next(p[box][loc] for p in present if loc in p.get(box, {}))
            rec = dict(base)
            rec["frac"] = statistics.median(fracs)
            rec["frac_stdev"] = statistics.pstdev(fracs) if len(fracs) > 1 else 0.0
            rec["n_rounds"] = len(fracs)
            dest[loc] = rec

        for loc in locs:
            take("reads", reads)
            take("writes", writes)
            take("writes_first", writes_first)
        out[tx] = {
            "dur_trace": statistics.median(durs) if durs else 0,
            "selector": sel,
            "reads": reads,
            "writes": writes,
            "writes_first": writes_first,
        }
    return out


def build_edges(state: dict[int, dict], beneficiary: int, drop_lazy_beneficiary: bool):
    """Return list of (j, i, lag_frac_unscaled_parts, klass, loc, w_frac, r_frac).

    lag is in 'fraction units' and scaled later by the two txs' durations:
    start_i >= start_j + w_j - r_i, with w_j = w_frac * d_j, r_i = r_frac * d_i.
    """
    readers: dict[int, list[int]] = defaultdict(list)
    writers: dict[int, list[int]] = defaultdict(list)
    kinds: dict[int, int] = {}
    for tx, st in state.items():
        for loc, rec in st["reads"].items():
            if drop_lazy_beneficiary and (loc == beneficiary or rec["kind"] == 3):
                continue
            readers[loc].append(tx)
            kinds[loc] = rec["kind"]
        for loc, rec in st["writes"].items():
            if drop_lazy_beneficiary and (loc == beneficiary or rec["kind"] == 3):
                continue
            writers[loc].append(tx)
            kinds[loc] = rec["kind"]
    for xs in readers.values():
        xs.sort()
    for xs in writers.values():
        xs.sort()
    edges = []

    def frac_w(tx, loc):
        return state[tx]["writes"][loc]["frac"]

    def frac_w_first(tx, loc):
        box = state[tx].get("writes_first") or state[tx]["writes"]
        return box[loc]["frac"]

    def frac_r(tx, loc):
        return state[tx]["reads"][loc]["frac"]

    for loc, ws in writers.items():
        for a, b in zip(ws, ws[1:]):
            # Successor offset is its first write, not the journal-final write.
            edges.append(("waw", a, b, loc, frac_w(a, loc), frac_w_first(b, loc)))
        rs = readers.get(loc) or []
        for r in rs:
            lower = [w for w in ws if w < r]
            if lower:
                w = lower[-1]
                edges.append(("raw", w, r, loc, frac_w(w, loc), frac_r(r, loc)))
            higher = [w for w in ws if w > r]
            if higher:
                w = higher[0]
                # Conservative WAR: later writer w, earlier reader r.
                edges.append(("war", r, w, loc, frac_r(r, loc), frac_w(w, loc)))
    return edges


def scale_edges(edges, durs: list[float], classes: set[str]):
    """edges rows are (klass, j, i, loc, fj, fi). lag_ns = fj*d_j - fi*d_i."""
    out = []
    for klass, j, i, loc, fj, fi in edges:
        if klass not in classes:
            continue
        if j >= len(durs) or i >= len(durs):
            continue
        lag = fj * durs[j] - fi * durs[i]
        out.append((j, i, lag, klass, loc, fj, fi))
    # Collapse duplicate (j,i) to the most restrictive (max) lag, keep the edge that won.
    best = {}
    for row in out:
        key = (row[0], row[1])
        if key not in best or row[2] > best[key][2]:
            best[key] = row
    return list(best.values())


def schedule(durs: list[float], edges: list[tuple], cores: int) -> dict:
    n = len(durs)
    cores = max(1, cores)
    succ: list[list[tuple]] = [[] for _ in range(n)]
    indeg = [0] * n
    seen = set()
    for j, i, lag, klass, loc, fj, fi in edges:
        if not (0 <= j < n and 0 <= i < n) or j == i:
            continue
        if (j, i, loc, klass) in seen:
            continue
        seen.add((j, i, loc, klass))
        succ[j].append((i, lag, klass, loc, fj, fi))
        indeg[i] += 1
    indeg0 = indeg[:]
    q = [i for i in range(n) if indeg[i] == 0]
    topo = []
    qi = 0
    while qi < len(q):
        u = q[qi]
        qi += 1
        topo.append(u)
        for v, *_ in succ[u]:
            indeg[v] -= 1
            if indeg[v] == 0:
                q.append(v)
    if len(topo) != n:
        return {"ok": False, "reason": "cycle"}
    es = [0.0] * n
    parent = [None] * n
    for u in topo:
        for v, lag, klass, loc, fj, fi in succ[u]:
            cand = es[u] + lag
            if cand > es[v]:
                es[v] = cand
                parent[v] = (u, klass, loc, fj, fi, lag)
    ef = [es[i] + durs[i] for i in range(n)]
    end_tx = max(range(n), key=lambda i: ef[i]) if n else 0
    l_step = ef[end_tx] if n else 0.0
    path = []
    cur = end_tx
    guard = 0
    while parent[cur] is not None and guard < n + 2:
        u, klass, loc, fj, fi, lag = parent[cur]
        path.append(
            {
                "from": u,
                "to": cur,
                "class": klass,
                "loc": loc_hex(loc),
                "hot": hot_name(loc),
                "w_or_r_frac_from": fj,
                "r_or_w_frac_to": fi,
                "lag_ns": lag,
            }
        )
        cur = u
        guard += 1
    path.reverse()
    # List schedule. Predecessors must be started. Worker is busy until finish.
    pred_left = indeg0[:]
    earliest = [0.0] * n
    prio = ef[:]
    ready = [(-prio[i], i) for i in range(n) if pred_left[i] == 0]
    heapq.heapify(ready)
    wh = [(0.0, w) for w in range(cores)]
    heapq.heapify(wh)
    start = [0.0] * n
    finish = [0.0] * n
    placed = 0
    while placed < n:
        if not ready:
            return {"ok": False, "reason": "stuck", "l_step_ns": l_step, "path": path}
        free_t, w = heapq.heappop(wh)
        _, tx = heapq.heappop(ready)
        st = max(free_t, earliest[tx], 0.0)
        start[tx] = st
        fin = st + durs[tx]
        finish[tx] = fin
        heapq.heappush(wh, (fin, w))
        placed += 1
        for v, lag, *_rest in succ[tx]:
            earliest[v] = max(earliest[v], st + lag)
            pred_left[v] -= 1
            if pred_left[v] == 0:
                heapq.heappush(ready, (-prio[v], v))
    return {
        "ok": True,
        "makespan_ns": max(finish) if finish else 0.0,
        "l_step_ns": l_step,
        "path": path,
        "path_end_tx": end_tx,
    }


def _group_stats(rows: list[dict]) -> list[dict]:
    buckets: dict[tuple, list[dict]] = defaultdict(list)
    for row in rows:
        buckets[row["key"]].append(row)
    out = []
    for key, xs in buckets.items():
        code, selector, key_class, endpoint = key
        if len(xs) < 2:
            continue
        fracs = [r["frac"] for r in xs]
        round_stdevs = [float(r.get("round_stdev") or 0.0) for r in xs]
        out.append(
            {
                "code_hash": code,
                "selector": selector,
                "key_class": key_class,
                "endpoint": endpoint,
                "n": len(fracs),
                "mean": statistics.fmean(fracs),
                "stdev": statistics.pstdev(fracs),
                "min": min(fracs),
                "max": max(fracs),
                "round_stdev_mean": statistics.fmean(round_stdevs),
            }
        )
    out.sort(key=lambda r: -r["n"])
    return out[:40]


def _endpoint(state: dict, tx: int, loc: int, box_name: str) -> dict:
    st = state.get(tx) or {}
    box = (st.get(box_name) or {})
    rec = box.get(loc) or {}
    return {
        "tx": tx,
        "selector": st.get("selector") or "",
        "frac": rec.get("frac"),
        "pc": rec.get("pc"),
        "op_index": rec.get("op_index"),
        "op": rec.get("op"),
        "code_hash": rec.get("code_hash") or "",
        "kind": rec.get("kind"),
        "addr": rec.get("addr") or "",
        "slot": rec.get("slot") or "",
        "round_stdev": rec.get("frac_stdev"),
    }


def join_groups(groups_a: list[dict], groups_b: list[dict]) -> list[dict]:
    """Same (code hash, selector, key class, endpoint) in two blocks."""

    def key(g: dict) -> tuple:
        return (g.get("code_hash") or "", g.get("selector") or "", g.get("key_class") or "", g.get("endpoint") or "")

    other = {key(g): g for g in groups_b}
    rows = []
    for g in groups_a:
        h = other.get(key(g))
        if h is None:
            continue
        rows.append(
            {
                "code_hash": g.get("code_hash") or "",
                "selector": g.get("selector") or "",
                "key_class": g.get("key_class") or "",
                "endpoint": g.get("endpoint") or "",
                "n_a": g["n"],
                "n_b": h["n"],
                "mean_a": g["mean"],
                "mean_b": h["mean"],
                "stdev_a": g["stdev"],
                "stdev_b": h["stdev"],
                "abs_mean_delta": abs(float(g["mean"]) - float(h["mean"])),
                "round_stdev_mean_a": g.get("round_stdev_mean"),
                "round_stdev_mean_b": h.get("round_stdev_mean"),
            }
        )
    rows.sort(key=lambda r: -(min(r["n_a"], r["n_b"])))
    return rows[:40]


def summarize(rounds_raw: list[dict], seq_costs_ns: list[float], cores: int, beneficiary: int) -> dict:
    reduced = [reduce_round(r["txs"]) for r in rounds_raw if r.get("txs")]
    state = median_maps(reduced)
    n = len(seq_costs_ns)
    # Pad state txs that the cost vector has.
    edges_all = build_edges(state, beneficiary, False)
    edges_ex = build_edges(state, beneficiary, True)

    def pack(edge_rows, classes, label):
        scaled = scale_edges(edge_rows, seq_costs_ns, classes)
        sim = schedule(seq_costs_ns, scaled, cores)
        return label, scaled, sim

    variants = {
        "raw_waw": pack(edges_ex, {"raw", "waw"}, "exclude beneficiary+lazy; WAR off"),
        "raw_waw_war": pack(edges_ex, {"raw", "waw", "war"}, "exclude beneficiary+lazy; WAR conservative"),
        "all_raw_waw": pack(edges_all, {"raw", "waw"}, "include beneficiary+lazy; WAR off"),
        "all_raw_waw_war": pack(edges_all, {"raw", "waw", "war"}, "include beneficiary+lazy; WAR conservative"),
    }
    # Distributions on the excluded-beneficiary edge set (the Ideal_tx comparable DAG)
    # plus a full set. Fractions are unscaled w/dur and r/dur.
    def _ns(tx: int, frac: float) -> float:
        if 0 <= tx < len(seq_costs_ns):
            return float(frac) * float(seq_costs_ns[tx])
        return 0.0

    def _touch(tx: int, loc: int, box_name: str) -> dict:
        st = state.get(tx) or {}
        return (st.get(box_name) or {}).get(loc) or {}

    def collect(edge_rows):
        by = defaultdict(lambda: {"w": [], "r": [], "w_ns": [], "r_ns": []})
        spine = defaultdict(lambda: {"w": [], "r": [], "w_ns": [], "r_ns": []})
        hots = defaultdict(lambda: defaultdict(lambda: {"w": [], "r": [], "w_ns": [], "r_ns": []}))
        per_loc = defaultdict(lambda: defaultdict(lambda: {"w": [], "r": [], "w_ns": [], "r_ns": []}))
        loc_n = defaultdict(int)
        groups = []
        spine_edges = []
        counts = defaultdict(int)
        for klass, j, i, loc, fj, fi in edge_rows:
            counts[klass] += 1
            # WAR rows are (reader, writer, r_frac, w_frac). w is the writer offset.
            if klass == "war":
                r_ns = _ns(j, fj)
                w_ns = _ns(i, fi)
                rec_from = _touch(j, loc, "reads")
                rec_to = _touch(i, loc, "writes")
            elif klass == "waw":
                w_ns = _ns(j, fj)
                r_ns = _ns(i, fi)
                rec_from = _touch(j, loc, "writes")
                rec_to = _touch(i, loc, "writes_first")
            else:
                w_ns = _ns(j, fj)
                r_ns = _ns(i, fi)
                rec_from = _touch(j, loc, "writes")
                rec_to = _touch(i, loc, "reads")
            by[klass]["w"].append(fj if klass != "war" else fi)
            by[klass]["r"].append(fi if klass != "war" else fj)
            by[klass]["w_ns"].append(w_ns)
            by[klass]["r_ns"].append(r_ns)
            name = hot_name(loc)
            loc_n[loc] += 1
            per_loc[loc][klass]["w"].append(fj if klass != "war" else fi)
            per_loc[loc][klass]["r"].append(fi if klass != "war" else fj)
            per_loc[loc][klass]["w_ns"].append(w_ns)
            per_loc[loc][klass]["r_ns"].append(r_ns)
            if name == "spine":
                spine[klass]["w"].append(fj if klass != "war" else fi)
                spine[klass]["r"].append(fi if klass != "war" else fj)
                spine[klass]["w_ns"].append(w_ns)
                spine[klass]["r_ns"].append(r_ns)
                if len(spine_edges) < 120:
                    spine_edges.append(
                        {
                            "class": klass,
                            "from": j,
                            "to": i,
                            "loc": loc_hex(loc),
                            "w_frac": fi if klass == "war" else fj,
                            "r_frac": fj if klass == "war" else fi,
                            "w_ns_same_timer": w_ns,
                            "r_ns_same_timer": r_ns,
                            "from_ep": _endpoint(state, j, loc, "reads" if klass == "war" else "writes"),
                            "to_ep": _endpoint(
                                state,
                                i,
                                loc,
                                "writes_first" if klass == "waw" else ("writes" if klass == "war" else "reads"),
                            ),
                        }
                    )
            if name:
                hots[name][klass]["w"].append(fj if klass != "war" else fi)
                hots[name][klass]["r"].append(fi if klass != "war" else fj)
                hots[name][klass]["w_ns"].append(w_ns)
                hots[name][klass]["r_ns"].append(r_ns)
            st_j = state.get(j) or {}
            st_i = state.get(i) or {}
            kc = name or KIND.get(int(rec_from.get("kind", rec_to.get("kind", 0)) or 0), "basic")
            groups.append(
                {
                    "key": (rec_from.get("code_hash") or "", st_j.get("selector") or "", kc, f"{klass}-from"),
                    "frac": fj,
                    "round_stdev": rec_from.get("frac_stdev") or 0.0,
                }
            )
            groups.append(
                {
                    "key": (rec_to.get("code_hash") or "", st_i.get("selector") or "", kc, f"{klass}-to"),
                    "frac": fi,
                    "round_stdev": rec_to.get("frac_stdev") or 0.0,
                }
            )

        def pack_dist(v: dict) -> dict:
            return {
                "w_over_dur": _dist(v["w"]),
                "r_over_dur": _dist(v["r"]),
                "w_ns_same_timer": _dist(v["w_ns"]),
                "r_ns_same_timer": _dist(v["r_ns"]),
            }

        dist = {k: pack_dist(v) for k, v in by.items()}
        spine_dist = {k: pack_dist(v) for k, v in spine.items()}
        hot_dist = {
            name: {k: pack_dist(v) for k, v in kinds.items()} for name, kinds in hots.items()
        }
        top = []
        for loc, n_edges in sorted(loc_n.items(), key=lambda kv: -kv[1])[:8]:
            top.append(
                {
                    "loc": loc_hex(loc),
                    "hot": hot_name(loc),
                    "edges": n_edges,
                    "by_class": {k: pack_dist(v) for k, v in per_loc[loc].items()},
                }
            )
        return dist, spine_dist, hot_dist, _group_stats(groups), spine_edges, dict(counts), top

    dist, spine_dist, hot_dist, groups, spine_edges, counts, top_locs = collect(edges_ex)
    dist_all, _, _, _, _, counts_all, _ = collect(edges_all)

    def enrich_path(path: list[dict]) -> list[dict]:
        out = []
        for edge in path[:24]:
            loc = int(edge["loc"], 16)
            klass = edge["class"]
            j = int(edge["from"])
            i = int(edge["to"])
            row = dict(edge)
            row["from_ep"] = _endpoint(state, j, loc, "reads" if klass == "war" else "writes")
            row["to_ep"] = _endpoint(
                state,
                i,
                loc,
                "writes_first" if klass == "waw" else ("writes" if klass == "war" else "reads"),
            )
            out.append(row)
        return out

    def sim_view(name):
        label, scaled, sim = variants[name]
        ms = sim.get("makespan_ns")
        lstep = sim.get("l_step_ns")
        return {
            "label": label,
            "ok": sim.get("ok"),
            "reason": sim.get("reason"),
            "edges": len(scaled),
            "makespan_ms": None if ms is None else ms / 1e6,
            "l_step_ms": None if lstep is None else lstep / 1e6,
            "tps_ideal_step": (n / (ms / 1e9)) if ms else None,
            "path": enrich_path(sim.get("path") or []),
            "path_end_tx": sim.get("path_end_tx"),
        }

    return {
        "clock": "fractions from OCC workers=1 step trace (inspect record-only); scaled onto same-timer ExecPhase costs",
        "rounds": len(reduced),
        "beneficiary": beneficiary,
        "historical_l_crit_ms_not_this_clock": 1.19,
        "edge_counts_ex_beneficiary_lazy": counts,
        "edge_counts_including_beneficiary": counts_all,
        "distributions_ex_beneficiary_lazy": dist,
        "distributions_including_beneficiary": dist_all,
        "spine_abd6bb397881": spine_dist,
        "spine_edges": spine_edges,
        "hot_locations": hot_dist,
        "top_locations_by_edges": top_locs,
        "offset_groups": groups,
        "ideal_step": {k: sim_view(k) for k in variants},
    }


def self_test() -> None:
    # Two independent txs, C=2.
    d = [10.0, 10.0]
    sim = schedule(d, [], 2)
    assert sim["ok"] and abs(sim["makespan_ns"] - 10) < 1e-6, sim
    # RAW-like lag 6 on one edge, C=2. start1 >= 6, finish 16.
    edges = [(0, 1, 6.0, "raw", 1, 0.8, 0.2)]
    sim = schedule(d, edges, 2)
    assert sim["ok"] and abs(sim["makespan_ns"] - 16) < 1e-6, sim
    assert abs(sim["l_step_ns"] - 16) < 1e-6, sim
    # C=1 still sums the durations when the worker is busy past the lag.
    sim = schedule(d, edges, 1)
    assert sim["ok"] and abs(sim["makespan_ns"] - 20) < 1e-6, sim
    print("step self-test ok")


if __name__ == "__main__":
    self_test()
