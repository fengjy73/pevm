#!/usr/bin/env python3
"""Join flag-off wall samples with the inflation profile and emit per-C JSON.

Headline: TPS_ideal(C) from a critical-path list schedule of the tx DAG,
versus TPS of SEQ / OCC / SpecFence. No untimed warm-up. All ``kind=timed``
rounds enter the median. ``oracle`` rows stay in a side section.

Ideal_C(seq) uses same-timer per-tx costs: successful ``vm.execute``
``ExecPhase.total_ns`` from OCC at workers=1 (the parallel VM, one core).
SEQ ``transact+commit`` is reported as basis A and is not that timer.

Decomposition (milliseconds), per engine E in {occ, sf}:

    wall = F + Ideal_C(seq) + inflation + schedule_loss
    inflation     = Ideal_C(par) - Ideal_C(seq)
    schedule_loss = wall - F - Ideal_C(par)
    LB_C          = max(L_crit, sum(work)/C)
    TPS           = n_tx / wall_seconds
    TPS_ideal     = n_tx / Ideal_C(seq)_seconds
    proximity     = TPS_E / TPS_ideal
"""

from __future__ import annotations

import argparse
import json
import math
import random
import statistics
from collections import defaultdict
from pathlib import Path

import specfence_step_ideal as step_ideal

NBOOT = 10_000
SEED = 0


def load_jsonl(path: Path) -> list[dict]:
    rows = []
    if path is None or not path.exists():
        return rows
    with path.open() as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            rows.append(json.loads(line))
    return rows


def median(xs: list[float]) -> float:
    ys = sorted(xs)
    n = len(ys)
    if n == 0:
        raise ValueError("empty")
    if n % 2 == 1:
        return float(ys[n // 2])
    return 0.5 * (ys[n // 2 - 1] + ys[n // 2])


def bootstrap_median_ci(xs: list[float], seed: int = SEED) -> tuple[float, float]:
    rng = random.Random(seed)
    n = len(xs)
    stats = []
    for _ in range(NBOOT):
        sample = [xs[rng.randrange(n)] for _ in range(n)]
        stats.append(median(sample))
    stats.sort()
    lo = stats[int(0.025 * (NBOOT - 1))]
    hi = stats[int(0.975 * (NBOOT - 1))]
    return lo, hi


def paired_ratio_ci(
    num: list[float], den: list[float], seed: int = SEED
) -> tuple[float, float, float]:
    """Ratio of medians, paired by index, with a bootstrap CI of that ratio."""
    n = min(len(num), len(den))
    point = median(num[:n]) / median(den[:n])
    rng = random.Random(seed)
    stats = []
    for _ in range(NBOOT):
        idx = [rng.randrange(n) for _ in range(n)]
        a = median([num[i] for i in idx])
        b = median([den[i] for i in idx])
        if b > 0:
            stats.append(a / b)
    stats.sort()
    lo = stats[int(0.025 * (len(stats) - 1))]
    hi = stats[int(0.975 * (len(stats) - 1))]
    return point, lo, hi


def geomean(xs: list[float]) -> float | None:
    ys = [x for x in xs if x > 0]
    if not ys:
        return None
    return math.exp(sum(math.log(x) for x in ys) / len(ys))


def sample_block(rows: list[dict], engine: str, kind: str = "timed") -> dict[int, dict]:
    """round -> row, for one engine. Later duplicate rounds overwrite."""
    out = {}
    for row in rows:
        if row.get("meta"):
            continue
        if row.get("engine") != engine or row.get("kind") != kind:
            continue
        if not row.get("ok", False):
            continue
        out[int(row["round"])] = row
    return out


def walls_aligned(rows: list[dict], kind: str = "timed") -> dict[str, list[float]]:
    by = {e: sample_block(rows, e, kind) for e in ("seq", "occ", "sf")}
    rounds = sorted(set(by["seq"]) & set(by["occ"]) & set(by["sf"]))
    return {
        "rounds": rounds,
        "seq": [by["seq"][r]["wall_ms"] for r in rounds],
        "occ": [by["occ"][r]["wall_ms"] for r in rounds],
        "sf": [by["sf"][r]["wall_ms"] for r in rounds],
        "rows": {e: [by[e][r] for r in rounds] for e in by},
    }


def engine_summary(walls: list[float], n_tx: int, seed: int) -> dict:
    if not walls:
        return {"n": 0}
    med = median(walls)
    lo, hi = bootstrap_median_ci(walls, seed)
    tps = [n_tx / (w / 1000.0) for w in walls if w > 0]
    tps_med = median(tps) if tps else None
    tps_lo, tps_hi = bootstrap_median_ci(tps, seed + 1) if tps else (None, None)
    return {
        "n": len(walls),
        "wall_ms": walls,
        "median_ms": med,
        "ci95_ms": [lo, hi],
        "min_ms": min(walls),
        "max_ms": max(walls),
        "tps": tps_med,
        "tps_ci95": [tps_lo, tps_hi],
        "tps_min": min(tps) if tps else None,
        "tps_max": max(tps) if tps else None,
    }


def successful_attempt(attempts: list[dict], tx: int) -> dict | None:
    best = None
    for a in attempts:
        if int(a.get("tx", -1)) != tx or int(a.get("kind", 0)) != 1:
            continue
        if best is None or int(a["inc"]) < int(best["inc"]):
            best = a
    return best


def per_tx_matrix(lines: list[dict], field: str) -> list[list[float | None]]:
    """One list per round. Missing successful exec is None."""
    matrices = []
    for line in lines:
        n = int(line["n_tx"])
        attempts = line.get("attempts") or []
        row: list[float | None] = []
        for tx in range(n):
            a = successful_attempt(attempts, tx)
            row.append(None if a is None else float(a.get(field, 0)))
        matrices.append(row)
    return matrices


def median_vec(matrices: list[list[float | None]]) -> tuple[list[float], int]:
    if not matrices:
        return [], 0
    n = len(matrices[0])
    out = []
    missing = 0
    for tx in range(n):
        xs = [m[tx] for m in matrices if m[tx] is not None]
        if not xs:
            out.append(0.0)
            missing += 1
        else:
            out.append(median(xs))
    return out, missing


def dag_edges(line: dict, include_war: bool) -> tuple[list[tuple[int, int]], dict]:
    """WAW + RAW, plus WAR when requested. Drop beneficiary and lazy writes."""
    n = int(line["n_tx"])
    ben = int(line.get("beneficiary") or 0)
    reads: list[list[int]] = [[] for _ in range(n)]
    writes: list[list[int]] = [[] for _ in range(n)]
    covered = 0
    for tx in range(n):
        a = successful_attempt(line.get("attempts") or [], tx)
        if a is None:
            continue
        covered += 1
        lazy = {int(h) for h in a.get("lazy_writes") or []}
        reads[tx] = [
            int(h)
            for h in (a.get("reads") or [])
            if int(h) != ben and int(h) not in lazy
        ]
        writes[tx] = [
            int(h)
            for h in (a.get("writes") or [])
            if int(h) != ben and int(h) not in lazy
        ]
    writers: dict[int, list[int]] = defaultdict(list)
    readers: dict[int, list[int]] = defaultdict(list)
    for tx in range(n):
        for h in writes[tx]:
            writers[h].append(tx)
        for h in reads[tx]:
            readers[h].append(tx)
    for xs in writers.values():
        xs.sort()
    for xs in readers.values():
        xs.sort()
    edge_set: set[tuple[int, int]] = set()

    def add(u: int, v: int) -> None:
        if u < v:
            edge_set.add((u, v))

    for ws in writers.values():
        for a, b in zip(ws, ws[1:]):
            add(a, b)
    for loc, rs in readers.items():
        ws = writers.get(loc)
        if not ws:
            continue
        for r in rs:
            lower = [w for w in ws if w < r]
            if lower:
                add(lower[-1], r)
            if include_war:
                higher = [w for w in ws if w > r]
                if higher:
                    add(r, higher[0])
    info = {
        "covered_txs": covered,
        "n_tx": n,
        "locations": len(writers),
        "include_war": include_war,
        "edges": len(edge_set),
    }
    return sorted(edge_set), info


def list_schedule(costs: list[float], edges: list[tuple[int, int]], cores: int) -> dict:
    """Non-delay critical-path list schedule. Costs are nanoseconds (float ok)."""
    n = len(costs)
    cores = max(1, cores)
    succ: list[list[int]] = [[] for _ in range(n)]
    indeg = [0] * n
    for u, v in edges:
        if u == v or not (0 <= u < n and 0 <= v < n):
            continue
        succ[u].append(v)
        indeg[v] += 1
    indeg0 = indeg[:]
    q = [i for i in range(n) if indeg[i] == 0]
    topo = []
    qi = 0
    while qi < len(q):
        u = q[qi]
        qi += 1
        topo.append(u)
        for v in succ[u]:
            indeg[v] -= 1
            if indeg[v] == 0:
                q.append(v)
    work = float(sum(costs))
    if len(topo) != n:
        return {
            "ok": False,
            "reason": "dag_cycle",
            "makespan_ns": None,
            "l_crit_ns": None,
            "work_ns": work,
        }
    prio = [0.0] * n
    for u in reversed(topo):
        prio[u] = costs[u] + (max((prio[v] for v in succ[u]), default=0.0))
    l_crit = max(prio) if prio else 0.0
    indeg = indeg0
    ready: list[tuple[float, int]] = []
    for i in range(n):
        if indeg[i] == 0:
            ready.append((-prio[i], i))
    ready.sort()
    # ready is a heap via heapq semantics; use heapq
    import heapq

    heapq.heapify(ready)
    running: list[tuple[float, int]] = []
    free = cores
    time = 0.0
    done = 0
    finish = [0.0] * n
    while done < n:
        while free > 0 and ready:
            _, tx = heapq.heappop(ready)
            end = time + costs[tx]
            heapq.heappush(running, (end, tx))
            free -= 1
        if not running:
            return {
                "ok": False,
                "reason": "stuck",
                "makespan_ns": None,
                "l_crit_ns": l_crit,
                "work_ns": work,
            }
        time = running[0][0]
        while running and running[0][0] <= time + 1e-6:
            end, tx = heapq.heappop(running)
            free += 1
            finish[tx] = end
            done += 1
            for v in succ[tx]:
                indeg[v] -= 1
                if indeg[v] == 0:
                    heapq.heappush(ready, (-prio[v], v))
    makespan = max(finish) if finish else 0.0
    return {
        "ok": True,
        "makespan_ns": makespan,
        "l_crit_ns": l_crit,
        "work_ns": work,
        "ideal_1_ns": work,
    }


def lb_ns(l_crit: float, work: float, cores: int) -> float:
    return max(l_crit, work / max(1, cores))


def ns_ms(ns: float | None) -> float | None:
    if ns is None:
        return None
    return ns / 1e6


def f_samples(lines: list[dict]) -> list[float]:
    out = []
    for line in lines:
        b = line.get("boundary") or {}
        if not b or not b.get("worker_seen"):
            continue
        out.append((float(b.get("f_pre_ns") or 0) + float(b.get("f_post_ns") or 0)) / 1e6)
    return out


def seq_transact_sum(lines: list[dict]) -> list[float]:
    """Basis A: sum of transact+commit ns, milliseconds, one value per round."""
    out = []
    for line in lines:
        seq = line.get("seq") or []
        if not seq:
            continue
        out.append(sum(float(s["transact_ns"]) + float(s["commit_ns"]) for s in seq) / 1e6)
    return out


def load_to_map(block_dir: Path, block: int) -> dict[int, str]:
    path = block_dir / str(block) / "block.json"
    if not path.exists():
        return {}
    with path.open() as f:
        block_json = json.load(f)
    txs = block_json.get("transactions") or []
    out = {}
    for i, tx in enumerate(txs):
        to = tx.get("to") if isinstance(tx, dict) else None
        out[i] = to or "create"
    return out


def phase_source_rank(
    seq_costs: list[float],
    par_costs: list[float],
    seq_phases: dict[str, list[float]],
    par_phases: dict[str, list[float]],
    edges: list[tuple[int, int]],
    cores: int,
    ideal_par_ns: float,
) -> list[dict]:
    fields = ("pre_ns", "interp_ns", "post_ns", "record_ns", "mv_lookup_ns", "mv_scan_ns", "storage_ns")
    ranked = []
    n = min(len(seq_costs), len(par_costs))
    for field in fields:
        sp = seq_phases.get(field) or []
        pp = par_phases.get(field) or []
        if len(sp) < n or len(pp) < n:
            continue
        delta_ns = 0.0
        new_costs = par_costs[:]
        for i in range(n):
            extra = max(0.0, pp[i] - sp[i])
            delta_ns += extra
            new_costs[i] = max(0.0, par_costs[i] - extra)
        sim = list_schedule(new_costs, edges, cores)
        drop = None
        if sim["ok"] and ideal_par_ns is not None:
            drop = ideal_par_ns - sim["makespan_ns"]
        ranked.append(
            {
                "phase": field,
                "thread_extra_ms": delta_ns / 1e6,
                "thread_extra_ms_over_c": (delta_ns / 1e6) / cores,
                "ideal_drop_ms": None if drop is None else drop / 1e6,
            }
        )
    ranked.sort(key=lambda r: (r["ideal_drop_ms"] is None, -(r["ideal_drop_ms"] or 0)))
    return ranked


def self_test() -> None:
    chain = list_schedule([1, 1, 1], [(0, 1), (1, 2)], 1)
    assert chain["ok"] and abs(chain["makespan_ns"] - 3) < 1e-6, chain
    wide = list_schedule([5, 5, 5], [], 3)
    assert wide["ok"] and abs(wide["makespan_ns"] - 5) < 1e-6, wide
    one = list_schedule([2, 3, 4], [(0, 1)], 1)
    assert one["ok"] and abs(one["makespan_ns"] - 9) < 1e-6, one
    crit = list_schedule([10, 1, 1], [(0, 1), (0, 2)], 2)
    assert crit["ok"] and crit["l_crit_ns"] == 11, crit
    assert abs(crit["makespan_ns"] - 11) < 1e-6, crit
    print("self-test ok")


def block_ids(rows: list[dict]) -> list[int]:
    ids = []
    for row in rows:
        if row.get("meta"):
            continue
        b = int(row["block"])
        if b not in ids:
            ids.append(b)
    return ids


def analyze_block(
    block: int,
    wall_rows: list[dict],
    profile_rows: list[dict],
    seq_profile_rows: list[dict],
    cores: int,
    block_dir: Path,
    step_rounds: list[dict] | None = None,
) -> dict:
    wall_rows = [r for r in wall_rows if int(r.get("block", -1)) == block and not r.get("meta")]
    profile_rows = [r for r in profile_rows if int(r.get("block", -1)) == block and not r.get("meta")]
    seq_profile_rows = [
        r for r in seq_profile_rows if int(r.get("block", -1)) == block and not r.get("meta")
    ]
    aligned = walls_aligned(wall_rows, "timed")
    n_tx = int(wall_rows[0]["n_tx"]) if wall_rows else 0
    gas = int(wall_rows[0]["gas_used"]) if wall_rows else 0
    path = {e: (aligned["rows"][e][0].get("path") if aligned["rounds"] else None) for e in ("seq", "occ", "sf")}
    fallback = any(r.get("product_gate_fallback") for r in wall_rows)
    engines = {}
    for i, name in enumerate(("seq", "occ", "sf")):
        engines[name] = engine_summary(aligned[name], n_tx, SEED + i)
        engines[name]["path"] = path[name]
        if aligned["rounds"]:
            row0 = aligned["rows"][name][0]
            engines[name]["est"] = [r["est"] for r in aligned["rows"][name]]
            engines[name]["soft"] = [r["soft"] for r in aligned["rows"][name]]
            engines[name]["occ_picks"] = [r["occ_picks"] for r in aligned["rows"][name]]
            engines[name]["spine_cores_max"] = [r["spine_cores_max"] for r in aligned["rows"][name]]
            engines[name]["phase_exec_ns_median"] = median(
                [float(r["phase_exec_ns"]) for r in aligned["rows"][name]]
            )
            engines[name]["phase_pre_ns_median"] = median(
                [float(r["phase_pre_ns"]) for r in aligned["rows"][name]]
            )
            engines[name]["phase_interp_ns_median"] = median(
                [float(r["phase_interp_ns"]) for r in aligned["rows"][name]]
            )
            engines[name]["phase_post_ns_median"] = median(
                [float(r["phase_post_ns"]) for r in aligned["rows"][name]]
            )
            engines[name]["phase_val_ns_median"] = median(
                [float(r["phase_val_ns"]) for r in aligned["rows"][name]]
            )
            engines[name]["reexec_entries_median"] = median(
                [float(r["reexec_entries"]) for r in aligned["rows"][name]]
            )
            engines[name]["phase_exec_n_median"] = median(
                [float(r["phase_exec_n"]) for r in aligned["rows"][name]]
            )
            _ = row0
    ratios = {}
    ge = False
    if aligned["rounds"]:
        s_occ, s_occ_lo, s_occ_hi = paired_ratio_ci(aligned["seq"], aligned["occ"], SEED + 11)
        s_sf, s_sf_lo, s_sf_hi = paired_ratio_ci(aligned["seq"], aligned["sf"], SEED + 12)
        r, r_lo, r_hi = paired_ratio_ci(aligned["occ"], aligned["sf"], SEED + 13)
        ge = r_lo > 1.5
        ratios = {
            "S_occ": s_occ,
            "S_occ_ci95": [s_occ_lo, s_occ_hi],
            "S_sf": s_sf,
            "S_sf_ci95": [s_sf_lo, s_sf_hi],
            "R": r,
            "R_ci95": [r_lo, r_hi],
            "ge_1_5": ge,
        }
    oracle = walls_aligned(wall_rows, "oracle")
    oracle_sum = None
    if oracle["rounds"]:
        oracle_sum = {
            e: engine_summary(oracle[e], n_tx, SEED + 20) for e in ("seq", "occ", "sf")
        }

    # Profile matrices.
    prof_aligned = {
        e: [sample_block(profile_rows, e)[r] for r in sorted(sample_block(profile_rows, e))]
        for e in ("seq", "occ", "sf")
    }
    seq_src = seq_profile_rows or profile_rows
    seq_occ_lines = [
        sample_block(seq_src, "occ")[r] for r in sorted(sample_block(seq_src, "occ"))
    ]
    seq_costs, seq_missing = median_vec(per_tx_matrix(seq_occ_lines, "total_ns"))
    seq_cpu, _ = median_vec(per_tx_matrix(seq_occ_lines, "cpu_ns"))
    phases = (
        "pre_ns",
        "interp_ns",
        "post_ns",
        "record_ns",
        "mv_lookup_ns",
        "mv_scan_ns",
        "storage_ns",
        "opcode_ns",
        "vmdb_ns",
        "detect_ns",
    )
    seq_phase = {p: median_vec(per_tx_matrix(seq_occ_lines, p))[0] for p in phases}

    dag_line = next((ln for ln in seq_occ_lines if ln.get("attempts")), None)
    edges_war, info_war = ([], {"edges": 0})
    edges_raw, info_raw = ([], {"edges": 0})
    if dag_line is not None:
        edges_war, info_war = dag_edges(dag_line, True)
        edges_raw, info_raw = dag_edges(dag_line, False)

    sched_seq = list_schedule(seq_costs, edges_war, cores) if seq_costs else {"ok": False}
    sched_seq_raw = (
        list_schedule(seq_costs, edges_raw, cores) if seq_costs else {"ok": False}
    )
    basis_a = seq_transact_sum(prof_aligned["seq"])

    def par_pack(engine: str) -> dict:
        lines = prof_aligned[engine]
        costs, missing = median_vec(per_tx_matrix(lines, "total_ns"))
        cpu, _ = median_vec(per_tx_matrix(lines, "cpu_ns"))
        ph = {p: median_vec(per_tx_matrix(lines, p))[0] for p in phases}
        sim = list_schedule(costs, edges_war, cores) if costs else {"ok": False}
        sim_cpu = list_schedule(cpu, edges_war, cores) if cpu else {"ok": False}
        fs = f_samples(lines)
        f_med = median(fs) if fs else None
        # reexec thread: attempts that are not the selected success
        reexec = []
        switches = []
        perf_ok = 0
        perf_n = 0
        instr = []
        misses = []
        for line in lines:
            extra = 0.0
            nvcsw = 0
            nivcsw = 0
            selected = set()
            for tx in range(int(line["n_tx"])):
                a = successful_attempt(line.get("attempts") or [], tx)
                if a is not None:
                    selected.add((tx, int(a["inc"])))
            for a in line.get("attempts") or []:
                key = (int(a["tx"]), int(a["inc"]))
                if key not in selected:
                    extra += float(a.get("total_ns") or 0)
                nvcsw += int(a.get("nvcsw") or 0)
                nivcsw += int(a.get("nivcsw") or 0)
                perf_n += 1
                if a.get("perf_ok"):
                    perf_ok += 1
                    instr.append(float(a.get("instr") or 0))
                    misses.append(float(a.get("cache_miss") or 0))
            reexec.append(extra / 1e6)
            switches.append({"nvcsw": nvcsw, "nivcsw": nivcsw})
        wall = engines[engine].get("median_ms")
        ideal_seq = sched_seq.get("makespan_ns")
        ideal_par = sim.get("makespan_ns")
        decomp = None
        if wall is not None and f_med is not None and ideal_seq is not None and ideal_par is not None:
            ideal_seq_ms = ideal_seq / 1e6
            ideal_par_ms = ideal_par / 1e6
            inflation = ideal_par_ms - ideal_seq_ms
            schedule = wall - f_med - ideal_par_ms
            gap = wall - ideal_seq_ms - f_med
            decomp = {
                "wall_ms": wall,
                "F_ms": f_med,
                "ideal_seq_ms": ideal_seq_ms,
                "ideal_par_ms": ideal_par_ms,
                "inflation_ms": inflation,
                "schedule_loss_ms": schedule,
                "inflation_fraction_of_gap": (inflation / gap) if gap else None,
                "schedule_fraction_of_gap": (schedule / gap) if gap else None,
                "wall_source": "flag-off median",
                "F_source": "profile boundary f_pre+f_post median",
                "ideal_source": "profile per-tx ExecPhase, DAG from OCC workers=1",
            }
        tps_e = engines[engine].get("tps")
        tps_ideal = None
        if ideal_seq:
            tps_ideal = n_tx / (ideal_seq / 1e9)
        prox = (tps_e / tps_ideal) if tps_e and tps_ideal else None
        sources = []
        if costs and sim.get("ok") and seq_costs:
            sources = phase_source_rank(
                seq_costs, costs, seq_phase, ph, edges_war, cores, sim["makespan_ns"]
            )
        # per-tx inflation distribution
        ratios = []
        dom = []
        to_map = load_to_map(block_dir, block)
        for i in range(min(len(costs), len(seq_costs))):
            if seq_costs[i] > 0 and costs[i] > 0:
                ratios.append(costs[i] / seq_costs[i])
                dom.append((costs[i] - seq_costs[i], i, to_map.get(i, "?")))
        dom.sort(reverse=True)
        by_to: dict[str, float] = defaultdict(float)
        for extra, _i, to in dom:
            by_to[to] += extra
        top_to = sorted(by_to.items(), key=lambda kv: -kv[1])[:8]
        dist = None
        if ratios:
            rs = sorted(ratios)
            dist = {
                "n": len(rs),
                "p50": rs[len(rs) // 2],
                "p90": rs[min(len(rs) - 1, int(0.9 * (len(rs) - 1)))],
                "max": rs[-1],
                "missing_txs": missing,
            }
        return {
            "per_tx_missing": missing,
            "sum_work_ms": (sum(costs) / 1e6) if costs else None,
            "sum_cpu_ms": (sum(cpu) / 1e6) if cpu else None,
            "ideal_par_ms": ns_ms(sim.get("makespan_ns")),
            "ideal_par_cpu_ms": ns_ms(sim_cpu.get("makespan_ns")),
            "ideal_par_ok": sim.get("ok"),
            "F_ms": f_med,
            "F_samples_ms": fs,
            "reexec_thread_ms_median": median(reexec) if reexec else None,
            "switches_median": {
                "nvcsw": median([float(s["nvcsw"]) for s in switches]) if switches else None,
                "nivcsw": median([float(s["nivcsw"]) for s in switches]) if switches else None,
            },
            "perf_ok_frac": (perf_ok / perf_n) if perf_n else None,
            "instr_sum_median_note": "sum of per-tx deltas is inside attempts; not aggregated here",
            "instr_per_success_median": median(instr) if instr else None,
            "cache_miss_per_success_median": median(misses) if misses else None,
            "decomposition": decomp,
            "tps_ideal": tps_ideal,
            "proximity": prox,
            "phase_sources": sources[:8],
            "inflation_dist": dist,
            "top_tx_extra_ns": [
                {"tx": i, "to": to, "extra_ns": extra} for extra, i, to in dom[:8]
            ],
            "top_contracts_extra_ms": [
                {"to": to, "extra_ms": extra / 1e6} for to, extra in top_to
            ],
            "profile_wall_median_ms": median([ln["wall_ms"] for ln in lines]) if lines else None,
        }

    par = {e: par_pack(e) for e in ("occ", "sf")}
    # SEQ basis A and F_seq from transact when the sequential path recorded it.
    f_seq = None
    if basis_a and engines["seq"].get("median_ms") is not None:
        # Use profile wall if present, else flag-off, minus basis A median.
        seq_lines = prof_aligned["seq"]
        seq_wall = median([ln["wall_ms"] for ln in seq_lines]) if seq_lines else engines["seq"]["median_ms"]
        f_seq = seq_wall - median(basis_a)

    l_crit = sched_seq.get("l_crit_ns")
    work = sched_seq.get("work_ns")
    out = {
        "block": block,
        "n_tx": n_tx,
        "gas_used": gas,
        "workers": cores,
        "product_gate_fallback": fallback,
        "paths": path,
        "engines": engines,
        "ratios": ratios,
        "oracle": oracle_sum,
        "same_timer": {
            "definition": "OCC workers=1 successful ExecPhase.total_ns median per tx",
            "source_workers": 1 if seq_profile_rows else cores,
            "sum_work_ms": (sum(seq_costs) / 1e6) if seq_costs else None,
            "sum_cpu_ms": (sum(seq_cpu) / 1e6) if seq_cpu else None,
            "missing_txs": seq_missing,
            "l_crit_ms": ns_ms(l_crit),
            "l_crit_raw_waw_ms": ns_ms(sched_seq_raw.get("l_crit_ns")),
            "ideal_seq_ms": ns_ms(sched_seq.get("makespan_ns")),
            "ideal_seq_raw_waw_ms": ns_ms(sched_seq_raw.get("makespan_ns")),
            "ideal_seq_ok": sched_seq.get("ok"),
            "lb_ms": ns_ms(lb_ns(l_crit or 0, work or 0, cores)) if l_crit is not None else None,
            "dag_war": info_war,
            "dag_raw_waw": info_raw,
            "basis_a_transact_commit_ms_median": median(basis_a) if basis_a else None,
            "basis_a_n": len(basis_a),
            "F_seq_ms": f_seq,
        },
        "parallel": par,
        "ge_1_5": ge,
    }
    # TPS ideal once, from same-timer ideal (tx-level DAG).
    if out["same_timer"]["ideal_seq_ms"]:
        out["tps_ideal"] = n_tx / (out["same_timer"]["ideal_seq_ms"] / 1000.0)
        out["tps_ideal_tx"] = out["tps_ideal"]
        for e in ("occ", "sf"):
            tps_e = engines[e].get("tps")
            out["parallel"][e]["tps_ideal"] = out["tps_ideal"]
            out["parallel"][e]["tps_ideal_tx"] = out["tps_ideal"]
            out["parallel"][e]["proximity"] = (
                tps_e / out["tps_ideal"] if tps_e and out["tps_ideal"] else None
            )
    step_rows = [r for r in (step_rounds or []) if int(r.get("block", -1)) == block and r.get("txs")]
    if step_rows and seq_costs:
        ben = 0
        for r in step_rows:
            if r.get("beneficiary"):
                ben = int(r["beneficiary"])
        step = step_ideal.summarize(step_rows, seq_costs, cores, ben)
        raw = (step.get("ideal_step") or {}).get("raw_waw") or {}
        out["step"] = step
        out["tps_ideal_step"] = raw.get("tps_ideal_step")
        out["ideal_step_ms"] = raw.get("makespan_ms")
        out["l_step_ms"] = raw.get("l_step_ms")
        tps_step = raw.get("tps_ideal_step")
        for e in ("occ", "sf"):
            tps_e = engines[e].get("tps")
            out["parallel"][e]["tps_ideal_step"] = tps_step
            out["parallel"][e]["proximity_step"] = (
                tps_e / tps_step if tps_e and tps_step else None
            )
        out["ideal_step_vs_tx"] = {
            "ideal_tx_ms": out["same_timer"].get("ideal_seq_ms"),
            "ideal_tx_raw_waw_ms": out["same_timer"].get("ideal_seq_raw_waw_ms"),
            "lb_ms": out["same_timer"].get("lb_ms"),
            "l_crit_tx_ms": out["same_timer"].get("l_crit_ms"),
            "l_crit_tx_raw_waw_ms": out["same_timer"].get("l_crit_raw_waw_ms"),
            "ideal_step_raw_waw_ms": raw.get("makespan_ms"),
            "l_step_raw_waw_ms": raw.get("l_step_ms"),
            "ideal_step_raw_waw_war_ms": ((step.get("ideal_step") or {}).get("raw_waw_war") or {}).get("makespan_ms"),
            "l_step_raw_waw_war_ms": ((step.get("ideal_step") or {}).get("raw_waw_war") or {}).get("l_step_ms"),
            "ideal_step_all_raw_waw_ms": ((step.get("ideal_step") or {}).get("all_raw_waw") or {}).get("makespan_ms"),
            "tps_ideal_tx": out.get("tps_ideal_tx"),
            "tps_ideal_step": tps_step,
            "tps_occ": engines["occ"].get("tps"),
            "tps_sf": engines["sf"].get("tps"),
            "historical_l_crit_ms_not_this_clock": 1.19,
        }
    return out


def markdown_block(rep: dict) -> str:
    lines = []
    b = rep["block"]
    lines.append(f"### block {b}  workers={rep['workers']}  n_tx={rep['n_tx']}  gas={rep['gas_used']}")
    lines.append("")
    lines.append("| engine | path | median ms | 95% CI | min | max | TPS |")
    lines.append("| --- | --- | ---: | --- | ---: | ---: | ---: |")
    for e in ("seq", "occ", "sf"):
        s = rep["engines"][e]
        if not s.get("n"):
            continue
        ci = s["ci95_ms"]
        lines.append(
            f"| {e} | `{s.get('path')}` | {s['median_ms']:.3f} | [{ci[0]:.3f}, {ci[1]:.3f}] | {s['min_ms']:.3f} | {s['max_ms']:.3f} | {s['tps']:.0f} |"
        )
    r = rep.get("ratios") or {}
    if r:
        lines.append("")
        lines.append(
            f"S_occ={r['S_occ']:.3f} CI[{r['S_occ_ci95'][0]:.3f}, {r['S_occ_ci95'][1]:.3f}]  "
            f"S_sf={r['S_sf']:.3f} CI[{r['S_sf_ci95'][0]:.3f}, {r['S_sf_ci95'][1]:.3f}]  "
            f"R={r['R']:.3f} CI[{r['R_ci95'][0]:.3f}, {r['R_ci95'][1]:.3f}]  ge_1_5={r['ge_1_5']}"
        )
    st = rep["same_timer"]
    lines.append("")
    lines.append(
        f"Ideal_seq={st.get('ideal_seq_ms')} ms  L_crit={st.get('l_crit_ms')} ms  "
        f"LB={st.get('lb_ms')} ms  Σwork_same_timer={st.get('sum_work_ms')} ms  "
        f"basisA_transact={st.get('basis_a_transact_commit_ms_median')} ms  "
        f"TPS_ideal_tx={rep.get('tps_ideal_tx') or rep.get('tps_ideal')}  "
        f"TPS_ideal_step={rep.get('tps_ideal_step')}  "
        f"Ideal_step={rep.get('ideal_step_ms')} ms  L_step={rep.get('l_step_ms')} ms"
    )
    lines.append("")
    lines.append("| engine | F | Ideal_par | inflation | schedule | proximity | profile wall |")
    lines.append("| --- | ---: | ---: | ---: | ---: | ---: | ---: |")
    for e in ("occ", "sf"):
        d = (rep["parallel"][e].get("decomposition") or {})
        lines.append(
            "| {e} | {F} | {ip} | {inf} | {sch} | {prox} | {pw} |".format(
                e=e,
                F=_fmt(d.get("F_ms")),
                ip=_fmt(d.get("ideal_par_ms")),
                inf=_fmt(d.get("inflation_ms")),
                sch=_fmt(d.get("schedule_loss_ms")),
                prox=_fmt(rep["parallel"][e].get("proximity")),
                pw=_fmt(rep["parallel"][e].get("profile_wall_median_ms")),
            )
        )
    return "\n".join(lines)


def _fmt(v) -> str:
    if v is None:
        return ""
    if isinstance(v, float):
        return f"{v:.3f}"
    return str(v)


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--self-test", action="store_true")
    ap.add_argument("--wall", type=Path)
    ap.add_argument("--profile", type=Path)
    ap.add_argument("--seq-profile", type=Path, help="workers=1 profile jsonl for same-timer costs and the DAG")
    ap.add_argument("--cores", type=int)
    ap.add_argument("--out", type=Path)
    ap.add_argument("--block-dir", type=Path, default=Path("data/ethereum/blocks"))
    ap.add_argument("--curves", type=Path, help="directory of C*.json to summarize")
    ap.add_argument("--step-trace", type=Path, help="OCC workers=1 step-trace jsonl")
    args = ap.parse_args()
    if args.self_test:
        self_test()
        if args.wall is None:
            return
    if args.curves:
        files = sorted(args.curves.glob("C*.json"))
        rows = []
        for f in files:
            doc = json.loads(f.read_text())
            for b in doc.get("blocks", []):
                rows.append(
                    {
                        "c": doc.get("workers"),
                        "cpus": doc.get("cpus"),
                        "block": b["block"],
                        "n_tx": b["n_tx"],
                        "tps_seq": b["engines"]["seq"].get("tps"),
                        "tps_occ": b["engines"]["occ"].get("tps"),
                        "tps_sf": b["engines"]["sf"].get("tps"),
                        "tps_ideal": b.get("tps_ideal"),
                        "tps_ideal_tx": b.get("tps_ideal_tx") or b.get("tps_ideal"),
                        "tps_ideal_step": b.get("tps_ideal_step"),
                        "ideal_step_ms": b.get("ideal_step_ms"),
                        "l_step_ms": b.get("l_step_ms"),
                        "l_crit_ms": b["same_timer"].get("l_crit_ms"),
                        "proximity_occ": b["parallel"]["occ"].get("proximity"),
                        "proximity_sf": b["parallel"]["sf"].get("proximity"),
                        "proximity_occ_step": b["parallel"]["occ"].get("proximity_step"),
                        "proximity_sf_step": b["parallel"]["sf"].get("proximity_step"),
                        "S_occ": (b.get("ratios") or {}).get("S_occ"),
                        "S_sf": (b.get("ratios") or {}).get("S_sf"),
                        "R": (b.get("ratios") or {}).get("R"),
                        "R_ci95": (b.get("ratios") or {}).get("R_ci95"),
                        "ge_1_5": b.get("ge_1_5"),
                        "ideal_seq_ms": b["same_timer"].get("ideal_seq_ms"),
                        "lb_ms": b["same_timer"].get("lb_ms"),
                    }
                )
        out = {"curves": rows, "ge_1_5": any(r["ge_1_5"] for r in rows)}
        text = json.dumps(out, indent=2)
        if args.out:
            args.out.write_text(text + "\n")
        print(text)
        return
    if not args.wall or not args.out or not args.cores:
        ap.error("--wall --cores --out are required")
    wall = load_jsonl(args.wall)
    profile = load_jsonl(args.profile) if args.profile else []
    seq_profile = load_jsonl(args.seq_profile) if args.seq_profile else []
    step_rounds = load_jsonl(args.step_trace) if args.step_trace else []
    meta = next((r for r in wall if r.get("meta")), {})
    blocks = [
        analyze_block(b, wall, profile, seq_profile, args.cores, args.block_dir, step_rounds)
        for b in block_ids(wall)
    ]
    if len(blocks) >= 2 and all(b.get("step") for b in blocks[:2]):
        doc_groups = {
            "block_a": blocks[0]["block"],
            "block_b": blocks[1]["block"],
            "groups": step_ideal.join_groups(
                blocks[0]["step"].get("offset_groups") or [],
                blocks[1]["step"].get("offset_groups") or [],
            ),
        }
    else:
        doc_groups = None
    # Overall across focus blocks: ratio of sums of median walls. Geomean secondary.
    overall = {}
    if len(blocks) >= 1:
        for e, key in (("occ", "S_occ"), ("sf", "S_sf")):
            seq_sum = sum(b["engines"]["seq"]["median_ms"] for b in blocks if b["engines"]["seq"].get("n"))
            e_sum = sum(b["engines"][e]["median_ms"] for b in blocks if b["engines"][e].get("n"))
            overall[key + "_overall"] = (seq_sum / e_sum) if e_sum else None
            overall[key + "_geomean"] = geomean(
                [(b.get("ratios") or {}).get(key) for b in blocks if (b.get("ratios") or {}).get(key)]
            )
        occ_sum = sum(b["engines"]["occ"]["median_ms"] for b in blocks if b["engines"]["occ"].get("n"))
        sf_sum = sum(b["engines"]["sf"]["median_ms"] for b in blocks if b["engines"]["sf"].get("n"))
        overall["R_overall"] = (occ_sum / sf_sum) if sf_sum else None
        overall["R_geomean"] = geomean(
            [(b.get("ratios") or {}).get("R") for b in blocks if (b.get("ratios") or {}).get("R")]
        )
        overall["fraction_S_occ_lt_1"] = None
        overall["fraction_S_sf_lt_1"] = None
        if blocks:
            overall["fraction_S_occ_lt_1"] = sum(
                1 for b in blocks if (b.get("ratios") or {}).get("S_occ", 1) < 1
            ) / len(blocks)
            overall["fraction_S_sf_lt_1"] = sum(
                1 for b in blocks if (b.get("ratios") or {}).get("S_sf", 1) < 1
            ) / len(blocks)
        overall["note"] = "S_overall and R_overall are ratios of sums of per-block median walls. Geomean is secondary. Not a mean of ratios."
    doc = {
        "workers": args.cores,
        "cpus": (meta.get("pin_cpus") if meta else None),
        "host_model": meta.get("model") if meta else None,
        "k": meta.get("k") if meta else None,
        "warmup": 0,
        "ge_1_5": any(b.get("ge_1_5") for b in blocks),
        "overall": overall,
        "offset_groups_across_blocks": doc_groups,
        "blocks": blocks,
    }
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(doc, indent=2) + "\n")
    print(f"wrote {args.out} ge_1_5={doc['ge_1_5']}")
    for b in blocks:
        print(markdown_block(b))
        print()


if __name__ == "__main__":
    main()
