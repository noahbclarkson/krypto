#!/usr/bin/env python3
"""
ATR_PERIOD Re-Optimization Comparison Chart
Data: snapshots/turtle_atr_prod_sweep.csv
Key equity: snapshots/turtle_atr_prod_key_equity.csv

Generates: charts/atr_prod_sweep_comparison.png
"""

import pandas as pd
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import numpy as np
import os

OUT = "charts/atr_prod_sweep_comparison.png"

# ── Load sweep summary ────────────────────────────────────────────────────────
sweep = pd.read_csv("snapshots/turtle_atr_prod_sweep.csv")

# Global aggregation
global_agg = (
    sweep
    .groupby("atr_period")
    .agg(
        avg_sharpe=("sharpe", "mean"),
        avg_return=("return_pct", "mean"),
        avg_max_dd=("max_dd_pct", "mean"),
        avg_trades=("trades", "mean"),
        pass_count=("pass", "sum"),
        pass_rate=("pass", lambda x: x.sum() / len(x) * 100),
    )
    .reset_index()
    .sort_values("atr_period")
)

print("Top 10 by Sharpe:")
print(global_agg.nlargest(10, "avg_sharpe")[["atr_period","avg_sharpe","pass_rate","avg_trades"]].to_string(index=False))

# ── Load key equity curves ──────────────────────────────────────────────────────
key_atrs = [16, 20, 24, 28, 32]
eq_dfs = {}
for atr in key_atrs:
    f = f"snapshots/turtle_atr_prod_equity.csv"
    try:
        df = pd.read_csv(f)
        df_atr = df[df["atr_period"] == atr]
        if not df_atr.empty:
            # Mean equity per bar across all universe/window combinations
            mean_eq = df_atr.groupby("bar")["equity"].mean()
            eq_dfs[atr] = mean_eq
            print(f"ATR={atr}: {len(mean_eq)} bars, final equity = {mean_eq.iloc[-1]:.4f}")
    except Exception as e:
        print(f"Could not load equity for ATR={atr}: {e}")

# ── Plot ───────────────────────────────────────────────────────────────────────
plt.style.use("seaborn-v0_8-whitegrid")
fig, axes = plt.subplots(2, 2, figsize=(16, 12))
fig.suptitle(
    "ATR_PERIOD Re-Optimization — Current Production Params\n"
    "EP=24, CHAND(7,2.30), HOLD_MAX=12, ATR_ENTRY_MULT=0.00 | 9 Universes × ~7 Windows",
    fontsize=13, fontweight="bold", y=0.98
)

COLORS = {
    16: "#2196F3",   # blue
    20: "#FF9800",   # orange
    24: "#9C27B0",   # purple (baseline)
    28: "#4CAF50",   # green (winner)
    32: "#F44336",   # red
}

# Panel A: Sharpe vs ATR Period
ax = axes[0, 0]
ax.plot(global_agg["atr_period"], global_agg["avg_sharpe"], 
        color="#1976D2", linewidth=2, zorder=5, marker='o', markersize=4)
ax.axvline(24, color="#9C27B0", linestyle="--", linewidth=1.5, alpha=0.7, label="ATR=24 [baseline]")
ax.axvline(28, color="#4CAF50", linestyle="--", linewidth=1.5, alpha=0.7, label="ATR=28 [winner]")

# Highlight baseline and winner
for _, row in global_agg.iterrows():
    if row["atr_period"] in [24, 28]:
        ax.scatter(row["atr_period"], row["avg_sharpe"], 
                   color=COLORS.get(row["atr_period"], "#333"), s=100, zorder=8,
                   marker="*" if row["atr_period"] == 28 else "D")

ax.set_xlabel("TURTLE_ATR_PERIOD")
ax.set_ylabel("Avg Walk-Forward Sharpe")
ax.set_title("A. Sharpe vs ATR Period\n(26 values, 10–60 step 2)")
ax.legend(fontsize=8)
ax.grid(True, alpha=0.3)

# Add text annotation for winner
win_row = global_agg[global_agg["atr_period"] == 28].iloc[0]
ax.annotate(f"Winner: ATR=28\nSharpe={win_row['avg_sharpe']:.2f}\n(Δ vs baseline: {win_row['avg_sharpe'] - global_agg[global_agg['atr_period']==24].iloc[0]['avg_sharpe']:+.2f})",
            xy=(28, win_row['avg_sharpe']),
            xytext=(38, win_row['avg_sharpe'] + 0.1),
            fontsize=8, color="#4CAF50",
            arrowprops=dict(arrowstyle="->", color="#4CAF50"))

# Panel B: Pass Rate vs ATR Period
ax = axes[0, 1]
ax.plot(global_agg["atr_period"], global_agg["pass_rate"],
        color="#388E3C", linewidth=2, marker='o', markersize=4)
ax.axhline(100, color="gray", linestyle=":", linewidth=1.5, alpha=0.7)
ax.set_xlabel("TURTLE_ATR_PERIOD")
ax.set_ylabel("Pass Rate (%)")
ax.set_title("B. Pass Rate vs ATR Period\n(NULL RESULT: 100% across ALL values)")
ax.grid(True, alpha=0.3)
ax.set_ylim([98, 101])

# Panel C: Equity Curves (log scale) — key configs
ax = axes[1, 0]
for atr, series in eq_dfs.items():
    label = f"ATR={atr}" + (" [winner]" if atr == 28 else " [baseline]" if atr == 24 else "")
    ax.plot(series.index, series.values, label=label,
             color=COLORS.get(atr, "#333"), linewidth=1.8, alpha=0.9)

ax.set_yscale("log")
ax.set_xlabel("Bar (walk-forward test window)")
ax.set_ylabel("Normalised Equity (log scale)")
ax.set_title("C. Equity Curves — Key ATR Periods\n(mean across 9 universes × ~7 windows)")
ax.legend(fontsize=8, loc="upper left")
ax.grid(True, which="both", alpha=0.3)

# Panel D: Full sweep — Sharpe heatmap-style bar chart
ax = axes[1, 1]
colors = ["#4CAF50" if r["atr_period"] in [28] else
          "#9C27B0" if r["atr_period"] in [24] else
          "#1976D2" for _, r in global_agg.iterrows()]
ax.bar(global_agg["atr_period"], global_agg["avg_sharpe"], color=colors, alpha=0.8, width=1.8)
ax.set_xlabel("TURTLE_ATR_PERIOD")
ax.set_ylabel("Avg Walk-Forward Sharpe")
ax.set_title("D. Full ATR Period Sweep (10–60 step 2)\n[Green=winner, Purple=baseline(ATR=24), Blue=others]")
ax.grid(True, alpha=0.3, axis="y")

plt.tight_layout(rect=[0, 0.03, 1, 0.95])
os.makedirs("charts", exist_ok=True)
plt.savefig(OUT, dpi=150, bbox_inches="tight")
print(f"\nSaved: {OUT}")
