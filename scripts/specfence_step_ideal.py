#!/usr/bin/env python3
"""List-schedule ideal makespan from per-transaction durations and RAW edges.

Stage 1 does not record opcode steps. `TPS_ideal(C)` is the makespan of a
list schedule: a transaction becomes ready when every RAW predecessor has
finished, and ready transactions are issued in index order onto C workers.
"""

from __future__ import annotations


def list_schedule(tx_ns: list[int], edges: list[tuple[int, int]], cores: int) -> dict:
    n = len(tx_ns)
    if n == 0 or cores < 1:
        return {"ok": False, "makespan_ns": None, "cores": cores}
    preds = [0] * n
    succ: list[list[int]] = [[] for _ in range(n)]
    pred_finish_need: list[list[int]] = [[] for _ in range(n)]
    for writer, reader in edges:
        if writer < 0 or reader < 0 or writer >= n or reader >= n or writer >= reader:
            continue
        preds[reader] += 1
        succ[writer].append(reader)
        pred_finish_need[reader].append(writer)
    ready = [i for i in range(n) if preds[i] == 0]
    ready.sort()
    worker_free = [0] * cores
    finish = [0] * n
    scheduled = 0
    guard = 0
    while ready and scheduled < n:
        guard += 1
        if guard > n * cores + n + 8:
            return {"ok": False, "makespan_ns": None, "cores": cores}
        worker = min(range(cores), key=lambda i: worker_free[i])
        tx = ready.pop(0)
        start = worker_free[worker]
        for pred in pred_finish_need[tx]:
            if finish[pred] > start:
                start = finish[pred]
        finish[tx] = start + int(tx_ns[tx])
        worker_free[worker] = finish[tx]
        scheduled += 1
        for reader in succ[tx]:
            preds[reader] -= 1
            if preds[reader] == 0:
                ready.append(reader)
        ready.sort()
    if scheduled != n:
        return {"ok": False, "makespan_ns": None, "cores": cores}
    return {"ok": True, "makespan_ns": max(finish) if finish else 0, "cores": cores}


def tps_ideal(tx_ns: list[int], edges: list[tuple[int, int]], cores: int) -> float | None:
    sim = list_schedule(tx_ns, edges, cores)
    span = sim.get("makespan_ns")
    if not sim.get("ok") or not span:
        return None
    return len(tx_ns) / (span / 1e9)
