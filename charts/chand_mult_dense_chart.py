#!/usr/bin/env python3
"""
CHAND_MULT Dense Sweep Comparison Chart
========================================
Generates: charts/chand_mult_dense_comparison.png

Data sources:
  snapshots/chand_mult_dense_sweep.csv       — 71 values (M∈[1.50..5.00] step 0.05)
  snapshots/chand_mult_dense_equity_curves.csv — mean equity per bar for top 5 M values

Panel layout:
  A. Sharpe vs CHAND_MULT (71 values, dense sweep)
  B. Pass Rate vs CHAND_MULT
  C. Log-scale equity curves for top 5 + baseline
  D. Return vs CHAND_MULT with winner annotated
"""

import pandas as pd
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import matplotlib.ticker as mticker
import os

SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(SCRIPT_DIR, "chand_mult_dense_comparison.png")

# ── Load sweep summary ────────────────────────────────────────────────────────
sweep_path = os.path.join(SCRIPT_DIR, "..", "snapshots", "chand_mult_dense_sweep.csv")
sweep = pd.read_csv(sweep_path)
sweep = sweep.sort_values("chand_mult").reset_index(drop=True)

# ── Load equity curves ─────────────────────────────────────────────────────────
eq_path = os.path.join(SCRIPT_DIR, "..", "snapshots", "chand_mult_dense_equity_curves.csv")
equity_raw = pd.read_csv(eq_path)

# Pivot: bar -> {M=2.25: mean_equity, M=2.15: mean_equity, ...}
# Equity is already aggregated as mean across windows per M value
top5_ms = sorted(equity_raw["chand_mult"].unique())
eq_pivot = equity_raw.pivot(index="bar", columns="chand_mult", values="mean_equity")
eq_pivot = eq_pivot.sort_index()

# ── Identify baseline (M=2.25 current default) and winner ─────────────────────
baseline_m = 2.25
sweep["sharpe_rank"] = sweep["avg_sharpe"].rank(ascending=False)
winner_row = sweep.loc[sweep["avg_sharpe"].idxmax()]
winner_m = winner_row["chand_mult"]
winner_sh = winner_row["avg_sharpe"]
baseline_row = sweep[sweep["chand_mult"] == baseline_m].iloc[0] if baseline_m in sweep["chand_mult"].values else None

print(f"Benchmark: M={baseline_m} → Sharpe={baseline_row['avg_sharpe']:.4f} (rank #{int(baseline_row['sharpe_rank'])})")
print(f"Winner:    M={winner_m:.2f} → Sharpe={winner_sh:.4f} (rank #1)")
print(f"Delta:     {winner_sh - (baseline_row['avg_sharpe'] if baseline_row is not None else 0):+.4f}")

# ── Color scheme ──────────────────────────────────────────────────────────────
COLORS = {
    2.25: "#1976D2",  # blue — baseline
}
import matplotlib.cm as cm
cmap = cm.get_cmap("tab10")
for i, m in enumerate(top5_ms):
    if m not in COLORS:
        COLORS[m] = cmap(i % 10)

# Star marker for winner
star_color = "#4CAF50"

# ── Plot ───────────────────────────────────────────────────────────────────────
plt.style.use("seaborn-v0_8-whitegrid")
fig, axes = plt.subplots(2, 2, figsize=(16, 12))
fig.suptitle(
    "Turtle+Chandelier — CHAND_MULT Dense Sweep\n"
    f"71 values · M∈[1.50..5.00] step=0.05 · 9 universes × 54 walk-forward windows",
    fontsize=13, fontweight="bold", y=0.99
)

# ── Panel A: Sharpe vs CHAND_MULT ──────────────────────────────────────────────
ax = axes[0, 0]
ax.plot(sweep["chand_mult"], sweep["avg_sharpe"], color="#1976D2", linewidth=2, zorder=5)
ax.scatter(sweep["chand_mult"], sweep["avg_sharpe"], color="#1976D2", s=20, zorder=6, alpha=0.7)

# Highlight winner
ax.scatter([winner_m], [winner_sh], color=star_color, s=150, zorder=10,
           marker="*", label=f"Winner M={winner_m:.2f} (sh={winner_sh:.3f})")
ax.annotate(
    f"M={winner_m:.2f}\nsh={winner_sh:.3f}",
    xy=(winner_m, winner_sh),
    xytext=(winner_m + 0.25, winner_sh + 0.05),
    fontsize=8, color=star_color,
    arrowprops=dict(arrowstyle="->", color=star_color, lw=1.2)
)

# Highlight baseline
if baseline_row is not None:
    ax.scatter([baseline_m], [baseline_row["avg_sharpe"]], color=COLORS[baseline_m],
               s=100, zorder=9, marker="D",
               label=f"Current default M={baseline_m} (sh={baseline_row['avg_sharpe']:.3f})")

ax.axvline(winner_m, color=star_color, linestyle="--", linewidth=1, alpha=0.4)
ax.set_xlabel("CHAND_MULT (M)")
ax.set_ylabel("Avg Walk-Forward Sharpe")
ax.set_title("A. Sharpe vs CHAND_MULT\n(Dense sweep: step=0.05, 71 values)")
ax.legend(fontsize=8)

# ── Panel B: Pass Rate vs CHAND_MULT ─────────────────────────────────────────
ax = axes[0, 1]
ax.plot(sweep["chand_mult"], sweep["pass_rate"], color="#388E3C", linewidth=2)
ax.scatter(sweep["chand_mult"], sweep["pass_rate"], color="#388E3C", s=20, alpha=0.7)
ax.axvline(winner_m, color=star_color, linestyle="--", linewidth=1, alpha=0.4)
ax.axhline(100, color="gray", linestyle=":", linewidth=1, alpha=0.5)
ax.set_xlabel("CHAND_MULT (M)")
ax.set_ylabel("Pass Rate (%)")
ax.set_title("B. Pass Rate vs CHAND_MULT")
ax.set_ylim(0, 110)

# ── Panel C: Equity curves (log scale) ─────────────────────────────────────────
ax = axes[1, 0]
if not eq_pivot.empty:
    # Plot all top 5 equity curves
    for m_val in sorted(eq_pivot.columns):
        label = f"M={m_val:.2f} {'[baseline]' if m_val == baseline_m else ('[winner]' if m_val == winner_m else '')}"
        color = COLORS.get(m_val, cmap(0))
        lw = 2.5 if m_val in (baseline_m, winner_m) else 1.4
        alpha = 0.95 if m_val in (baseline_m, winner_m) else 0.7
        ax.plot(eq_pivot.index, eq_pivot[m_val], label=label, color=color,
                linewidth=lw, alpha=alpha)

    ax.set_yscale("log")
    ax.set_xlabel("Bar (walk-forward test window)")
    ax.set_ylabel("Normalised Equity (log scale)")
    ax.set_title("C. Equity Curves — Top 5 CHAND_MULT values\n(mean across all universe/windows)")
    ax.legend(fontsize=8, loc="upper left")
    ax.grid(True, which="both", alpha=0.3)

    # Annotate final equity for winner and baseline
    final = eq_pivot.iloc[-1]
    if winner_m in final.index:
        ax.annotate(f"M={winner_m:.2f} final: {final[winner_m]:.3f}x",
                   xy=(final.name, final[winner_m]),
                   xytext=(20, -20), textcoords="offset points",
                   fontsize=7, color=star_color)
else:
    ax.text(0.5, 0.5, "Equity data not yet available", transform=ax.transAxes,
            ha="center", va="center", fontsize=11, color="gray")

# ── Panel D: Return vs CHAND_MULT ─────────────────────────────────────────────
ax = axes[1, 1]
ax.plot(sweep["chand_mult"], sweep["avg_return"], color="#E91E63", linewidth=2)
ax.scatter(sweep["chand_mult"], sweep["avg_return"], color="#E91E63", s=20, alpha=0.7)
ax.axvline(winner_m, color=star_color, linestyle="--", linewidth=1, alpha=0.4)

# Shade the region between baseline and winner
if baseline_row is not None:
    ax.fill_between(
        sorted([baseline_m, winner_m]), 0, 500,
        alpha=0.1, color=star_color, label="Improvement zone"
    )

ax.set_xlabel("CHAND_MULT (M)")
ax.set_ylabel("Avg Walk-Forward Return (%)")
ax.set_title("D. Return vs CHAND_MULT")
ax.set_ylim(bottom=0)

plt.tight_layout(rect=[0, 0.03, 1, 0.95])
os.makedirs(os.path.dirname(OUT), exist_ok=True)
plt.savefig(OUT, dpi=150, bbox_inches="tight")
print(f"\nSaved: {OUT}")
