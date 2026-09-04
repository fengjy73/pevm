#!/usr/bin/env python3
"""VLDB-style TPS-vs-cores and abort-rate-vs-cores plots (mean + min/max)."""
from __future__ import annotations

import argparse
import json
from collections import defaultdict
from pathlib import Path

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt
import numpy as np

# Bound to mode for the whole lab (see lab/literature/vldb-cc-figure-conventions.md)
COLORS = {
    "sequential": "#637083",
    "occ": "#2563eb",
    "pcc": "#0f766e",
    "specfence": "#c2410c",
}
LABELS = {
    "sequential": "Sequential",
    "occ": "OCC (Block-STM)",
    "pcc": "PCC",
    "specfence": "SpecFence",
}
ORDER = ["sequential", "occ", "pcc", "specfence"]


def agg(rows, block, mode, metric):
    by_cores = defaultdict(list)
    for r in rows:
        if r.get("block") != block or r.get("mode") != mode or not r.get("ok", True):
            continue
        by_cores[int(r["cores"])].append(float(r[metric]))
    xs, mean, lo, hi = [], [], [], []
    for c in sorted(by_cores):
        v = np.array(by_cores[c], dtype=float)
        xs.append(c)
        mean.append(float(v.mean()))
        lo.append(float(v.min()))
        hi.append(float(v.max()))
    return np.array(xs), np.array(mean), np.array(lo), np.array(hi)


def style_ax(ax, xlabel, ylabel, title):
    ax.set_xlabel(xlabel)
    ax.set_ylabel(ylabel)
    ax.set_title(title)
    ax.grid(True, alpha=0.25, linestyle="--", linewidth=0.6)
    ax.spines["top"].set_visible(False)
    ax.spines["right"].set_visible(False)


def plot_block(rows, block, outdir: Path):
    fig, axes = plt.subplots(1, 2, figsize=(10.2, 3.8), dpi=160)
    # TPS
    ax = axes[0]
    seq_x, seq_m, _, _ = agg(rows, block, "sequential", "tps")
    if len(seq_m):
        ax.axhline(seq_m[0], color=COLORS["sequential"], linestyle="--", linewidth=1.2, label="Sequential")
    occ1 = None
    for mode in ["occ", "pcc", "specfence"]:
        x, m, lo, hi = agg(rows, block, mode, "tps")
        if len(x) == 0:
            continue
        if mode == "occ" and 1 in x:
            occ1 = m[list(x).index(1)]
        yerr = np.vstack([np.maximum(0, m - lo), np.maximum(0, hi - m)])
        ax.errorbar(
            x, m, yerr=yerr, color=COLORS[mode], marker="o", markersize=4.5,
            linewidth=1.6, capsize=3, label=LABELS[mode],
        )
    if occ1 is not None:
        xmax = max(r["cores"] for r in rows if r["block"] == block)
        ax.plot([1, xmax], [occ1, occ1 * xmax], color="#9ca3af", linestyle=":", linewidth=1, label="Perfect scale (from 1-core OCC)")
    style_ax(ax, "Worker threads", "TPS (tx / s)", f"Block {block} — throughput")
    ax.legend(frameon=False, fontsize=8)

    ax = axes[1]
    for mode in ["occ", "pcc", "specfence"]:
        x, m, lo, hi = agg(rows, block, mode, "abort_rate")
        if len(x) == 0:
            continue
        yerr = np.vstack([np.maximum(0, m - lo), np.maximum(0, hi - m)])
        ax.errorbar(
            x, m, yerr=yerr, color=COLORS[mode], marker="o", markersize=4.5,
            linewidth=1.6, capsize=3, label=LABELS[mode],
        )
    style_ax(ax, "Worker threads", "Abort rate (retries / tx)", f"Block {block} — abort rate")
    ax.legend(frameon=False, fontsize=8)
    fig.tight_layout()
    path = outdir / f"block_{block}_tps_abort.png"
    fig.savefig(path, bbox_inches="tight")
    plt.close(fig)
    return path


def plot_overview(rows, outdir: Path):
    blocks = sorted({r["block"] for r in rows})
    fig, axes = plt.subplots(2, 1, figsize=(9.5, 7.2), dpi=160)
    for metric, ax, ylabel, title in [
        ("tps", axes[0], "Mean TPS", "Throughput vs cores (mean over repeats)"),
        ("abort_rate", axes[1], "Mean abort rate (retries / tx)", "Abort rate vs cores"),
    ]:
        for mode in ["occ", "pcc", "specfence"]:
            # mean across blocks of per-block means
            core_set = sorted({int(r["cores"]) for r in rows if r["mode"] == mode})
            xs, ys = [], []
            for c in core_set:
                per_block = []
                for b in blocks:
                    x, m, _, _ = agg(rows, b, mode, metric)
                    if len(x) and c in x:
                        per_block.append(m[list(x).index(c)])
                if per_block:
                    xs.append(c)
                    ys.append(float(np.mean(per_block)))
            if xs:
                ax.plot(xs, ys, color=COLORS[mode], marker="o", linewidth=1.7, label=LABELS[mode])
        style_ax(ax, "Worker threads", ylabel, title)
        ax.legend(frameon=False, fontsize=8)
    fig.tight_layout()
    path = outdir / "overview_tps_abort.png"
    fig.savefig(path, bbox_inches="tight")
    plt.close(fig)
    return path


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--input", required=True)
    ap.add_argument("--outdir", required=True)
    args = ap.parse_args()
    rows = json.loads(Path(args.input).read_text())
    outdir = Path(args.outdir)
    outdir.mkdir(parents=True, exist_ok=True)
    written = []
    for b in sorted({r["block"] for r in rows}):
        written.append(str(plot_block(rows, b, outdir)))
    written.append(str(plot_overview(rows, outdir)))
    # wait vs speculate companion for SpecFence
    fig, ax = plt.subplots(figsize=(8.4, 3.6), dpi=160)
    blocks = sorted({r["block"] for r in rows})
    x = np.arange(len(blocks))
    waits, specs = [], []
    for b in blocks:
        w, s = [], []
        for r in rows:
            if r["block"] == b and r["mode"] == "specfence" and r.get("ok", True):
                w.append(r["wait_admissions"])
                s.append(r["speculate_executions"])
        waits.append(float(np.mean(w) if w else 0))
        specs.append(float(np.mean(s) if s else 0))
    ax.bar(x - 0.18, waits, 0.36, color=COLORS["pcc"], label="Wait admissions")
    ax.bar(x + 0.18, specs, 0.36, color=COLORS["occ"], label="Speculate executions")
    ax.set_xticks(x, [str(b) for b in blocks], rotation=30, ha="right")
    style_ax(ax, "Block", "Count (mean over cores × repeats)", "SpecFence mixed CC — Wait vs Speculate")
    ax.legend(frameon=False, fontsize=8)
    fig.tight_layout()
    mix = outdir / "specfence_wait_vs_speculate.png"
    fig.savefig(mix, bbox_inches="tight")
    plt.close(fig)
    written.append(str(mix))

    # P1a: wait_hard vs spec_read (SpecFence only, mean over cores×repeats)
    fig, ax = plt.subplots(figsize=(8.4, 3.6), dpi=160)
    wh, sr = [], []
    for b in blocks:
        wvals, svals = [], []
        for r in rows:
            if r["block"] == b and r["mode"] == "specfence" and r.get("ok", True):
                wvals.append(float(r.get("wait_hard_count", 0) or 0))
                svals.append(float(r.get("spec_read_count", 0) or 0))
        wh.append(float(np.mean(wvals) if wvals else 0))
        sr.append(float(np.mean(svals) if svals else 0))
    ax.bar(x - 0.18, wh, 0.36, color=COLORS["specfence"], label="WaitHard")
    ax.bar(x + 0.18, sr, 0.36, color=COLORS["occ"], label="SpecRead")
    ax.set_xticks(x, [str(b) for b in blocks], rotation=30, ha="right")
    style_ax(ax, "Block", "Count (mean over cores × repeats)", "SpecFence P1a — WaitHard vs SpecRead")
    ax.legend(frameon=False, fontsize=8)
    fig.tight_layout()
    p = outdir / "specfence_wait_hard_vs_spec_read.png"
    fig.savefig(p, bbox_inches="tight")
    plt.close(fig)
    written.append(str(p))

    # P1a overview: full_retry / bind_hits / selective metrics vs cores (SpecFence)
    fig, axes = plt.subplots(2, 2, figsize=(10.2, 7.0), dpi=160)
    panels = [
        ("tx_full_retry", "Mean tx_full_retry", "FullRetry vs cores"),
        ("bind_hits", "Mean bind_hits", "Bind hits vs cores"),
        ("selective_invalidate_count", "Mean selective_invalidate", "Selective invalidate vs cores"),
        ("selective_fallback_full", "Mean selective_fallback_full", "Selective→full fallback vs cores"),
    ]
    for ax, (metric, ylabel, title) in zip(axes.ravel(), panels):
        core_set = sorted({int(r["cores"]) for r in rows if r["mode"] == "specfence"})
        xs, ys = [], []
        for c in core_set:
            per_block = []
            for b in blocks:
                vals = [
                    float(r.get(metric, 0) or 0)
                    for r in rows
                    if r.get("block") == b
                    and r.get("mode") == "specfence"
                    and int(r["cores"]) == c
                    and r.get("ok", True)
                ]
                if vals:
                    per_block.append(float(np.mean(vals)))
            if per_block:
                xs.append(c)
                ys.append(float(np.mean(per_block)))
        if xs:
            ax.plot(xs, ys, color=COLORS["specfence"], marker="o", linewidth=1.7, label="SpecFence")
        style_ax(ax, "Worker threads", ylabel, title)
        ax.legend(frameon=False, fontsize=8)
    fig.tight_layout()
    p = outdir / "specfence_p1a_metrics_overview.png"
    fig.savefig(p, bbox_inches="tight")
    plt.close(fig)
    written.append(str(p))


    # P2: partial_retry vs full_retry (SpecFence, mean over cores×repeats)
    fig, ax = plt.subplots(figsize=(8.4, 3.6), dpi=160)
    pr, fr = [], []
    for b in blocks:
        pvals, fvals = [], []
        for r in rows:
            if r["block"] == b and r["mode"] == "specfence" and r.get("ok", True):
                pvals.append(float(r.get("partial_retry_count", 0) or 0))
                fvals.append(float(r.get("tx_full_retry", 0) or 0))
        pr.append(float(np.mean(pvals) if pvals else 0))
        fr.append(float(np.mean(fvals) if fvals else 0))
    ax.bar(x - 0.18, pr, 0.36, color=COLORS["pcc"], label="PartialRetry")
    ax.bar(x + 0.18, fr, 0.36, color=COLORS["specfence"], label="FullRetry")
    ax.set_xticks(x, [str(b) for b in blocks], rotation=30, ha="right")
    style_ax(ax, "Block", "Count (mean over cores × repeats)", "SpecFence P2 — PartialRetry vs FullRetry")
    ax.legend(frameon=False, fontsize=8)
    fig.tight_layout()
    p = outdir / "specfence_partial_vs_full_retry.png"
    fig.savefig(p, bbox_inches="tight")
    plt.close(fig)
    written.append(str(p))

    # P2 overview: partial_retry / full_retry / pr_fallback / bind vs cores
    fig, axes = plt.subplots(2, 2, figsize=(10.2, 7.0), dpi=160)
    panels_p2 = [
        ("partial_retry_count", "Mean partial_retry_count", "PartialRetry vs cores"),
        ("tx_full_retry", "Mean tx_full_retry", "FullRetry vs cores"),
        ("partial_retry_fallback_full", "Mean partial_retry_fallback_full", "Partial→full fallback vs cores"),
        ("bind_hits", "Mean bind_hits", "Bind hits vs cores"),
    ]
    for ax, (metric, ylabel, title) in zip(axes.ravel(), panels_p2):
        core_set = sorted({int(r["cores"]) for r in rows if r["mode"] == "specfence"})
        xs, ys = [], []
        for c in core_set:
            per_block = []
            for b in blocks:
                vals = [
                    float(r.get(metric, 0) or 0)
                    for r in rows
                    if r.get("block") == b
                    and r.get("mode") == "specfence"
                    and int(r["cores"]) == c
                    and r.get("ok", True)
                ]
                if vals:
                    per_block.append(float(np.mean(vals)))
            if per_block:
                xs.append(c)
                ys.append(float(np.mean(per_block)))
        if xs:
            ax.plot(xs, ys, color=COLORS["specfence"], marker="o", linewidth=1.7, label="SpecFence")
        style_ax(ax, "Worker threads", ylabel, title)
        ax.legend(frameon=False, fontsize=8)
    fig.tight_layout()
    p = outdir / "specfence_p2_metrics_overview.png"
    fig.savefig(p, bbox_inches="tight")
    plt.close(fig)
    written.append(str(p))

    print("\n".join(written))


if __name__ == "__main__":
    main()
