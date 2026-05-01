#!/usr/bin/env python3
"""Plot CHAND_PERIOD hyperopt equity curves.

Reads:
  snapshots/chand_period_fine_summary.csv
  snapshots/chand_period_fine_equity.csv
Writes:
  charts/comparison_chart.png

The equity panel stitches Base5 walk-forward windows sequentially so the
chart is a true line graph of portfolio value over time/steps for the
baseline, winner, and top runner-ups.
"""
from __future__ import annotations

import csv
import os
from collections import defaultdict

import matplotlib.pyplot as plt
import numpy as np

ROOT = "/home/ubuntu/.openclaw/workspace-krypto/krypto"
SUMMARY = os.path.join(ROOT, "snapshots/chand_period_fine_summary.csv")
EQUITY = os.path.join(ROOT, "snapshots/chand_period_fine_equity.csv")
OUT = os.path.join(ROOT, "charts/comparison_chart.png")
os.makedirs(os.path.dirname(OUT), exist_ok=True)

BASELINE_CP = 7

# Load summary and rank by robustness: pass_rate desc, avg_sharpe desc, avg_ret desc.
summary = []
with open(SUMMARY, newline="") as f:
    reader = csv.DictReader(f)
    for row in reader:
        row = {k: (float(v) if k not in {"cp", "global_pass", "global_total", "total_trades"} else int(float(v))) for k, v in row.items()}
        summary.append(row)

summary_sorted = sorted(summary, key=lambda r: (r["pass_rate"], r["avg_sharpe"], r["avg_ret"]), reverse=True)
winner_cp = summary_sorted[0]["cp"]
runner_cps = [r["cp"] for r in summary_sorted if r["cp"] not in {winner_cp, BASELINE_CP}][:3]
plot_cps = [BASELINE_CP, winner_cp] + runner_cps

metrics_by_cp = {r["cp"]: r for r in summary}

# Load Base5 equity by CP/window/step.
curves = defaultdict(lambda: defaultdict(list))
with open(EQUITY, newline="") as f:
    reader = csv.DictReader(f)
    for row in reader:
        cp = int(row["cp"])
        if cp not in set(plot_cps):
            continue
        if row["universe"] != "Base5":
            continue
        window = int(row["window"])
        step = int(row["step"])
        eq = float(row["equity"])
        curves[cp][window].append((step, eq))

# Stitch Base5 windows sequentially for each CP.
stitches = {}
for cp in plot_cps:
    x, y = [], []
    cum = 1.0
    global_step = 0
    for window in sorted(curves[cp]):
        pts = sorted(curves[cp][window])
        if not pts:
            continue
        # Each window starts at local equity 1.0. Convert to cumulative global equity.
        for j, (_, local_eq) in enumerate(pts):
            if x and j == 0:
                continue  # skip duplicate reset point
            x.append(global_step)
            y.append(cum * local_eq)
            global_step += 1
        cum *= pts[-1][1]
    stitches[cp] = (np.asarray(x), np.asarray(y))

# Plot.
fig, axes = plt.subplots(2, 1, figsize=(15, 10), gridspec_kw={"height_ratios": [3, 1]})
ax = axes[0]
colors = {
    BASELINE_CP: "#1f77b4",
    winner_cp: "#d62728",
}
palette = ["#2ca02c", "#9467bd", "#ff7f0e", "#17becf"]
for i, cp in enumerate(plot_cps):
    if cp not in colors:
        colors[cp] = palette[i % len(palette)]

for cp in plot_cps:
    x, y = stitches.get(cp, (np.array([]), np.array([])))
    if len(x) == 0:
        continue
    m = metrics_by_cp[cp]
    label = (
        f"CP={cp}"
        f" | pass {m['global_pass']}/{m['global_total']}"
        f" | Sharpe {m['avg_sharpe']:.2f}"
    )
    style = "--" if cp == BASELINE_CP else "-"
    width = 2.6 if cp in {winner_cp, BASELINE_CP} else 1.8
    ax.plot(x, y, label=label, color=colors[cp], linestyle=style, linewidth=width)

all_y = np.concatenate([stitches[cp][1] for cp in plot_cps if len(stitches.get(cp, ([], []))[1]) > 0])
if len(all_y):
    ymin = max(all_y.min() * 0.92, 1e-6)
    ymax = all_y.max() * 1.08
    ax.set_ylim(ymin, ymax)
    ax.set_yscale("log")

ax.set_title(
    "CHAND_PERIOD Hyperopt — Base5 Stitched Walk-Forward Equity\n"
    f"Winner CP={winner_cp} vs Production Baseline CP={BASELINE_CP}; 9-universe robustness ranking used for selection"
)
ax.set_ylabel("Portfolio equity (log scale, dynamic Y)")
ax.set_xlabel("Walk-forward step (Base5 windows stitched sequentially)")
ax.grid(True, which="both", linestyle="--", alpha=0.35)
ax.legend(loc="upper left", fontsize=9)

# Panel 2: full sweep pass-rate and Sharpe curve.
ax2 = axes[1]
cps = [r["cp"] for r in sorted(summary, key=lambda r: r["cp"])]
pass_rates = [metrics_by_cp[cp]["pass_rate"] for cp in cps]
sharpes = [metrics_by_cp[cp]["avg_sharpe"] for cp in cps]
ax2.plot(cps, pass_rates, color="#4c78a8", marker="o", markersize=3, linewidth=1.5, label="Pass rate %")
ax2b = ax2.twinx()
ax2b.plot(cps, sharpes, color="#f58518", marker="s", markersize=3, linewidth=1.2, label="Avg Sharpe")
ax2.axvline(BASELINE_CP, color=colors[BASELINE_CP], linestyle="--", alpha=0.7, label=f"Baseline CP={BASELINE_CP}")
ax2.axvline(winner_cp, color=colors[winner_cp], linestyle="-", alpha=0.8, label=f"Winner CP={winner_cp}")
ax2.set_xlabel("CHAND_PERIOD")
ax2.set_ylabel("9-universe pass rate %")
ax2b.set_ylabel("Avg Sharpe")
ax2.grid(True, linestyle="--", alpha=0.35)
lines, labels = ax2.get_legend_handles_labels()
lines2, labels2 = ax2b.get_legend_handles_labels()
ax2.legend(lines + lines2, labels + labels2, loc="upper right", fontsize=9)

plt.tight_layout()
plt.savefig(OUT, dpi=200, bbox_inches="tight")
print(f"Saved {OUT}")
print(f"Plot CPs: {plot_cps}; winner={winner_cp}, baseline={BASELINE_CP}")
