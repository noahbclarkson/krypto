#!/usr/bin/env python3
"""
T81: HOLD_MAX comparison chart — hyperparameter optimization report.

Data sources:
  snapshots/t81_hold_max_summary.csv  — aggregated metrics per HM value
  snapshots/t81_hold_max_equity.csv   — Base5 equity time-series per HM value

Output: /home/ubuntu/.openclaw/workspace-krypto/charts/comparison_chart.png
"""

import pandas as pd
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import os

OUT = "/home/ubuntu/.openclaw/workspace-krypto/charts/comparison_chart.png"
WORKSPACE = "/home/ubuntu/.openclaw/workspace-krypto/krypto"

os.makedirs(os.path.dirname(OUT), exist_ok=True)

# ── 1. Load data ──────────────────────────────────────────────────────────────
summary_df = pd.read_csv(f"{WORKSPACE}/snapshots/t81_hold_max_summary.csv")
eq_df = pd.read_csv(f"{WORKSPACE}/snapshots/t81_hold_max_equity.csv")

BASELINE_HM   = 12
WINNER_HM     = 30
PLATEAU_HM_1  = 40
PLATEAU_HM_2  = 75

def get_series(df, hm):
    sub = df[df["hold_max"] == hm].sort_values("step")
    return sub.set_index("step")["equity"]

eq_baseline  = get_series(eq_df, BASELINE_HM)
eq_winner   = get_series(eq_df, WINNER_HM)
eq_plateau1 = get_series(eq_df, PLATEAU_HM_1)
eq_plateau2 = get_series(eq_df, PLATEAU_HM_2)

# ── 2. Plot ─────────────────────────────────────────────────────────────────────
plt.style.use("seaborn-v0_8-whitegrid")
fig, axes = plt.subplots(2, 2, figsize=(16, 12))
fig.suptitle(
    "HOLD_MAX Hyperparameter Optimization — Turtle+Chandelier Live Path\n"
    "Sweep: 5–100 step 5 (20 values) × 9 universes × 7 walk-forward windows",
    fontsize=13, fontweight="bold", y=0.98
)

# ── Panel A: Sharpe vs HOLD_MAX ──────────────────────────────────────────────
ax = axes[0, 0]
sdf = summary_df.sort_values("hold_max")
ax.plot(sdf["hold_max"], sdf["avg_sharpe"], color="#1976D2", linewidth=2, zorder=5)
ax.scatter(sdf["hold_max"], sdf["avg_sharpe"], color="#1976D2", s=30, zorder=6, alpha=0.7)

# Mark baseline
bl = sdf[sdf["hold_max"] == BASELINE_HM]
if not bl.empty:
    ax.scatter(bl["hold_max"], bl["avg_sharpe"],
              color="#2196F3", s=100, zorder=8, marker="D",
              label=f"HM={BASELINE_HM} [baseline]")

# Mark winner
win = sdf[sdf["hold_max"] == WINNER_HM]
if not win.empty:
    ax.scatter(win["hold_max"], win["avg_sharpe"],
               color="#4CAF50", s=140, zorder=9, marker="*",
               label=f"HM={WINNER_HM} [winner]")
    ax.annotate(
        f"HM={WINNER_HM}\nsh={win['avg_sharpe'].values[0]:.3f}",
        xy=(WINNER_HM, win['avg_sharpe'].values[0]),
        xytext=(WINNER_HM + 14, win['avg_sharpe'].values[0] + 0.04),
        fontsize=8, color="#4CAF50",
        arrowprops=dict(arrowstyle="->", color="#4CAF50")
    )

ax.axvline(WINNER_HM, color="#4CAF50", linestyle="--", linewidth=1, alpha=0.5)
ax.set_xlabel("HOLD_MAX (bars)")
ax.set_ylabel("Avg Walk-Forward Sharpe (9 universes × 7 windows)")
ax.set_title("A. Sharpe vs HOLD_MAX\n(HM ∈ [5..100] step 5, 20 values)")
ax.legend(fontsize=9)

# ── Panel B: Pass Rate vs HOLD_MAX ───────────────────────────────────────────
ax = axes[0, 1]
ax.plot(sdf["hold_max"], sdf["pass_rate_pct"], color="#388E3C", linewidth=2)
ax.scatter(sdf["hold_max"], sdf["pass_rate_pct"], color="#388E3C", s=30, alpha=0.7)

bl2 = sdf[sdf["hold_max"] == BASELINE_HM]
if not bl2.empty:
    ax.scatter(bl2["hold_max"], bl2["pass_rate_pct"],
               color="#2196F3", s=100, zorder=8, marker="D")
win2 = sdf[sdf["hold_max"] == WINNER_HM]
if not win2.empty:
    ax.scatter(win2["hold_max"], win2["pass_rate_pct"],
               color="#4CAF50", s=140, zorder=9, marker="*")

ax.axvline(WINNER_HM, color="#4CAF50", linestyle="--", linewidth=1, alpha=0.5)
ax.axhline(100, color="gray", linestyle=":", linewidth=1, alpha=0.4)
ax.set_xlabel("HOLD_MAX (bars)")
ax.set_ylabel("Pass Rate (%)")
ax.set_title("B. Pass Rate vs HOLD_MAX\n(9 universes × 7 walk-forward windows)")

# ── Panel C: Equity curves full range ─────────────────────────────────────────
ax = axes[1, 0]
COLORS = {
    f"HM={BASELINE_HM} [baseline]": "#2196F3",
    f"HM={WINNER_HM} [winner]":     "#4CAF50",
    f"HM={PLATEAU_HM_1} [plateau]": "#FF9800",
    f"HM={PLATEAU_HM_2} [plateau]": "#9C27B0",
}
for label, col in [
    (f"HM={BASELINE_HM} [baseline]", eq_baseline),
    (f"HM={WINNER_HM} [winner]",     eq_winner),
    (f"HM={PLATEAU_HM_1} [plateau]", eq_plateau1),
    (f"HM={PLATEAU_HM_2} [plateau]", eq_plateau2),
]:
    ax.plot(range(len(col)), col.values, label=label,
            color=COLORS[label], linewidth=1.8, alpha=0.9)

ax.set_yscale("log")
ax.set_xlabel("Bar (daily, Base5 full history)")
ax.set_ylabel("Normalised Equity (log scale)")
ax.set_title("C. Equity Curves — Baseline, Winner & Plateau\n(Base5 full history)")
ax.legend(fontsize=9, loc="upper left")
ax.grid(True, which="both", alpha=0.3)

# ── Panel D: Equity curves zoom (last 600 bars) ────────────────────────────────
ax = axes[1, 1]
OFFSET = 600
for label, col in [
    (f"HM={BASELINE_HM} [baseline]", eq_baseline),
    (f"HM={WINNER_HM} [winner]",     eq_winner),
    (f"HM={PLATEAU_HM_1} [plateau]", eq_plateau1),
    (f"HM={PLATEAU_HM_2} [plateau]", eq_plateau2),
]:
    vals = col.values
    start = max(0, len(vals) - OFFSET)
    x = range(start, len(vals))
    y = vals[start:]
    ax.plot(x, y, label=label, color=COLORS[label], linewidth=1.8, alpha=0.9)

ax.set_yscale("log")
ax.set_xlabel("Bar (daily, Base5 — last 600 bars)")
ax.set_ylabel("Normalised Equity (log scale)")
ax.set_title("D. Equity Curves Zoom — Last 600 Bars\n(highlight divergence)")
ax.legend(fontsize=9, loc="upper left")
ax.grid(True, which="both", alpha=0.3)

plt.tight_layout(rect=[0, 0.03, 1, 0.95])
plt.savefig(OUT, dpi=150, bbox_inches="tight")
print(f"Saved {OUT}")

# ── Print summary for report ───────────────────────────────────────────────────
print("\n=== T81 HOLD_MAX Hyperopt Summary ===")
for hm in sorted(sdf["hold_max"].unique()):
    row = sdf[sdf["hold_max"] == hm]
    if not row.empty:
        marker = ""
        if hm == BASELINE_HM:
            marker = " [BASELINE]"
        elif hm == WINNER_HM:
            marker = " [WINNER]"
        print(f"HM={hm:3d}{marker}: pass {row['pass_count'].values[0]:>2}/60 "
              f"({row['pass_rate_pct'].values[0]:5.1f}%) | "
              f"Sharpe {row['avg_sharpe'].values[0]:6.3f} | "
              f"Ret {row['avg_return_pct'].values[0]:7.2f}% | "
              f"DD {row['avg_max_dd_pct'].values[0]:5.2f}%")
