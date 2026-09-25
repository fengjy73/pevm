#!/usr/bin/env python3
"""Stage-1 report: 1-core TPS_SEQ, TPS_OCC, TPS_SF, TPS_ideal(C), in-block traces.

TPS_SEQ is the sequential engine at C=1. It is not recomputed per worker count.
Wall medians use a percentile bootstrap. TPS_ideal(C) list-schedules the traced
per-tx durations and RAW edges (see specfence_step_ideal.py).
"""

from __future__ import annotations

import argparse
import json
import random
import statistics
import sys
from collections import defaultdict
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import specfence_step_ideal as step_ideal


def load_rows(scan_dir: Path) -> list[dict]:
    rows = []
    for path in sorted(scan_dir.glob("*.jsonl")):
        for line in path.read_text().splitlines():
            line = line.strip()
            if not line:
                continue
            row = json.loads(line)
            row["_file"] = path.name
            rows.append(row)
    return rows


def bootstrap_median_ci(samples: list[float], seed: int = 1, draws: int = 2000) -> dict:
    if not samples:
        return {"n": 0, "median": None, "lo": None, "hi": None}
    ordered = sorted(samples)
    mid = ordered[len(ordered) // 2] if len(ordered) % 2 else (
        ordered[len(ordered) // 2 - 1] + ordered[len(ordered) // 2]
    ) / 2
    if len(samples) == 1:
        return {"n": 1, "median": mid, "lo": mid, "hi": mid}
    rng = random.Random(seed)
    meds = []
    n = len(samples)
    for _ in range(draws):
        draw = [samples[rng.randrange(n)] for _ in range(n)]
        meds.append(statistics.median(draw))
    meds.sort()
    lo = meds[int(0.025 * (draws - 1))]
    hi = meds[int(0.975 * (draws - 1))]
    return {"n": n, "median": statistics.median(samples), "lo": lo, "hi": hi}


def tps(n_tx: int, wall_ms: float | None) -> float | None:
    if wall_ms is None or wall_ms <= 0:
        return None
    return n_tx / (wall_ms / 1000.0)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--scan-dir", required=True)
    args = parser.parse_args()
    scan_dir = Path(args.scan_dir)
    rows = [r for r in load_rows(scan_dir) if r.get("ok")]
    host = (scan_dir / "host.txt").read_text() if (scan_dir / "host.txt").exists() else ""
    print("# host")
    print(host.strip())
    print()

    # (block, workers, engine, class_key) -> walls. SEQ/OCC have empty class key.
    groups: dict[tuple, list[float]] = defaultdict(list)
    meta: dict[int, dict] = {}
    for row in rows:
        if row.get("_file", "").startswith("trace-"):
            continue
        block = int(row["block"])
        workers = int(row["workers"])
        engine = row["engine"]
        key = row.get("class_key") or ""
        groups[(block, workers, engine, key)].append(float(row["wall_ms"]))
        meta[block] = {"n_tx": int(row["n_tx"]), "gas_used": int(row["gas_used"])}

    seq_base: dict[int, dict] = {}
    for (block, workers, engine, key), walls in groups.items():
        if engine == "seq" and workers == 1:
            seq_base[block] = bootstrap_median_ci(walls, seed=block)

    print("# wall medians (ms) and TPS")
    print("block workers engine class_key n median_ms ci95_lo ci95_hi tps tps_seq tps_ideal")
    blocks = sorted(meta)
    worker_set = sorted({w for (_, w, _, _) in groups})
    for block in blocks:
        n_tx = meta[block]["n_tx"]
        seq = seq_base.get(block)
        tps_seq = tps(n_tx, seq["median"]) if seq else None
        for workers in worker_set:
            for engine, key in (
                ("seq", ""),
                ("occ", ""),
                ("sf", "to+selector"),
                ("sf", "code_hash+selector"),
            ):
                walls = groups.get((block, workers, engine, key))
                if not walls and engine == "seq":
                    # SEQ rows are stored once per C; the baseline is C=1.
                    walls = groups.get((block, workers, "seq", ""))
                if not walls:
                    continue
                # Prefer the C=1 sequential row as TPS_SEQ even when this row is C>1.
                if engine == "seq" and workers != 1:
                    continue
                stat = bootstrap_median_ci(walls, seed=block + workers)
                ideal = None
                print(
                    f"{block} {workers} {engine} {key or '-'} {stat['n']} "
                    f"{stat['median']:.3f} {stat['lo']:.3f} {stat['hi']:.3f} "
                    f"{tps(n_tx, stat['median']):.1f} "
                    f"{'' if tps_seq is None else f'{tps_seq:.1f}'} "
                    f"{'' if ideal is None else f'{ideal:.1f}'}"
                )

    print()
    print("# in-block trace (fresh single run) and TPS_ideal")
    print(
        "block workers class_key reexec full_replay reads_after_arm "
        "full_replay_after_arm chain_len armed exec_entries tps_ideal"
    )
    for path in sorted(scan_dir.glob("trace-*.jsonl")):
        for line in path.read_text().splitlines():
            if not line.strip():
                continue
            row = json.loads(line)
            if row.get("engine") != "sf" or not row.get("ok"):
                continue
            tx_ns = [int(x) for x in row.get("tx_ns") or []]
            edges = [(int(a), int(b)) for a, b in row.get("raw_edges") or []]
            cores = int(row["workers"])
            ideal = step_ideal.tps_ideal(tx_ns, edges, cores) if tx_ns else None
            print(
                f"{row['block']} {cores} {row.get('class_key') or '-'} "
                f"{row.get('reexec')} {row.get('full_replay')} {row.get('reads_after_arm')} "
                f"{row.get('full_replay_after_arm')} {row.get('chain_len')} {row.get('armed')} "
                f"{row.get('exec_entries')} "
                f"{'' if ideal is None else f'{ideal:.1f}'}"
            )


if __name__ == "__main__":
    main()
