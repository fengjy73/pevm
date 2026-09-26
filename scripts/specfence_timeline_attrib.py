#!/usr/bin/env python3
"""Attribute one SpecFence timeline against the ideal list schedule.

Spans are nanoseconds from the timeline base. Transaction stamps are stored
as nanoseconds plus one; zero means the transaction never reached that point.

The ideal DAG is the true read-from graph on this run:
  RAW  reader after the writer it consumed
  WAW  later non-lazy writer after the previous writer (the writer chain)
  sender  nonce chain
WAR is reported and scheduled only in the conservative column. Lazy beneficiary
updates are not edges. Primary Ideal_C does not wait on them.
"""

from __future__ import annotations

import argparse
import json
import sys
from collections import defaultdict
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import specfence_inflation_report as rep  # noqa: E402

EXEC, INLINE, VALIDATE, IDLE, SPIN, PARK, QUEUE, POST = range(1, 9)
ADMIT, CLASS, ARMED, NONCE, ESTIMATE, UNARMED, OTHER, RESCAN, LAZY, SETUP = range(1, 11)
REASON = {
    ADMIT: "admission",
    CLASS: "class_head",
    ARMED: "armed_read_until_commit",
    NONCE: "nonce_sender",
    ESTIMATE: "estimate",
    UNARMED: "unarmed_cost",
    OTHER: "other",
    RESCAN: "rescan",
    LAZY: "lazy_eval",
    SETUP: "setup",
}
CYC_NAME = ["coord", "sched", "publish", "record", "pre", "writeset", "mark"]
ORIGIN_STORAGE = 0xFFFFFFFF


def dec(v: int) -> int | None:
    if v == 0:
        return None
    return int(v) - 1


def ms(ns: float | None) -> float:
    if ns is None:
        return 0.0
    return ns / 1e6


def load_rows(path: Path) -> list[dict]:
    rows = []
    with path.open() as f:
        for line in f:
            line = line.strip()
            if line:
                rows.append(json.loads(line))
    return rows


def tx_table(row: dict) -> list[dict]:
    out = []
    for i, raw in enumerate(row["txs"]):
        start, end, commit, worker, cls = raw
        out.append(
            {
                "tx": i,
                "start": dec(start),
                "end": dec(end),
                "commit": dec(commit),
                "worker": None if worker >= 0xFFFFFFFF else int(worker),
                "class": None if cls >= 65535 else int(cls),
            }
        )
    return out


def spans_of(row: dict) -> list[dict]:
    out = []
    for raw in row["spans"]:
        kind, reason, tx, pred, loc, cls, t0, t1 = raw[:8]
        worker = int(raw[8]) if len(raw) > 8 else 0
        out.append(
            {
                "kind": int(kind),
                "reason": int(reason),
                "tx": int(tx),
                "pred": int(pred),
                "loc": int(loc),
                "class": None if int(cls) >= 65535 else int(cls),
                "t0": int(t0),
                "t1": int(t1),
                "ns": max(0, int(t1) - int(t0)),
                "worker": worker,
            }
        )
    return out


def build_dag(row: dict) -> dict:
    ben = int(row["beneficiary"])
    writers: dict[int, list[tuple[int, bool]]] = defaultdict(list)
    for tx, loc, lazy in row["writes"]:
        writers[int(loc)].append((int(tx), bool(lazy)))
    raw: set[tuple[int, int]] = set()
    waw: set[tuple[int, int]] = set()
    war: set[tuple[int, int]] = set()
    lazy_waw = 0
    for loc, ws in writers.items():
        if loc == ben:
            continue
        ordered = sorted({tx for tx, _lazy in ws})
        lazy_only = all(lazy for _tx, lazy in ws)
        if lazy_only:
            lazy_waw += max(0, len(ordered) - 1)
            continue
        for a, b in zip(ordered, ordered[1:]):
            waw.add((a, b))
    reads_by_loc: dict[int, list[tuple[int, int]]] = defaultdict(list)
    for tx, loc, origin in row["reads"]:
        loc = int(loc)
        tx = int(tx)
        origin = int(origin)
        if loc == ben:
            continue
        reads_by_loc[loc].append((tx, origin))
        if origin != ORIGIN_STORAGE and origin < tx:
            raw.add((origin, tx))
    for loc, reads in reads_by_loc.items():
        ordered = sorted({tx for tx, lazy in writers.get(loc, []) if not lazy})
        if not ordered:
            continue
        for tx, _origin in reads:
            later = [w for w in ordered if w > tx]
            if later:
                war.add((tx, later[0]))
    sender = {(int(a), int(b)) for a, b in row["sender"] if int(a) < int(b)}
    true = raw | waw | sender
    return {
        "raw": sorted(raw),
        "waw": sorted(waw),
        "war": sorted(war),
        "sender": sorted(sender),
        "true": sorted(true),
        "lazy_waw": lazy_waw,
        "n_raw": len(raw),
        "n_waw": len(waw),
        "n_war": len(war),
        "n_sender": len(sender),
    }


def costs_from(txs: list[dict], spans: list[dict]) -> list[float]:
    """Successful-attempt duration, with in-interpreter waits removed."""
    n = len(txs)
    inline = [0] * n
    for sp in spans:
        if sp["kind"] != INLINE:
            continue
        tx = txs[sp["tx"]] if sp["tx"] < n else None
        if tx is None or tx["start"] is None or tx["end"] is None:
            continue
        a = max(sp["t0"], tx["start"])
        b = min(sp["t1"], tx["end"])
        if b > a:
            inline[sp["tx"]] += b - a
    out = []
    for i, tx in enumerate(txs):
        if tx["start"] is None or tx["end"] is None or tx["end"] < tx["start"]:
            out.append(0.0)
        else:
            out.append(float(max(0, tx["end"] - tx["start"] - inline[i])))
    return out


def sum_kind(spans: list[dict], kind: int, reason: int | None = None) -> int:
    total = 0
    for sp in spans:
        if sp["kind"] != kind:
            continue
        if reason is not None and sp["reason"] != reason:
            continue
        total += sp["ns"]
    return total


def worker_exec_spans(spans: list[dict], n_workers: int) -> list[int]:
    """Per-worker interpreter occupancy. Inline spans sit inside exec spans."""
    acc = [0] * n_workers
    for sp in spans:
        if sp["kind"] != EXEC:
            continue
        # Spans are pushed by the worker that ran them; the tx stamp records
        # the successful worker. Failed attempts have no tx worker yet, so
        # attribute by walking is not available. Use the span order's worker
        # implicitly via a side channel: we stored worker only on the tx.
        # Recompute from the successful stamp below. Here sum globally.
        acc[0] += sp["ns"]
    return acc


def per_worker(spans: list[dict], n_workers: int) -> list[dict]:
    keys = ("exec", "inline", "validate", "idle", "spin", "park", "queue")
    buckets = [{k: 0 for k in keys} for _ in range(max(1, n_workers))]
    kind_key = {
        EXEC: "exec",
        INLINE: "inline",
        VALIDATE: "validate",
        IDLE: "idle",
        SPIN: "spin",
        PARK: "park",
        QUEUE: "queue",
    }
    for sp in spans:
        key = kind_key.get(sp["kind"])
        if key is None:
            continue
        w = sp.get("worker", 0)
        if w >= len(buckets):
            continue
        # Inline time is already inside exec. Keep it as a subset, not extra CPU.
        buckets[w][key] += sp["ns"]
    return buckets


def park_rows(spans: list[dict]) -> list[dict]:
    return [sp for sp in spans if sp["kind"] == PARK]


def merge_coverage(intervals: list[tuple[int, int]]) -> int:
    """Wall-clock union of [t0, t1). Overlapping residences count once."""
    if not intervals:
        return 0
    ordered = sorted((a, b) for a, b in intervals if b > a)
    if not ordered:
        return 0
    total = 0
    cur_a, cur_b = ordered[0]
    for a, b in ordered[1:]:
        if a <= cur_b:
            cur_b = max(cur_b, b)
        else:
            total += cur_b - cur_a
            cur_a, cur_b = a, b
    total += cur_b - cur_a
    return total


def group_parks(parks: list[dict]) -> list[dict]:
    box: dict[tuple, int] = defaultdict(int)
    cnt: dict[tuple, int] = defaultdict(int)
    spans: dict[tuple, list[tuple[int, int]]] = defaultdict(list)
    for sp in parks:
        key = (sp["reason"], sp["loc"], sp["class"] if sp["class"] is not None else -1)
        box[key] += sp["ns"]
        cnt[key] += 1
        spans[key].append((sp["t0"], sp["t1"]))
    rows = []
    for (reason, loc, cls), ns in box.items():
        rows.append(
            {
                "reason": REASON.get(reason, str(reason)),
                "reason_id": reason,
                "loc": f"{loc:016x}" if loc else "-",
                "class": cls,
                "ns": ns,
                "coverage_ns": merge_coverage(spans[(reason, loc, cls)]),
                "n": cnt[(reason, loc, cls)],
            }
        )
    # fix count key: class -1 was stored as -1 in both
    rows.sort(key=lambda r: -r["coverage_ns"])
    return rows


def park_coverage(parks: list[dict]) -> dict[str, dict]:
    """Residence sum can exceed wall × threads: a parked tx keeps its clock
    while the worker runs something else, and many txs overlap.

    Coverage is the union of those intervals and cannot exceed the wall.
    """
    by_reason: dict[str, list[tuple[int, int]]] = defaultdict(list)
    residence: dict[str, int] = defaultdict(int)
    counts: dict[str, int] = defaultdict(int)
    for sp in parks:
        name = REASON.get(sp["reason"], str(sp["reason"]))
        by_reason[name].append((sp["t0"], sp["t1"]))
        residence[name] += sp["ns"]
        counts[name] += 1
    out = {}
    for name, intervals in by_reason.items():
        out[name] = {
            "n": counts[name],
            "residence_ms": ms(residence[name]),
            "coverage_ms": ms(merge_coverage(intervals)),
        }
    return out


def armed_excess(parks: list[dict], txs: list[dict]) -> dict:
    total = 0
    after = 0
    before = 0
    n = 0
    whole_after = 0
    for sp in parks:
        if sp["reason"] != ARMED:
            continue
        pred = sp["pred"]
        if pred >= len(txs) or txs[pred]["end"] is None:
            continue
        n += 1
        total += sp["ns"]
        prod_end = txs[pred]["end"]
        # Time the reader still waited after the producer finished executing.
        after_ns = max(0, sp["t1"] - max(sp["t0"], prod_end))
        after += after_ns
        before += sp["ns"] - after_ns
        if prod_end <= sp["t0"]:
            whole_after += sp["ns"]
    return {
        "n": n,
        "park_ms": ms(total),
        "after_producer_ms": ms(after),
        "before_producer_ms": ms(before),
        "already_finished_ms": ms(whole_after),
        "after_frac": (after / total) if total else 0.0,
    }


def class_false(parks: list[dict], true_edges: set[tuple[int, int]]) -> dict:
    total = 0
    false_ns = 0
    n = 0
    n_false = 0
    for sp in parks:
        if sp["reason"] != CLASS:
            continue
        n += 1
        total += sp["ns"]
        if (sp["pred"], sp["tx"]) not in true_edges:
            n_false += 1
            false_ns += sp["ns"]
    return {
        "n": n,
        "park_ms": ms(total),
        "no_true_edge_n": n_false,
        "no_true_edge_ms": ms(false_ns),
        "false_frac": (false_ns / total) if total else 0.0,
    }


def commit_lag(txs: list[dict]) -> dict:
    lags = []
    for tx in txs:
        if tx["end"] is None or tx["commit"] is None:
            continue
        lags.append(tx["commit"] - tx["end"])
    if not lags:
        return {"n": 0}
    lags.sort()

    def pct(p: float) -> float:
        i = min(len(lags) - 1, max(0, int(round(p * (len(lags) - 1)))))
        return lags[i]

    return {
        "n": len(lags),
        "sum_ms": ms(sum(lags)),
        "p50_us": pct(0.5) / 1e3,
        "p90_us": pct(0.9) / 1e3,
        "max_ms": ms(lags[-1]),
    }


def critical_path(txs: list[dict], parks: list[dict], true_edges: list[tuple[int, int]], wall: int) -> list[dict]:
    """Walk backward from the last commit. Segments abut and should sum to the wall."""
    n = len(txs)
    preds: list[list[int]] = [[] for _ in range(n)]
    for u, v in true_edges:
        if 0 <= u < n and 0 <= v < n:
            preds[v].append(u)
    by_tx: dict[int, list[dict]] = defaultdict(list)
    for sp in parks:
        by_tx[sp["tx"]].append(sp)

    segs: list[dict] = []
    if n == 0:
        return segs
    cursor = wall
    # Post-commit tail (rescan, lazy) sits after the last commit.
    last_c = txs[-1]["commit"]
    if last_c is not None and cursor > last_c:
        segs.append({"label": "post", "ns": cursor - last_c, "tx": n - 1})
        cursor = last_c

    i = n - 1
    steps = 0
    while i >= 0 and steps < n * 8 and cursor > 0:
        steps += 1
        tx = txs[i]
        if tx["commit"] is None or tx["end"] is None or tx["start"] is None:
            break
        prev_c = txs[i - 1]["commit"] if i else 0
        prev_c = 0 if prev_c is None else prev_c
        if tx["end"] >= prev_c:
            gap = tx["commit"] - tx["end"]
            if gap > 0 and tx["commit"] <= cursor:
                segs.append({"label": "commit_after_exec", "ns": min(gap, cursor - tx["end"]), "tx": i})
            segs.append({"label": "exec", "ns": tx["end"] - tx["start"], "tx": i})
            cursor = tx["start"]
            ready = 0
            ready_tx = None
            for p in preds[i]:
                end = txs[p]["end"]
                if end is not None and end >= ready and end <= tx["start"]:
                    ready = end
                    ready_tx = p
            park = None
            best = -1
            for sp in by_tx.get(i, []):
                if sp["t1"] <= tx["start"] + 2_000 and sp["t0"] >= best:
                    park = sp
                    best = sp["t0"]
            if park is not None and park["pred"] < n:
                pred = park["pred"]
                prod_end = txs[pred]["end"] or 0
                label = REASON.get(park["reason"], "park")
                after = max(0, min(tx["start"], park["t1"]) - max(park["t0"], prod_end, ready))
                before = max(0, tx["start"] - ready - after)
                if after:
                    segs.append({"label": label + "_after_producer", "ns": after, "tx": i, "pred": pred})
                if before:
                    segs.append({"label": label + "_until_producer", "ns": before, "tx": i, "pred": pred})
                cursor = max(0, tx["start"] - after - before)
                # Continue from the event that released the wait.
                if park["reason"] in (ARMED, NONCE, ESTIMATE) and (txs[pred]["commit"] or 0) >= prod_end:
                    i = pred
                    # Land on that commit. The loop's commit handling runs next.
                    if txs[i]["commit"] is not None and cursor > txs[i]["commit"]:
                        segs.append(
                            {
                                "label": "schedule_gap",
                                "ns": cursor - txs[i]["commit"],
                                "tx": i,
                            }
                        )
                        cursor = txs[i]["commit"]
                    continue
                i = pred
                if txs[i]["end"] is not None and cursor > txs[i]["end"]:
                    segs.append({"label": "schedule_gap", "ns": cursor - txs[i]["end"], "tx": i})
                    cursor = txs[i]["end"]
                # Re-enter as if we still need to account this exec: set cursor at end
                # and pretend the commit walk should take the exec branch.
                # Force exec by using a synthetic commit equal to end.
                continue_at_exec = True
                if continue_at_exec:
                    # Account exec on the next iteration by bumping commit view:
                    # set i and let the loop see end >= prev. Easiest: jump into exec now.
                    segs.append({"label": "exec", "ns": (txs[i]["end"] or 0) - (txs[i]["start"] or 0), "tx": i})
                    cursor = txs[i]["start"] or 0
                    # Do not loop the same tx forever: move to its true pred or previous commit.
                    nxt = None
                    best_end = -1
                    for p in preds[i]:
                        end = txs[p]["end"]
                        if end is not None and end > best_end:
                            best_end = end
                            nxt = p
                    if nxt is None:
                        if i == 0:
                            if cursor:
                                segs.append({"label": "prefix_idle", "ns": cursor, "tx": 0})
                            break
                        i = i - 1
                        if txs[i]["commit"] is not None and cursor > txs[i]["commit"]:
                            segs.append({"label": "schedule_gap", "ns": cursor - txs[i]["commit"], "tx": i})
                            cursor = txs[i]["commit"]
                    else:
                        i = nxt
                        if txs[i]["end"] is not None and cursor > txs[i]["end"]:
                            segs.append({"label": "dep_gap", "ns": cursor - txs[i]["end"], "tx": i})
                            cursor = txs[i]["end"]
                continue
            # No park. Resource predecessor: previous tx on the same worker.
            w = tx["worker"]
            prev_w = None
            prev_end = -1
            if w is not None:
                for j, other in enumerate(txs):
                    if other["worker"] == w and other["end"] is not None and other["end"] <= tx["start"] and other["end"] > prev_end:
                        prev_end = other["end"]
                        prev_w = j
            gate = ready
            gate_tx = ready_tx
            gate_label = "true_dep_gap"
            if prev_end > gate:
                gate = prev_end
                gate_tx = prev_w
                gate_label = "worker_gap"
            gap = tx["start"] - gate
            if gap > 0:
                segs.append({"label": gate_label, "ns": gap, "tx": i})
            if gate_tx is None:
                if gate:
                    segs.append({"label": "prefix_idle", "ns": gate, "tx": i})
                break
            i = gate_tx
            cursor = gate
            continue
        # Commit prefix: this tx finished before the previous commit.
        step = tx["commit"] - prev_c
        if step > 0:
            segs.append({"label": "commit_prefix", "ns": step, "tx": i})
        cursor = prev_c
        i -= 1
    if cursor > 0:
        segs.append({"label": "unattributed_prefix", "ns": cursor, "tx": max(i, 0)})
    return segs


def writer_chain(row: dict, txs: list[dict]) -> dict:
    """Non-lazy writers of the hottest location.

    A hop is one non-lazy writer to the next higher one. Intervening writes
    are chain members between them. `kind` 1 in `members` is a delta and is
    not a blocker; kind 0 (or a dump with no kinds) counts as a blocker.
    """
    ben = int(row.get("beneficiary") or 0)
    by_loc: dict[int, list[tuple[int, bool]]] = defaultdict(list)
    for tx, loc, lazy in row.get("writes") or []:
        loc = int(loc)
        if loc == ben:
            continue
        by_loc[loc].append((int(tx), bool(lazy)))
    if not by_loc:
        return {}
    loc, writers = max(by_loc.items(), key=lambda kv: sum(1 for _tx, lazy in kv[1] if not lazy))
    nonlazy = sorted({tx for tx, lazy in writers if not lazy})
    lazy_set = {tx for tx, lazy in writers if lazy}
    kinds: dict[int, int] = {}
    for raw in row.get("members") or []:
        mloc, mtx, kind = int(raw[0]), int(raw[1]), int(raw[2])
        if mloc == loc:
            kinds[mtx] = kind
    hops = []
    gap_ns = 0
    after_prev_end = 0
    blockers = []
    intervening = []
    exec_ns = 0
    for tx in nonlazy:
        meta = txs[tx] if tx < len(txs) else None
        if meta and meta["start"] is not None and meta["end"] is not None and meta["end"] >= meta["start"]:
            exec_ns += meta["end"] - meta["start"]
    for a, b in zip(nonlazy, nonlazy[1:]):
        left = txs[a] if a < len(txs) else None
        right = txs[b] if b < len(txs) else None
        if left and right and left["end"] is not None and right["start"] is not None:
            gap = right["start"] - left["end"]
            gap_ns += max(0, gap)
            if right["start"] >= left["end"]:
                after_prev_end += 1
        between = range(a + 1, b)
        n_between = 0
        n_block = 0
        for tx in between:
            wrote = tx in lazy_set or tx in set(nonlazy)
            known = tx in kinds
            if not wrote and not known:
                continue
            n_between += 1
            # No kind dump: every intervening writer was a blocker.
            if not kinds or kinds.get(tx, 0) == 0:
                n_block += 1
        intervening.append(n_between)
        blockers.append(n_block)
        hops.append(1)
    starts = [txs[tx]["start"] for tx in nonlazy if tx < len(txs) and txs[tx]["start"] is not None]
    ends = [txs[tx]["end"] for tx in nonlazy if tx < len(txs) and txs[tx]["end"] is not None]
    span = (max(ends) - min(starts)) if starts and ends else 0

    def avg(xs: list[int]) -> float:
        return (sum(xs) / len(xs)) if xs else 0.0

    return {
        "loc": f"{loc:016x}",
        "writers": len(nonlazy),
        "exec_ms": ms(exec_ns),
        "span_ms": ms(span),
        "hops": len(hops),
        "hops_after_prev_end": after_prev_end,
        "gap_ms": ms(gap_ns),
        "intervening_per_hop": round(avg(intervening), 2),
        "blockers_per_hop": round(avg(blockers), 2),
        "intervening_max": max(intervening) if intervening else 0,
        "blockers_max": max(blockers) if blockers else 0,
    }


def cyc_ms(row: dict) -> dict[str, float]:
    cycles = row.get("cyc") or []
    scale = float(row.get("ns_per_cycle") or 0.0)
    names = {name: 0.0 for name in CYC_NAME}
    width = len(CYC_NAME)
    for i, c in enumerate(cycles):
        name = CYC_NAME[i % width]
        names[name] += float(c) * scale
    return {k: v / 1e6 for k, v in names.items()}


def analyze(row: dict) -> dict:
    txs = tx_table(row)
    spans = spans_of(row)
    dag = build_dag(row)
    n = int(row["n"])
    workers = int(row["workers"])
    wall = int(row["wall_ns"])
    costs = costs_from(txs, spans)
    ideal = rep.list_schedule(costs, dag["true"], workers)
    ideal_war = rep.list_schedule(costs, sorted(set(dag["true"]) | set(dag["war"])), workers)
    ideal_1 = rep.list_schedule(costs, dag["true"], 1)
    parks = park_rows(spans)
    # Inline is nested in exec. Worker CPU uses exec (includes inline) plus the rest.
    exec_ns = sum_kind(spans, EXEC)
    inline_ns = sum_kind(spans, INLINE)
    pure_exec = max(0, exec_ns - inline_ns)
    validate_ns = sum_kind(spans, VALIDATE)
    idle_ns = sum_kind(spans, IDLE)
    spin_ns = sum_kind(spans, SPIN)
    queue_ns = sum_kind(spans, QUEUE)
    post = {REASON.get(r, str(r)): ms(sum_kind(spans, POST, r)) for r in (SETUP, RESCAN, LAZY)}
    setup_ns = sum_kind(spans, POST, SETUP)
    rescan_ns = sum_kind(spans, POST, RESCAN)
    lazy_ns = sum_kind(spans, POST, LAZY)
    parallel = max(0, wall - setup_ns - rescan_ns - lazy_ns)
    worker_accounted = exec_ns + validate_ns + idle_ns + spin_ns
    # Drive-loop POST/WORK spans cover each worker, including time outside
    # the interpreter. Union per worker, then divide by the parallel phase.
    work_cover = []
    for w in range(workers):
        iv = [
            (sp["t0"], sp["t1"])
            for sp in spans
            if sp["worker"] == w and sp["kind"] in (EXEC, VALIDATE, IDLE, SPIN, POST, QUEUE)
        ]
        work_cover.append(merge_coverage(iv))
    loop_cover = (sum(work_cover) / (workers * parallel)) if parallel else None
    # exec_ns includes inline. Bookkeeping is whatever the workers did not stamp.
    book_ns = max(0, workers * parallel - worker_accounted)
    excess = armed_excess(parks, txs)
    false_class = class_false(parks, set(map(tuple, dag["true"])))
    lag = commit_lag(txs)
    cp = critical_path(txs, parks, dag["true"], wall)
    cp_sum = sum(s["ns"] for s in cp)
    cp_by: dict[str, int] = defaultdict(int)
    for s in cp:
        cp_by[s["label"]] += s["ns"]
    ideal_ns = ideal.get("makespan_ns") or 0.0
    gap = wall - ideal_ns
    # Park sums are overlapping tx-time, not wall-time. CP labels are wall-time.
    park_by: dict[str, int] = defaultdict(int)
    park_n: dict[str, int] = defaultdict(int)
    for sp in parks:
        name = REASON.get(sp["reason"], str(sp["reason"]))
        park_by[name] += sp["ns"]
        park_n[name] += 1
    top = group_parks(parks)[:12]
    coverage = park_coverage(parks)
    chain = writer_chain(row, txs)
    inline_by: dict[str, int] = defaultdict(int)
    for sp in spans:
        if sp["kind"] == INLINE:
            inline_by[REASON.get(sp["reason"], str(sp["reason"]))] += sp["ns"]
    return {
        "block": row["block"],
        "workers": workers,
        "class": row["class"],
        "n": n,
        "full_replay": row["full_replay"],
        "reexec": row["reexec"],
        "chain_len": row["chain_len"],
        "armed": row["armed"],
        "wall_ms": ms(wall),
        "ideal_ms": ms(ideal_ns),
        "ideal_war_ms": ms(ideal_war.get("makespan_ns")),
        "ideal_1_ms": ms(ideal_1.get("makespan_ns")),
        "ideal_ok": bool(ideal.get("ok")),
        "l_crit_ms": ms(ideal.get("l_crit_ns")),
        "gap_ms": ms(gap),
        "sf_over_ideal": (wall / ideal_ns) if ideal_ns else None,
        "work_ms": ms(sum(costs)),
        "dag": {k: dag[k] for k in ("n_raw", "n_waw", "n_war", "n_sender", "lazy_waw")},
        "per_worker_ms": [
            {k: ms(v) for k, v in bucket.items()} for bucket in per_worker(spans, workers)
        ],
        "worker_ms": {
            "pure_exec": ms(pure_exec),
            "inline": ms(inline_ns),
            "exec_including_inline": ms(exec_ns),
            "validate": ms(validate_ns),
            "idle": ms(idle_ns),
            "spin": ms(spin_ns),
            "queue_tx": ms(queue_ns),
            "book_residual": ms(book_ns),
            "parallel_phase": ms(parallel),
            "accounted_over_capacity": (worker_accounted / (workers * parallel)) if parallel else None,
            "loop_cover": loop_cover,
        },
        "post_ms": post,
        "park_ms": {k: ms(v) for k, v in park_by.items()},
        "park_n": dict(park_n),
        "park_coverage": coverage,
        "writer_chain": chain,
        "inline_ms": {k: ms(v) for k, v in inline_by.items()},
        "armed": excess,
        "class_head": false_class,
        "commit_lag": lag,
        "cyc_ms": cyc_ms(row),
        "top_parks": top,
        "cp_ms": {k: ms(v) for k, v in sorted(cp_by.items(), key=lambda kv: -kv[1])},
        "cp_sum_ms": ms(cp_sum),
        "cp_cover": (cp_sum / wall) if wall else None,
        "cp_head": [
            {"label": s["label"], "ms": round(ms(s["ns"]), 4), "tx": s.get("tx")}
            for s in cp
            if s["ns"] >= 50_000
        ][:40],
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("paths", nargs="+")
    parser.add_argument("--json-out", default="")
    args = parser.parse_args()
    reports = []
    for path in args.paths:
        for row in load_rows(Path(path)):
            reports.append(analyze(row))
    if args.json_out:
        Path(args.json_out).write_text(json.dumps(reports, indent=2))
    for rep_row in reports:
        print(
            f"BLOCK {rep_row['block']} C={rep_row['workers']} {rep_row['class']} "
            f"wall={rep_row['wall_ms']:.3f}ms ideal={rep_row['ideal_ms']:.3f}ms "
            f"gap={rep_row['gap_ms']:.3f}ms ratio={rep_row['sf_over_ideal']:.2f} "
            f"full_replay={rep_row['full_replay']} reexec={rep_row['reexec']} "
            f"cp_cover={rep_row['cp_cover']:.2f}"
        )
        print("  dag", rep_row["dag"], "work_ms", round(rep_row["work_ms"], 3), "ideal1", round(rep_row["ideal_1_ms"], 3))
        print("  worker", {k: round(v, 3) if isinstance(v, float) else v for k, v in rep_row["worker_ms"].items()})
        print("  per_worker", [
            {k: round(v, 3) for k, v in b.items() if v}
            for b in rep_row["per_worker_ms"]
        ])
        print("  post", {k: round(v, 3) for k, v in rep_row["post_ms"].items()})
        print("  park_ms", {k: round(v, 3) for k, v in rep_row["park_ms"].items()}, "n", rep_row["park_n"])
        print(
            "  park_coverage",
            {
                k: (round(v["coverage_ms"], 3), round(v["residence_ms"], 3), v["n"])
                for k, v in rep_row["park_coverage"].items()
            },
        )
        print("  writer_chain", rep_row["writer_chain"])
        print("  inline_ms", {k: round(v, 3) for k, v in rep_row["inline_ms"].items()})
        print("  armed", rep_row["armed"])
        print("  class_head", rep_row["class_head"])
        print("  commit_lag", rep_row["commit_lag"])
        print("  cyc_ms", {k: round(v, 3) for k, v in rep_row["cyc_ms"].items()})
        print("  cp", {k: round(v, 3) for k, v in rep_row["cp_ms"].items()})
        print("  top", [
            (
                t["reason"],
                t["loc"],
                t["class"],
                round(t["coverage_ns"] / 1e6, 3),
                round(t["ns"] / 1e6, 3),
                t["n"],
            )
            for t in rep_row["top_parks"][:8]
        ])


if __name__ == "__main__":
    main()
