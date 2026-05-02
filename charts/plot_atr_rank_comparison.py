#!/usr/bin/env python3
"""ATR_RANK threshold equity comparison chart.

Generates: charts/comparison_chart.png
Data: snapshots/atr_rank_equity_comparison.csv (T=0, T=5, T=24, Base5, 7 windows)
"""
import pandas as pd
import matplotlib.pyplot as plt
import matplotlib.ticker as mticker
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
EQ_PATH = ROOT / "snapshots" / "atr_rank_equity_comparison.csv"
OUT_PATH = ROOT / "charts" / "comparison_chart.png"

# Load equity data
df = pd.read_csv(EQ_PATH)
print("Columns:", list(df.columns))
print(df.head(3))

plt.style.use("seaborn-v0_8-whitegrid")
fig, ax = plt.subplots(figsize=(14, 8), dpi=160)

# Color palette: dark baseline, mid runner-up, highlight winner
colors = {
    "T_0":  "#9ca3af",   # grey — baseline (no filter)
    "T_5":  "#2563eb",   # blue — runner-up (old default)
    "T_24": "#d97706",   # amber — WINNER (current production)
}

configs = ["T_0", "T_5", "T_24"]
labels = {
    "T_0":  r"T=0 (no filter) — Baseline",
    "T_5":  r"T=5 (old default) — Runner-up",
    "T_24": r"T=24 (production) — Winner",
}
linewidths = {"T_0": 1.8, "T_5": 1.8, "T_24": 2.8}

for cfg in configs:
    col = cfg
    if col not in df.columns:
        print(f"Warning: {col} not in columns {list(df.columns)}")
        continue
    equity = df[col].values
    windows = df["window"].values

    lw = linewidths[cfg]
    color = colors[cfg]

    ax.plot(windows, equity, label=labels[cfg], linewidth=lw, color=color, marker='o', markersize=5)

    # Annotate final value
    final = equity[-1]
    ax.annotate(
        f"  {final:.1f}x",
        xy=(windows[-1], final),
        fontsize=9,
        color=color,
        va="center",
    )

ax.set_xlabel("Walk-Forward Window", fontsize=11)
ax.set_ylabel("Portfolio Equity (compounded, log scale)", fontsize=11)
ax.set_title(
    "ATR_RANK Threshold: Baseline (T=0) vs Old Default (T=5) vs Winner (T=24)\n"
    "Base5 walk-forward, 7 windows — Turtle-only live path",
    fontsize=13,
)
ax.legend(fontsize=10, framealpha=0.9, loc="upper left")
ax.yaxis.set_major_formatter(mticker.FuncFormatter(lambda x, _: f"{x:.0f}x"))
ax.set_yscale("log")
ax.grid(True, alpha=0.4)

# Caption box
caption = (
    "T=24 Winner: 7/7 pass, Sharpe 7.22 avg | "
    "T=5: 4/7 pass, Sharpe 3.69 avg | "
    "T=0: 6/7 pass, Sharpe 3.64 avg\n"
    "Live Turtle-only path: Turtle breakout + ATR_RANK gate + Turtle ATR exit"
)
ax.text(
    0.5, -0.12, caption,
    transform=ax.transAxes,
    ha="center", fontsize=8.5,
    color="#4b5563",
    style="italic",
)

plt.tight_layout()
plt.savefig(OUT_PATH, bbox_inches="tight", dpi=160)
print(f"Saved {OUT_PATH}")
plt.close()