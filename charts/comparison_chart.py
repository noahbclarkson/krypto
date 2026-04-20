#!/usr/bin/env python3
"""
CHAND_MULT vs CHAND_PERIOD comparison chart.
Generates: charts/comparison_chart.png

Data sources:
  snapshots/chand_mult_sweep.csv          — aggregated metrics by M
  snapshots/chand_mult_2.00_equity.csv    — baseline equity curve
  snapshots/chand_mult_2.25_equity.csv     — winner equity curve
  snapshots/chand_mult_2.15_equity.csv     — runner-up equity curve
  snapshots/chand_mult_2.20_equity.csv     — runner-up equity curve
  snapshots/chand_period_sweep.csv         — per-window results by CP
"""

import pandas as pd
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import os

OUT = "charts/comparison_chart.png"

# ── 1. Load sweep summaries ───────────────────────────────────────────────────
mult_sweep = pd.read_csv("snapshots/chand_mult_sweep.csv")

period_raw = pd.read_csv("snapshots/chand_period_sweep.csv")
period_agg = (
    period_raw
    .rename(columns={"cp": "chand_period"})
    .groupby("chand_period")
    .agg(
        avg_sharpe=("sharpe", "mean"),
        avg_return=("return_pct", "mean"),
        avg_max_dd=("max_dd_pct", "mean"),
        avg_trades=("trades", "mean"),
        pass_rate=("pass", lambda x: x.sum() / len(x)),
    )
    .reset_index()
)

# ── 2. Load equity curves ──────────────────────────────────────────────────────
eq_m200 = pd.read_csv("snapshots/chand_mult_2.00_equity.csv")
eq_m215 = pd.read_csv("snapshots/chand_mult_2.15_equity.csv")
eq_m220 = pd.read_csv("snapshots/chand_mult_2.20_equity.csv")
eq_m225 = pd.read_csv("snapshots/chand_mult_2.25_equity.csv")

def build_series(df, label):
    """Mean equity per bar across all universe/window rows."""
    return df.groupby("bar")["equity"].mean().rename(label)

eq = pd.DataFrame({
    "M=2.00 [baseline]": build_series(eq_m200, "M=2.00 [baseline]"),
    "M=2.15":            build_series(eq_m215, "M=2.15"),
    "M=2.20":            build_series(eq_m220, "M=2.20"),
    "M=2.25 [winner]":   build_series(eq_m225, "M=2.25 [winner]"),
})

COLORS = {
    "M=2.00 [baseline]": "#2196F3",
    "M=2.15":            "#FF9800",
    "M=2.20":            "#9C27B0",
    "M=2.25 [winner]":   "#4CAF50",
}

# ── 3. Plot ────────────────────────────────────────────────────────────────────
plt.style.use("seaborn-v0_8-whitegrid")
fig, axes = plt.subplots(2, 2, figsize=(16, 12))
fig.suptitle(
    "Turtle+Chandelier — Hyperparameter Optimization: CHAND_MULT × CHAND_PERIOD",
    fontsize=14, fontweight="bold", y=0.98
)

# Panel A: Sharpe vs M
ax = axes[0, 0]
ms = mult_sweep.sort_values("chand_mult")
ax.plot(ms["chand_mult"], ms["avg_sharpe"], color="#1976D2", linewidth=2, zorder=5)
ax.scatter(ms["chand_mult"], ms["avg_sharpe"], color="#1976D2", s=30, zorder=6)

win = ms[ms["chand_mult"] == 2.25]
if not win.empty:
    ax.scatter(win["chand_mult"], win["avg_sharpe"],
               color="#4CAF50", s=120, zorder=8, marker="*", label="M=2.25 [winner]")
    ax.annotate(f"M=2.25  sh={win['avg_sharpe'].values[0]:.2f}",
               xy=(2.25, win["avg_sharpe"].values[0]),
               xytext=(2.6, win["avg_sharpe"].values[0] + 0.2),
               fontsize=8, color="#4CAF50",
               arrowprops=dict(arrowstyle="->", color="#4CAF50"))

base = ms[ms["chand_mult"] == 2.0]
if not base.empty:
    ax.scatter(base["chand_mult"], base["avg_sharpe"],
               color="#2196F3", s=80, zorder=7, marker="D", label="M=2.00 [baseline]")

ax.axvline(2.25, color="#4CAF50", linestyle="--", linewidth=1, alpha=0.5)
ax.set_xlabel("CHAND_MULT (M)")
ax.set_ylabel("Avg Walk-Forward Sharpe (9 universes)")
ax.set_title("A. Sharpe vs CHAND_MULT\n(M∈[0.50..5.00] step 0.25, 19 values)")
ax.legend(fontsize=8)

# Panel B: Pass Rate vs M
ax = axes[0, 1]
ax.plot(ms["chand_mult"], ms["pass_rate"] * 100, color="#388E3C", linewidth=2)
ax.scatter(ms["chand_mult"], ms["pass_rate"] * 100, color="#388E3C", s=30)
ax.axvline(2.25, color="#4CAF50", linestyle="--", linewidth=1, alpha=0.5)
ax.axhline(100, color="gray", linestyle=":", linewidth=1, alpha=0.5)
ax.set_xlabel("CHAND_MULT (M)")
ax.set_ylabel("Pass Rate (%)")
ax.set_title("B. Pass Rate vs CHAND_MULT\n(M∈[0.50..5.00] step 0.25)")

# Panel C: Equity curves (log scale)
ax = axes[1, 0]
for col in eq.columns:
    ax.plot(eq.index, eq[col], label=col, color=COLORS[col], linewidth=1.8, alpha=0.9)
ax.set_yscale("log")
ax.set_xlabel("Bar (walk-forward test window)")
ax.set_ylabel("Normalised Equity (log scale)")
ax.set_title("C. Equity Curves — M∈{2.00, 2.15, 2.20, 2.25}\n(mean across universe/windows)")
ax.legend(fontsize=8, loc="upper left")
ax.grid(True, which="both", alpha=0.3)

# Panel D: Sharpe vs CP
ax = axes[1, 1]
pa = period_agg.sort_values("chand_period")
ax.plot(pa["chand_period"], pa["avg_sharpe"], color="#E91E63", linewidth=2, zorder=5)
ax.scatter(pa["chand_period"], pa["avg_sharpe"], color="#E91E63", s=30, zorder=6)

win_cp = pa[pa["chand_period"] == 11]
if not win_cp.empty:
    ax.scatter(win_cp["chand_period"], win_cp["avg_sharpe"],
               color="#4CAF50", s=120, zorder=8, marker="*")
    ax.annotate(f"CP=11  sh={win_cp['avg_sharpe'].values[0]:.2f}",
               xy=(11, win_cp["avg_sharpe"].values[0]),
               xytext=(15, win_cp["avg_sharpe"].values[0] + 0.15),
               fontsize=8, color="#4CAF50",
               arrowprops=dict(arrowstyle="->", color="#4CAF50"))

base_cp = pa[pa["chand_period"] == 15]
if not base_cp.empty:
    ax.scatter(base_cp["chand_period"], base_cp["avg_sharpe"],
               color="#2196F3", s=80, zorder=7, marker="D", label="CP=15 [baseline]")

ax.axvline(11, color="#4CAF50", linestyle="--", linewidth=1, alpha=0.5)
ax.set_xlabel("CHAND_PERIOD (P)")
ax.set_ylabel("Avg Walk-Forward Sharpe (9 universes)")
ax.set_title("D. Sharpe vs CHAND_PERIOD\n(P∈[5..60] step 2, 33 values)")
ax.legend(fontsize=8)

plt.tight_layout(rect=[0, 0.03, 1, 0.96])
os.makedirs("charts", exist_ok=True)
plt.savefig(OUT, dpi=150, bbox_inches="tight")
print(f"Saved {OUT}")
