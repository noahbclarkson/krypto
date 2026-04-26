#!/usr/bin/env python3
"""
CHAND_PERIOD Dense Sweep — Comparison Chart
==========================================
Generates: charts/chand_period_dense_comparison.png

Data sources:
  snapshots/chand_period_dense_sweep.csv  — 56 values (CP 5..60, step=1)
  snapshots/chand_period_dense_equity.csv — equity curves for top 5 + baseline

Panel layout:
  A. Sharpe vs CP (56 values, dense sweep) — with winner annotated
  B. Pass Rate vs CP
  C. Log-scale equity curves (baseline CP=7 + top 5 winners)
  D. Return vs CP
"""

import pandas as pd
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import os

SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(SCRIPT_DIR, "..", "charts", "chand_period_dense_comparison.png")

# ── Load sweep summary ────────────────────────────────────────────────────────
sweep = pd.read_csv(os.path.join(SCRIPT_DIR, "..", "snapshots", "chand_period_dense_sweep.csv"))

# Aggregate by CP
cp_agg = (sweep.groupby("cp")
    .agg(pass_cnt=("pass", "sum"),
         total=("pass", "count"),
         avg_sharpe=("sharpe", "mean"),
         avg_ret=("ret_pct", "mean"),
         avg_dd=("max_dd_pct", "mean"),
         avg_equity=("equity_final", "mean"))
    .reset_index())
cp_agg["pass_rate"] = cp_agg["pass_cnt"] / cp_agg["total"] * 100
cp_agg = cp_agg.sort_values("cp").reset_index(drop=True)

# ── Load equity curves ─────────────────────────────────────────────────────────
eq = pd.read_csv(os.path.join(SCRIPT_DIR, "..", "snapshots", "chand_period_dense_equity.csv"))
eq_pivot = eq.pivot(index="bar", columns="cp", values="equity_mult")

# ── Identify baseline and winner ──────────────────────────────────────────────
baseline_cp = 7
winner_row = cp_agg.loc[cp_agg["avg_sharpe"].idxmax()]
winner_cp = int(winner_row["cp"])
winner_sh = winner_row["avg_sharpe"]
baseline_row = cp_agg[cp_agg["cp"] == baseline_cp].iloc[0]
baseline_sh = baseline_row["avg_sharpe"]
delta_pct = (winner_sh - baseline_sh) / abs(baseline_sh) * 100

print(f"Baseline:  CP={baseline_cp} → Sharpe={baseline_sh:.4f}, pass={baseline_row['pass_cnt']}/{baseline_row['total']}")
print(f"Winner:    CP={winner_cp} → Sharpe={winner_sh:.4f}, pass={winner_row['pass_cnt']}/{winner_row['total']}")
print(f"Delta:     {delta_pct:+.1f}%")

# ── Colors ──────────────────────────────────────────────────────────────────────
COLORS = {
    7:  "#1976D2",   # blue — baseline
}
import matplotlib.cm as cm
cmap = cm.get_cmap("tab10")
all_cps = sorted(eq_pivot.columns)
for i, cp in enumerate(all_cps):
    if cp not in COLORS:
        COLORS[cp] = cmap(i % 10)
star_color = "#4CAF50"
winner_color = "#E91E63"

# ── Plot ───────────────────────────────────────────────────────────────────────
plt.style.use("seaborn-v0_8-whitegrid")
fig, axes = plt.subplots(2, 2, figsize=(16, 12))
fig.suptitle(
    f"Turtle+Chandelier — CHAND_PERIOD Dense Sweep (Step=1)\n"
    f"56 values · CP∈[5..60] · EP=21/CM=2.30/HM=12/EM=0.00 · 9 universes × 6 windows",
    fontsize=13, fontweight="bold", y=0.99
)

# ── Panel A: Sharpe vs CP ──────────────────────────────────────────────────────
ax = axes[0, 0]
ax.plot(cp_agg["cp"], cp_agg["avg_sharpe"], color="#1976D2", linewidth=2, zorder=5)
ax.scatter(cp_agg["cp"], cp_agg["avg_sharpe"], color="#1976D2", s=20, zorder=6, alpha=0.7)

# Winner star
ax.scatter([winner_cp], [winner_sh], color=star_color, s=200, zorder=10,
           marker="*", label=f"Winner CP={winner_cp} (sh={winner_sh:.3f})")
ax.annotate(
    f"CP={winner_cp}\nsh={winner_sh:.3f}\npass={winner_row['pass_cnt']}/{winner_row['total']}",
    xy=(winner_cp, winner_sh),
    xytext=(winner_cp + 5, winner_sh + 0.1),
    fontsize=8, color=star_color, fontweight="bold",
    arrowprops=dict(arrowstyle="->", color=star_color, lw=1.2)
)

# Baseline diamond
ax.scatter([baseline_cp], [baseline_sh], color=COLORS[baseline_cp],
           s=120, zorder=9, marker="D",
           label=f"Prior default CP={baseline_cp} (sh={baseline_sh:.3f})")
ax.axvline(winner_cp, color=star_color, linestyle="--", linewidth=1, alpha=0.4)
ax.axvline(baseline_cp, color=COLORS[baseline_cp], linestyle=":", linewidth=1.5, alpha=0.6)
ax.set_xlabel("CHAND_PERIOD (ATR lookback)")
ax.set_ylabel("Avg Walk-Forward Sharpe")
ax.set_title("A. Sharpe vs CHAND_PERIOD\n(Dense sweep: step=1, 56 values)")
ax.legend(fontsize=8)

# Shade plateau region
plateau_cps = cp_agg[(cp_agg["cp"] >= 38) & (cp_agg["cp"] <= 46)]
ax.axvspan(38, 46, alpha=0.06, color=star_color, label="Robustness plateau (38-46)")

# ── Panel B: Pass Rate vs CP ──────────────────────────────────────────────────
ax = axes[0, 1]
ax.plot(cp_agg["cp"], cp_agg["pass_rate"], color="#388E3C", linewidth=2)
ax.scatter(cp_agg["cp"], cp_agg["pass_rate"], color="#388E3C", s=20, alpha=0.7)
ax.axvline(winner_cp, color=star_color, linestyle="--", linewidth=1, alpha=0.4)
ax.axvline(baseline_cp, color=COLORS[baseline_cp], linestyle=":", linewidth=1.5, alpha=0.6)
ax.axhline(70, color="gray", linestyle=":", linewidth=1, alpha=0.5)
ax.set_xlabel("CHAND_PERIOD")
ax.set_ylabel("Pass Rate (%)")
ax.set_title("B. Pass Rate vs CHAND_PERIOD")
ax.set_ylim(40, 80)

# ── Panel C: Equity curves (log scale) ─────────────────────────────────────────
ax = axes[1, 0]
top5_in_eq = [cp for cp in [42, 43, 41, 44, 40] if cp in eq_pivot.columns]
cols_to_plot = top5_in_eq + [c for c in eq_pivot.columns if c not in top5_in_eq]

for cp_val in cols_to_plot:
    label_extra = ""
    if cp_val == baseline_cp:
        label_extra = " [baseline]"
    elif cp_val == winner_cp:
        label_extra = " [winner]"
    elif cp_val in top5_in_eq:
        label_extra = f" [rank-{top5_in_eq.index(cp_val)+1}]"

    label = f"CP={cp_val}{label_extra}"
    color = COLORS.get(cp_val, cmap(0))
    lw = 2.5 if cp_val in (baseline_cp, winner_cp) else 1.4
    alpha = 0.95 if cp_val in (baseline_cp, winner_cp) else 0.7
    ax.plot(eq_pivot.index, eq_pivot[cp_val], label=label, color=color,
            linewidth=lw, alpha=alpha)

ax.set_yscale("log")
ax.set_xlabel("Bar (equity curve index)")
ax.set_ylabel("Normalised Equity (log scale)")
ax.set_title("C. Equity Curves — Baseline + Top 5 CP values\n(Base5 universe, full history from bar 252)")
ax.legend(fontsize=7, loc="upper left")
ax.grid(True, which="both", alpha=0.3)

# ── Panel D: Return vs CP ──────────────────────────────────────────────────────
ax = axes[1, 1]
ax.plot(cp_agg["cp"], cp_agg["avg_ret"], color="#E91E63", linewidth=2)
ax.scatter(cp_agg["cp"], cp_agg["avg_ret"], color="#E91E63", s=20, alpha=0.7)
ax.axvline(winner_cp, color=star_color, linestyle="--", linewidth=1, alpha=0.4)
ax.axvline(baseline_cp, color=COLORS[baseline_cp], linestyle=":", linewidth=1.5, alpha=0.6)
ax.set_xlabel("CHAND_PERIOD")
ax.set_ylabel("Avg Walk-Forward Return (%)")
ax.set_title("D. Return vs CHAND_PERIOD")
ax.set_ylim(bottom=-5)

plt.tight_layout(rect=[0, 0.03, 1, 0.95])
os.makedirs(os.path.dirname(OUT), exist_ok=True)
plt.savefig(OUT, dpi=150, bbox_inches="tight")
print(f"\nSaved: {OUT}")
