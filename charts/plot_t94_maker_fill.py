#!/usr/bin/env python3
"""Chart T94 maker-fill scenario results."""
import subprocess

import pandas as pd

# Read CSV
df = pd.read_csv("krypto/snapshots/t94_maker_fill_scenarios.csv")

# Strip quotes from label column
df["scenario"] = df["scenario"].str.strip('"')

import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
import matplotlib.ticker as mticker

fig, axes = plt.subplots(1, 2, figsize=(14, 5))

# --- Left: Equity multiplies (log scale) ---
ax = axes[0]
colors = ["#e74c3c", "#f39c12", "#27ae60", "#2980b9"]
bars = ax.bar(df["scenario"], df["equity_mult"], color=colors, edgecolor="white", linewidth=0.8)
ax.set_yscale("log")
ax.set_title("Equity by Maker-Fill Scenario", fontsize=13, fontweight="bold")
ax.set_ylabel("Equity Multiple (log scale)")
ax.set_xlabel("")
ax.yaxis.set_major_formatter(mticker.FuncFormatter(lambda v, _: f"{v:.2f}x"))
for bar, eq in zip(bars, df["equity_mult"]):
    ax.text(bar.get_x() + bar.get_width()/2, bar.get_height()*1.01,
            f"{eq:.3f}x", ha="center", va="bottom", fontsize=9)
ax.set_ylim(2.5, 3.0)
ax.tick_params(axis="x", rotation=15)
ax.grid(axis="y", alpha=0.3)

# --- Right: Sharpe + MaxDD scatter ---
ax2 = axes[1]
sc = ax2.scatter(df["sharpe"], df["max_dd_pct"],
                c=df["maker_fill_pct"], cmap="RdYlGn",
                s=200, zorder=5, edgecolors="white", linewidths=1.5)
for _, row in df.iterrows():
    ax2.annotate(row["scenario"], (row["sharpe"], row["max_dd_pct"]),
                 textcoords="offset points", xytext=(8, 0), fontsize=8)
ax2.set_xlabel("Annualised Sharpe")
ax2.set_ylabel("Max Drawdown (%)")
ax2.set_title("Sharpe vs MaxDD by Scenario", fontsize=13, fontweight="bold")
ax2.invert_yaxis()
ax2.grid(alpha=0.3)
cbar = plt.colorbar(sc, ax=ax2)
cbar.set_label("Maker Fill %")

fig.suptitle("T94: Maker-Fill Scenario Analysis — Fee Impact Is Minimal",
             fontsize=14, fontweight="bold", y=1.01)
plt.tight_layout()
plt.savefig("krypto/charts/t94_maker_fill_scenarios.png", dpi=150, bbox_inches="tight")
print("Saved: krypto/charts/t94_maker_fill_scenarios.png")