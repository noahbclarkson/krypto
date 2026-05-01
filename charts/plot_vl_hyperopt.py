#!/usr/bin/env python3
"""
VOL_LOOKBACK Hyperopt Chart
Panel 1: pass rate + Sharpe vs VL (1..=100)
Panel 2: log-scale equity curves for Baseline (VL=8), Winner, and Runner-ups

Usage: python3 charts/plot_vl_hyperopt.py
"""
import pandas as pd
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import numpy as np

KRYPT = "/home/ubuntu/.openclaw/workspace-krypto/krypto"

# ── 1. Load data ──────────────────────────────────────────────────────────────
summary  = pd.read_csv(f"{KRYPT}/snapshots/vl_hyperopt_summary.csv")
equity   = pd.read_csv(f"{KRYPT}/snapshots/vl_hyperopt_equity.csv")
selected = pd.read_csv(f"{KRYPT}/snapshots/vl_hyperopt_selected.csv")

# ── 2. Identify winners and runners-up ────────────────────────────────────────
# Sort by pass_pct desc, then sharpe desc, then pos_universes desc
top = summary.sort_values(
    ["pass_pct", "avg_sharpe", "pos_universes"],
    ascending=[False, False, False]
).reset_index(drop=True)

baseline_vl = 8   # current production default
winner_vl   = int(top.iloc[0]["vl"])

# Pick runner-ups: pick the midpoint of the plateau (VL=34 is first value matching baseline pass rate with better Sharpe)
runner_ups = [34, 100]

print(f"Baseline  : VL={baseline_vl}")
print(f"Winner    : VL={winner_vl}")
print(f"Runner-ups: {runner_ups}")

# ── 3. Aggregate equity ────────────────────────────────────────────────────────
def agg_equity(df, vl):
    """Geometric-mean equity across universes/windows at each bar_idx."""
    sub = df[df["vl"] == vl].copy()
    sub["uw"] = sub["universe"] + "_W" + sub["window"].astype(str)
    # pivot so each uw becomes a column
    piv = sub.pivot_table(index="bar_idx", columns="uw", values="equity", aggfunc="first")
    # geometric mean: mean of log, then exp
    log_mean = np.log(piv.replace(0, np.nan)).mean(axis=1, skipna=True)
    return np.exp(log_mean)

base_equity = agg_equity(selected, baseline_vl)
win_equity  = agg_equity(selected, winner_vl)
ru1_equity  = agg_equity(selected, runner_ups[0])
ru2_equity  = agg_equity(selected, runner_ups[1])

# ── 4. Summary stats for annotations ─────────────────────────────────────────
def stats(df, vl):
    row = df[df["vl"] == vl].iloc[0]
    return (f"VL={vl}\n"
            f"{row['pass_count']:.0f}/{row['total_windows']:.0f} pass ({row['pass_pct']:.1f}%)\n"
            f"Sharpe {row['avg_sharpe']:.2f} | Ret {row['avg_ret']:+.1f}%\n"
            f"DD {row['avg_dd']:.1f}% | {row['trades']:.0f} trades")

# ── 5. Plot ──────────────────────────────────────────────────────────────────
fig, (ax1, ax2) = plt.subplots(2, 1, figsize=(14, 10), gridspec_kw={"height_ratios": [1, 2]})
fig.suptitle(
    f"VOL_LOOKBACK Hyperopt — 1..=100 × 9 Universes × 6 WF Windows\n"
    f"Turtle+Chandelier(7,2.30) / TurtleATR(24,2.0) | EP=21 HM=12 CAP=3",
    fontsize=13, fontweight="bold"
)

# ── Panel 1: pass rate + Sharpe ───────────────────────────────────────────────
ax1b = ax1.twinx()
ax1.plot(summary["vl"], summary["pass_pct"],  color="tab:blue",   linewidth=1.3, label="Pass Rate %")
ax1b.plot(summary["vl"], summary["avg_sharpe"], color="tab:orange", linewidth=1.3, label="Avg Sharpe")

# Reference lines
colors_labels = [
    (baseline_vl, "tab:blue",   "--", f"BASELINE (VL={baseline_vl})"),
    (winner_vl,   "tab:green",  "-.", f"WINNER (VL={winner_vl})"),
    (runner_ups[0],"tab:red",    ":",  f"Runner-up (VL={runner_ups[0]})"),
    (runner_ups[1],"tab:purple", ":",  f"Runner-up (VL={runner_ups[1]})"),
]
for vl, color, ls, label in colors_labels:
    ax1.axvline(vl, color=color, linestyle=ls, alpha=0.8, linewidth=1.4, label=label)

ax1.set_xlabel("VOL_LOOKBACK")
ax1.set_ylabel("Pass Rate %", color="tab:blue")
ax1b.set_ylabel("Avg Sharpe", color="tab:orange")
ax1.set_xlim(0, 101)
ax1.set_ylim(60, 100)
ax1.legend(loc="upper left", fontsize=8)
ax1b.legend(loc="upper right", fontsize=8)
ax1.set_title("Pass Rate + Sharpe vs VOL_LOOKBACK")
ax1.grid(True, alpha=0.3)

# Annotate key points
for vl, color in [(baseline_vl, "tab:blue"), (winner_vl, "tab:green")]:
    row = summary[summary["vl"] == vl].iloc[0]
    offset = -12 if vl == winner_vl else 8
    ax1.annotate(
        f"VL={vl}\n{row['pass_count']:.0f}/{row['total_windows']:.0f} ({row['pass_pct']:.1f}%)\nsh={row['avg_sharpe']:.2f}",
        xy=(vl, row["pass_pct"]),
        xytext=(vl + offset * 0.01, row["pass_pct"] - 5),
        fontsize=7.5, color=color,
        arrowprops=dict(arrowstyle="->", color=color, alpha=0.5),
    )

# ── Panel 2: equity curves (log scale) ──────────────────────────────────────
for vl_eq, color, label in [
    (base_equity,  "tab:blue",   f"Baseline VL={baseline_vl}"),
    (win_equity,   "tab:green",  f"Winner VL={winner_vl}"),
    (ru1_equity,   "tab:red",    f"Runner-up VL={runner_ups[0]}"),
    (ru2_equity,   "tab:purple", f"Runner-up VL={runner_ups[1]}"),
]:
    if vl_eq.notna().any() and vl_eq.dropna().shape[0] > 0:
        # Replace zeros with NaN for log plotting
        vals = vl_eq.replace(0, np.nan).dropna()
        if not vals.empty:
            ax2.plot(vals.index[:len(vals)], vals.values, color=color, linewidth=1.5, label=label)

ax2.set_xlabel("Bar Index")
ax2.set_ylabel("Portfolio Equity (log scale)")
ax2.set_title("Geometric-Mean Equity Curve — Baseline, Winner, and Runner-ups")
ax2.set_yscale("log")
ax2.grid(True, alpha=0.3)
ax2.legend(loc="upper left", fontsize=9)
# Ensure y-axis lower bound is meaningful
ymin = min(
    base_equity.replace(0, np.nan).min() if not base_equity.dropna().empty else 1.0,
    win_equity.replace(0, np.nan).min() if not win_equity.dropna().empty else 1.0,
    ru1_equity.replace(0, np.nan).min() if not ru1_equity.dropna().empty else 1.0,
    ru2_equity.replace(0, np.nan).min() if not ru2_equity.dropna().empty else 1.0,
)
ymax = max(
    base_equity.max() if not base_equity.dropna().empty else 2.0,
    win_equity.max()  if not win_equity.dropna().empty  else 2.0,
    ru1_equity.max()  if not ru1_equity.dropna().empty else 2.0,
    ru2_equity.max()  if not ru2_equity.dropna().empty else 2.0,
)
ax2.set_ylim(ymin * 0.9, ymax * 1.1)

plt.tight_layout()
out = f"{KRYPT}/charts/comparison_chart.png"
plt.savefig(out, dpi=150, bbox_inches="tight")
print(f"Saved: {out}")
