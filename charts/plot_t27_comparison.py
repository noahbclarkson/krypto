#!/usr/bin/env python3
"""Plot 2026-04-29 T27 ATR-rank hyperopt equity comparison.

Reads the walk-forward equity CSV exported by examples/atr_rank_filter_prod_sweep.rs
and writes a real equity-over-time line chart to charts/comparison_chart.png.
"""
import csv
import math
import re
from collections import defaultdict
from pathlib import Path

import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt

ROOT = Path(__file__).resolve().parents[1]
RESULTS_CSV = ROOT / "snapshots" / "atr_rank_filter_prod_results.csv"
EQUITY_CSV = ROOT / "snapshots" / "atr_rank_filter_prod_equity.csv"
OUT = ROOT / "charts" / "comparison_chart.png"
ALIAS = ROOT / "charts" / "t27_comparison_chart.png"


def read_results():
    rows = []
    with RESULTS_CSV.open() as f:
        for row in csv.DictReader(f):
            rows.append({
                "threshold": int(row["threshold"]),
                "return_pct": float(row["return_pct"]),
                "sharpe": float(row["sharpe"]),
                "max_dd_pct": float(row["max_dd_pct"]),
                "trades": int(row["trades"]),
                "win_rate": float(row["win_rate"]),
                "pass": row["pass"] in {"1", "true", "True"},
            })
    return rows


def aggregate(rows):
    by_t = defaultdict(list)
    for r in rows:
        by_t[r["threshold"]].append(r)
    out = {}
    for t, rs in by_t.items():
        out[t] = {
            "n": len(rs),
            "pass_count": sum(1 for r in rs if r["pass"]),
            "avg_return": sum(r["return_pct"] for r in rs) / len(rs),
            "avg_sharpe": sum(r["sharpe"] for r in rs) / len(rs),
            "avg_dd": sum(r["max_dd_pct"] for r in rs) / len(rs),
            "trades": sum(r["trades"] for r in rs),
            "avg_win_rate": sum(r["win_rate"] for r in rs) / len(rs),
        }
    return out


def read_equity():
    curves_by_t = defaultdict(list)
    with EQUITY_CSV.open() as f:
        reader = csv.DictReader(f)
        cols = reader.fieldnames or []
        parsed = []
        for col in cols:
            if col == "step":
                continue
            m = re.search(r"_T(\d+)$", col)
            if m:
                parsed.append((col, int(m.group(1))))
        curves = {col: [] for col, _ in parsed}
        for row in reader:
            for col, _ in parsed:
                v = row.get(col, "")
                if not v:
                    curves[col].append(math.nan)
                else:
                    try:
                        curves[col].append(float(v))
                    except ValueError:
                        curves[col].append(math.nan)
        for col, t in parsed:
            curves_by_t[t].append(curves[col])
    return curves_by_t


def mean_curve(curves):
    max_len = max(len(c) for c in curves)
    y = []
    for i in range(max_len):
        vals = [c[i] for c in curves if i < len(c) and math.isfinite(c[i]) and c[i] > 0]
        y.append(sum(vals) / len(vals) if vals else math.nan)
    return y


def main():
    rows = read_results()
    agg = aggregate(rows)
    curves_by_t = read_equity()

    # Robustness-first: pass count, then avg Sharpe, then lower DD. T=0 is baseline.
    baseline = 0
    winner = max(agg, key=lambda t: (agg[t]["pass_count"], agg[t]["avg_sharpe"], -agg[t]["avg_dd"]))
    runners = [t for t in sorted(agg, key=lambda t: (agg[t]["pass_count"], agg[t]["avg_sharpe"], -agg[t]["avg_dd"]), reverse=True)
               if t not in {baseline, winner}][:3]
    selected = [baseline, winner] + runners

    colors = {
        baseline: "#222222",
        winner: "#1f77b4",
    }
    palette = ["#2ca02c", "#ff7f0e", "#9467bd", "#d62728"]
    for t, c in zip(runners, palette):
        colors[t] = c

    fig, ax = plt.subplots(figsize=(13, 7.5))
    for t in selected:
        y = mean_curve(curves_by_t[t])
        x = list(range(len(y)))
        a = agg[t]
        label = (
            f"T={t}"
            f" — {a['pass_count']}/{a['n']} pass, Sharpe {a['avg_sharpe']:.2f}, "
            f"DD {a['avg_dd']:.1f}%"
        )
        if t == baseline:
            label = "Baseline no ATR-rank filter: " + label
        elif t == winner:
            label = "Winner ATR-rank >=10th pct: " + label
        ax.plot(x, y, label=label, linewidth=3.0 if t in {baseline, winner} else 2.0,
                color=colors[t], alpha=0.92)

    vals = []
    for t in selected:
        vals.extend([v for v in mean_curve(curves_by_t[t]) if math.isfinite(v) and v > 0])
    ymin, ymax = min(vals), max(vals)
    pad = (math.log(ymax) - math.log(ymin)) * 0.08 if ymax > ymin else 0.1
    ax.set_yscale("log")
    ax.set_ylim(math.exp(math.log(ymin) - pad), math.exp(math.log(ymax) + pad))
    ax.set_title("T27 ATR-Rank Entry Filter Hyperopt — Mean Walk-Forward Equity Curves")
    ax.set_xlabel("Walk-forward test step (252-bar OOS windows, averaged across 9 universes × 6 windows)")
    ax.set_ylabel("Mean portfolio equity, log scale (start = 1.0)")
    ax.grid(True, which="both", alpha=0.28)
    ax.legend(loc="best", fontsize=9)
    ax.axhline(1.0, color="gray", linestyle="--", linewidth=0.8, alpha=0.8)

    txt = (
        "Extensive sweep: ATR percentile threshold 0..100 step 5.\n"
        f"Winner: T={winner}; baseline T=0 remains pass-rate tied but lower Sharpe/higher DD.\n"
        "No fixed-zero Y-axis; log scaling used to keep curves visible."
    )
    ax.text(0.012, 0.02, txt, transform=ax.transAxes, fontsize=9,
            bbox=dict(facecolor="white", alpha=0.8, edgecolor="none"))

    fig.tight_layout()
    OUT.parent.mkdir(parents=True, exist_ok=True)
    fig.savefig(OUT, dpi=180, bbox_inches="tight")
    fig.savefig(ALIAS, dpi=180, bbox_inches="tight")
    plt.close(fig)

    print(f"Saved {OUT}")
    print(f"Winner T={winner}")
    for t in selected:
        a = agg[t]
        print(f"T={t:>3}: pass={a['pass_count']}/{a['n']} avg_sharpe={a['avg_sharpe']:.4f} "
              f"avg_dd={a['avg_dd']:.2f}% trades={a['trades']}")


if __name__ == "__main__":
    main()
