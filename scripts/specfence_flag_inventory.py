#!/usr/bin/env python3
"""List SpecFence env identifiers in this tree.

Reads crates/, scripts/, and bins/. Does not decide KEEP versus DELETE.
The classification lives in docs/specfence-dead-code-inventory.md.
"""

from __future__ import annotations

import re
import sys
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SCAN_DIRS = ("crates", "scripts", "bins")
TOKEN = re.compile(r"SPECFENCE_[A-Z0-9_]+")
SUFFIXES = {".rs", ".py", ".sh", ".toml"}


def main() -> int:
    hits: dict[str, set[str]] = defaultdict(set)
    for dirname in SCAN_DIRS:
        base = ROOT / dirname
        if not base.is_dir():
            continue
        for path in base.rglob("*"):
            if not path.is_file() or path.suffix not in SUFFIXES:
                continue
            if "target" in path.parts:
                continue
            text = path.read_text(errors="ignore")
            rel = path.relative_to(ROOT).as_posix()
            for match in TOKEN.finditer(text):
                hits[match.group()].add(rel)
    for name in sorted(hits):
        files = ", ".join(sorted(hits[name]))
        print(f"{name}\t{len(hits[name])}\t{files}")
    print(f"# {len(hits)} identifiers", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
