#!/usr/bin/env python3
"""Plot fee sensitivity equity curves for fee_sweep_walkforward."""
from pathlib import Path
import pandas as pd
import matplotlib.pyplot as plt

ROOT = Path(__file__).resolve().parents[1]
summary_path = ROOT / "snapshots" / "fee_sweep_walkforward_summary.csv"
equity_path = ROOT / "snapshots" / "fee_sweep_walkforward_equity.csv"
out_path = ROOT / "charts" / "comparison_chart.png"

summary = pd.read_csv(summary_path)
equity = pd.read_csv(equity_path)

# Baseline = historical hardcoded harness fee; winner = best robust pass/share Sharpe.
baseline_bps = 10.0
# Fee is exogenous, so "winner" for charting is the best realistic live-cost calibration, not a tradable hyperparam.
live_bps = 4.0
# Include a no-fee upper bound and two stress cases.
plot_bps = [0.0, live_bps, baseline_bps, 15.0, 20.0]
labels = {
    0.0: "No-fee upper bound (0 bps)",
    4.0: "Live config / Binance taker (4 bps)",
    10.0: "Historical baseline (10 bps)",
    15.0: "Stress (15 bps)",
    20.0: "Stress (20 bps)",
}
colors = {
    0.0: "#2ca02c",
    4.0: "#1f77b4",
    10.0: "#ff7f0e",
    15.0: "#9467bd",
    20.0: "#d62728",
}

fig, ax = plt.subplots(figsize=(15, 8), dpi=160)
for bps in plot_bps:
    df = equity[equity["fee_bps"].round(1) == bps].copy()
    if df.empty:
        continue
    df = df.sort_values("step")
    ax.plot(df["step"], df["equity"], label=labels.get(bps, f"{bps:.1f} bps"), linewidth=2.0, color=colors.get(bps))

ax.set_title("Turtle+Chandelier Fee Sensitivity — Composite 9-Universe Walk-Forward Equity")
ax.set_xlabel("Sequential OOS validation step (all universes/windows concatenated)")
ax.set_ylabel("Composite equity (log scale, normalized to 1.0)")
ax.set_yscale("log")
ax.grid(True, which="both", alpha=0.28)
ax.legend(loc="best")

# Dynamic Y limits: no forced zero, with a small margin around actual plotted range.
subset = equity[equity["fee_bps"].round(1).isin(plot_bps)]
ymin = max(subset["equity"].min() * 0.85, 1e-6)
ymax = subset["equity"].max() * 1.15
ax.set_ylim(ymin, ymax)

# Add a concise metrics box.
rows = []
for bps in plot_bps:
    s = summary[summary["fee_bps"].round(1) == bps]
    if not s.empty:
        r = s.iloc[0]
        rows.append(f"{bps:>4.1f} bps: pass {int(r['pass'])}/{int(r['total'])}, sh {r['avg_sharpe']:.2f}, ret {r['avg_return_pct']:+.1f}%")
if rows:
    ax.text(0.01, 0.02, "\n".join(rows), transform=ax.transAxes, fontsize=9,
            va="bottom", ha="left", bbox=dict(boxstyle="round", facecolor="white", alpha=0.78, edgecolor="gray"))

fig.tight_layout()
out_path.parent.mkdir(parents=True, exist_ok=True)
fig.savefig(out_path)
print(out_path)
