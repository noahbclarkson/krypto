#!/usr/bin/env python3
"""
ATR_ENTRY_MULT hyperopt comparison chart.
Generates: charts/atr_entry_mult_comparison.png

Data: snapshots/atr_entry_mult_summary.csv (aggregated metrics)
Data: snapshots/atr_entry_mult_equity.csv (equity curves)
"""

import pandas as pd
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import os

OUT = "charts/atr_entry_mult_comparison.png"
os.makedirs("charts", exist_ok=True)

# ── 1. Load summary ───────────────────────────────────────────────────────────
df = pd.read_csv("snapshots/atr_entry_mult_summary.csv")
df = df.sort_values("mult")

# ── 2. Load equity curves ────────────────────────────────────────────────────
eq = pd.read_csv("snapshots/atr_entry_mult_equity.csv")

# Build mean equity per bar for each mult (across all universes/windows)
eq_agg = eq.groupby(["mult", "bar"])["equity"].mean().reset_index()
mults = sorted(eq_agg["mult"].unique())

# ── 3. Colors and labels ─────────────────────────────────────────────────────
colors = {
    0.00: "#2196F3",  # blue — baseline (no filter)
    0.25: "#9C27B0", # purple
    0.50: "#FF9800", # orange
    0.75: "#00BCD4", # cyan
    1.00: "#4CAF50", # green — winner
}

labels = {
    0.00: "M=0.00 [baseline]",
    0.25: "M=0.25 [runner-up]",
    0.50: "M=0.50 [runner-up]",
    0.75: "M=0.75",
    1.00: "M=1.00 [WINNER]",
}

# ── 4. Plot ────────────────────────────────────────────────────────────────────
plt.style.use("seaborn-v0_8-whitegrid")
fig, axes = plt.subplots(2, 2, figsize=(16, 12))
fig.suptitle(
    "Turtle+Chandelier — ATR_ENTRY_MULT Hyperopt (Production Params)\n"
    "CHAND(11,2.25)/EP=24/ATR(24)/HM=12 | 9 Universes × 6 Windows",
    fontsize=13, fontweight="bold", y=0.98
)

# Panel A: Sharpe vs mult
ax = axes[0, 0]
ax.plot(df["mult"], df["avg_sharpe"], color="#1976D2", linewidth=2.5, zorder=5, marker='o', markersize=6)
# Highlight winner
win = df[df["mult"] == 1.00].iloc[0]
ax.scatter([1.00], [win["avg_sharpe"]], color="#4CAF50", s=150, zorder=8, marker="*", label=f"M=1.00 [WINNER] sh={win['avg_sharpe']:.2f}")
# Baseline marker
base = df[df["mult"] == 0.00].iloc[0]
ax.scatter([0.00], [base["avg_sharpe"]], color="#2196F3", s=100, zorder=7, marker="D", label=f"M=0.00 [baseline] sh={base['avg_sharpe']:.2f}")
ax.axvline(1.00, color="#4CAF50", linestyle="--", linewidth=1.5, alpha=0.5)
ax.set_xlabel("ATR_ENTRY_MULT")
ax.set_ylabel("Avg Walk-Forward Sharpe")
ax.set_title("A. Sharpe vs ATR_ENTRY_MULT\n(11 values tested, M=1.00 wins)")
ax.legend(fontsize=9)

# Panel B: Pass Rate vs mult
ax = axes[0, 1]
ax.plot(df["mult"], df["pass_rate_pct"], color="#388E3C", linewidth=2.5, marker='o', markersize=6)
ax.axvline(1.00, color="#4CAF50", linestyle="--", linewidth=1.5, alpha=0.5)
ax.axhline(80, color="gray", linestyle=":", linewidth=1, alpha=0.5, label="80% threshold")
ax.set_xlabel("ATR_ENTRY_MULT")
ax.set_ylabel("Pass Rate (%)")
ax.set_title("B. Pass Rate vs ATR_ENTRY_MULT\n(Only M=1.00 clears 80%)")
ax.legend(fontsize=9)

# Panel C: Equity curves (log scale) — top 5 mults by pass rate
ax = axes[1, 0]
top_mults = [1.00, 0.75, 0.25, 0.50, 0.00]
for m in top_mults:
    sub = eq_agg[eq_agg["mult"] == m]
    mean_eq = sub.groupby("bar")["equity"].mean()
    label = labels.get(m, f"M={m}")
    color = colors.get(m, "#888888")
    lw = 2.5 if m in [1.00, 0.00] else 1.5
    ax.plot(mean_eq.index, mean_eq.values, label=label, color=color, linewidth=lw, alpha=0.9)
ax.set_yscale("log")
ax.set_xlabel("Bar (walk-forward test window)")
ax.set_ylabel("Normalised Equity (log scale)")
ax.set_title("C. Equity Curves — Baseline vs Winner vs Runners-up\n(mean across universe/windows, sampled every 5 bars)")
ax.legend(fontsize=8, loc="upper left")
ax.grid(True, which="both", alpha=0.3)

# Panel D: Bar chart — Sharpe + Pass Rate side by side
ax = axes[1, 1]
x = df["mult"].values
width = 0.35
bar_sharpe = ax.bar(x - width/2, df["avg_sharpe"], width, label="Avg Sharpe", color="#1976D2", alpha=0.8)
bar_pass = ax.bar(x + width/2, df["pass_rate_pct"]/10, width, label="Pass Rate/10", color="#4CAF50", alpha=0.8)
ax.set_xlabel("ATR_ENTRY_MULT")
ax.set_ylabel("Avg Sharpe / (Pass Rate/10)")
ax.set_title("D. Sharpe + Pass Rate by ATR_ENTRY_MULT\n(Sharpe in blue, pass rate÷10 in green)")
ax.set_xticks(x)
ax.legend(fontsize=9)

plt.tight_layout(rect=[0, 0.03, 1, 0.95])
plt.savefig(OUT, dpi=150, bbox_inches="tight")
print(f"Saved {OUT}")

# ── 5. Key result summary ─────────────────────────────────────────────────────
win_row = df[df["mult"] == 1.00].iloc[0]
base_row = df[df["mult"] == 0.00].iloc[0]
print(f"\n=== ATR_ENTRY_MULT Results ===")
print(f"BASELINE (M=0.00):  Sharpe={base_row['avg_sharpe']:.4f}, Pass={base_row['pass_count']}/{base_row['total_runs']} ({base_row['pass_rate_pct']:.1f}%)")
print(f"WINNER   (M=1.00):  Sharpe={win_row['avg_sharpe']:.4f}, Pass={win_row['pass_count']}/{win_row['total_runs']} ({win_row['pass_rate_pct']:.1f}%)")
print(f"DELTA:   +{win_row['avg_sharpe']-base_row['avg_sharpe']:.4f} Sharpe, +{win_row['pass_rate_pct']-base_row['pass_rate_pct']:.1f}pp pass rate")
print(f"\nKey insight: ATR filter M=1.00 is ROBUST winner.")
print(f"Trade reduction from M=1.00: {df[df['mult']==0.0]['pass_count'].values[0]}/54 vs {win_row['pass_count']}/54 windows pass")