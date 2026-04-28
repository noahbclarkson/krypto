#!/usr/bin/env python3
"""Plot MIN_TRADES sweep comparison: equity curves for baseline, winner, runnerups."""

import csv
import sys
import os

BASE = "/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots"
OUT  = "/home/ubuntu/.openclaw/workspace-krypto/krypto/charts/min_trades_sweep_comparison.png"

def load_csv(path):
    bars, eqs = [], []
    with open(path) as f:
        reader = csv.DictReader(f)
        for row in reader:
            bars.append(int(row["bar"]))
            eqs.append(float(row["equity"]))
    return bars, eqs

files = {
    "MT=3 (baseline)":   "min_trades_baseline_equity.csv",
    "MT=12 (winner)":     "min_trades_winner_equity.csv",
    "MT=11 (runnerup)":   "min_trades_runnerup_equity.csv",
}

try:
    curves = {label: load_csv(os.path.join(BASE, fname)) for label, fname in files.items()}
except FileNotFoundError as e:
    print(f"ERROR: {e}")
    sys.exit(1)

import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
import matplotlib.ticker as mticker

fig, (ax1, ax2) = plt.subplots(2, 1, figsize=(14, 10), gridspec_kw={"height_ratios": [3, 1]})
fig.suptitle("MIN_TRADES Hyperparameter Sweep — Turtle+Chandelier\nMT ∈ [1..13] × Base5 + NoDOGE (walk-forward)", fontsize=14, fontweight="bold")

colors = {"MT=3 (baseline)": "#2196F3", "MT=12 (winner)": "#F44336", "MT=11 (runnerup)": "#FF9800"}

# ── Top: equity curves (log scale) ──
for label, (bars, eqs) in curves.items():
    ax1.plot(bars, eqs, label=label, color=colors[label], linewidth=1.8, alpha=0.9)

ax1.set_yscale("log")
ax1.yaxis.set_major_formatter(mticker.FuncFormatter(lambda v, _: f"{v:.2f}"))
ax1.set_ylabel("Portfolio Equity (log scale)", fontsize=11)
ax1.set_xlabel("Bar index", fontsize=11)
ax1.set_title("Equity Curves — baseline MT=3 vs winner MT=12", fontsize=11, style="italic")
ax1.legend(loc="upper left", fontsize=10)
ax1.grid(True, linestyle="--", alpha=0.4)
ax1.set_xlim(0, max(len(b) for b, _ in curves.values()))

# ── Bottom: bar chart of Sharpe by MT value ──
sweep_path = os.path.join(BASE, "min_trades_sweep_results.csv")
mt_sharpe = []
with open(sweep_path) as f:
    reader = csv.DictReader(f)
    for row in reader:
        mt_sharpe.append((int(row["mt"]), float(row["avg_sharpe"])))

mt_sharpe.sort(key=lambda x: x[0])
mts = [x[0] for x in mt_sharpe]
sharpes = [x[1] for x in mt_sharpe]
colored = ["#2196F3" if m == 3 else "#F44336" if m == 12 else "#F48FB1" if m == 11 else "#90CAF9" for m in mts]

ax2.bar(mts, sharpes, color=colored, edgecolor="white", linewidth=0.5)
ax2.set_xlabel("MIN_TRADES value", fontsize=11)
ax2.set_ylabel("Avg Sharpe (OOS)", fontsize=11)
ax2.set_title("Sharpe by MIN_TRADES — winner MT=12 vs baseline MT=3", fontsize=11, style="italic")
ax2.axhline(y=sharpes[mts.index(3)], color="#2196F3", linestyle="--", alpha=0.6, linewidth=1, label="baseline MT=3")
ax2.axhline(y=max(sharpes), color="#F44336", linestyle="--", alpha=0.6, linewidth=1, label="winner MT=12")
ax2.legend(fontsize=9)
ax2.grid(True, axis="y", linestyle="--", alpha=0.4)
ax2.set_xticks(mts)

plt.tight_layout()
os.makedirs(os.path.dirname(OUT), exist_ok=True)
plt.savefig(OUT, dpi=150, bbox_inches="tight", facecolor="white")
print(f"Saved: {OUT}")
