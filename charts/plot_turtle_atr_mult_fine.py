#!/usr/bin/env python3
"""Plot TURTLE_ATR_MULT fine-sweep equity curves.

Reads:
  snapshots/turtle_atr_mult_dual_fine_summary.csv
  snapshots/turtle_atr_mult_dual_fine_equity.csv
Writes:
  charts/comparison_chart.png
"""
from pathlib import Path
import pandas as pd
import matplotlib.pyplot as plt
import matplotlib.ticker as mticker

ROOT = Path(__file__).resolve().parents[1]
SUMMARY = ROOT / "snapshots" / "turtle_atr_mult_dual_fine_summary.csv"
EQUITY = ROOT / "snapshots" / "turtle_atr_mult_dual_fine_equity.csv"
OUT = ROOT / "charts" / "comparison_chart.png"

summary = pd.read_csv(SUMMARY)
equity = pd.read_csv(EQUITY)

# Robustness ranking: pass rate first, then positive universes, then Sharpe, then lower worst DD.
ranked = summary.sort_values(
    ["pass_count", "positive_universes", "avg_sharpe", "worst_dd_pct"],
    ascending=[False, False, False, True],
).reset_index(drop=True)

baseline = 2.0
winner = float(ranked.iloc[0]["atr_mult"])
# All values tie in this sweep; select representative runner-ups across the tested range so the
# plot proves invariance instead of hiding it behind top-N duplicates near the same value.
selected = []
for v in [baseline, winner, 0.5, 1.5, 3.0, 5.0]:
    if v in set(summary["atr_mult"].round(1)) and v not in selected:
        selected.append(v)
selected = selected[:5]

fig, ax = plt.subplots(figsize=(14, 8), dpi=160)
styles = ["-", "--", "-.", ":", (0, (3, 1, 1, 1))]
colors = ["#1f77b4", "#ff7f0e", "#2ca02c", "#d62728", "#9467bd"]

for i, v in enumerate(selected):
    rows = equity[equity["atr_mult"].round(1) == round(v, 1)].copy()
    m = summary[summary["atr_mult"].round(1) == round(v, 1)].iloc[0]
    role = []
    if abs(v - baseline) < 1e-9:
        role.append("baseline")
    if abs(v - winner) < 1e-9:
        role.append("winner/tie")
    label = (
        f"M={v:.1f} {'/'.join(role)} | "
        f"pass {int(m.pass_count)}/{int(m.total_count)}, "
        f"Sh {m.avg_sharpe:.2f}, Ret {m.avg_ret_pct:.1f}%"
    )
    ax.plot(
        rows["bar"], rows["equity"],
        label=label,
        color=colors[i % len(colors)],
        linestyle=styles[i % len(styles)],
        linewidth=2.2 if abs(v - baseline) < 1e-9 else 1.8,
        alpha=0.95 if abs(v - baseline) < 1e-9 else 0.78,
    )

all_sel = equity[equity["atr_mult"].round(1).isin([round(v,1) for v in selected])]
ymin = all_sel["equity"].min()
ymax = all_sel["equity"].max()
# Dynamic scale: do not anchor at zero. Log scale keeps early and late differences visible.
ax.set_yscale("log")
ax.set_ylim(max(ymin * 0.85, 1e-6), ymax * 1.15)
ax.yaxis.set_major_formatter(mticker.FuncFormatter(lambda y, _: f"{y:,.0f}x" if y >= 1 else f"{y:.2f}x"))

ax.set_title("TURTLE_ATR_MULT Fine Sweep — Base5 Compounded WF Equity Curves\nM=0.5..5.0 step=0.1 × 9 universes × 54 windows; all values tie under current exit logic", fontsize=13)
ax.set_xlabel("Walk-forward equity step (Base5 compounded test windows)")
ax.set_ylabel("Portfolio equity multiple (log scale)")
ax.grid(True, which="both", alpha=0.25)
ax.legend(loc="best", fontsize=8.5, frameon=True)
fig.tight_layout()
OUT.parent.mkdir(parents=True, exist_ok=True)
fig.savefig(OUT, bbox_inches="tight")
print(f"wrote {OUT}")
print("winner", winner)
print(ranked.head(8).to_string(index=False))
