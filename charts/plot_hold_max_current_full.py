#!/usr/bin/env python3
"""Plot HOLD_MAX full-range hyperopt equity comparison.

Reads:
- snapshots/hold_max_current_full_summary.csv
- snapshots/hold_max_current_full_equity.csv

Writes:
- charts/comparison_chart.png
"""

from pathlib import Path
import pandas as pd
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
import matplotlib.ticker as mticker

ROOT = Path("/home/ubuntu/.openclaw/workspace-krypto/krypto")
SUMMARY = ROOT / "snapshots/hold_max_current_full_summary.csv"
EQUITY = ROOT / "snapshots/hold_max_current_full_equity.csv"
OUT = ROOT / "charts/comparison_chart.png"
BASELINE = 12
UNIVERSE = "Base5"

summary = pd.read_csv(SUMMARY)
summary = summary.sort_values(["avg_sharpe", "pass_rate_pct"], ascending=[False, False]).reset_index(drop=True)
winner = int(summary.iloc[0]["hold_max"])
runnerups = [int(x) for x in summary[~summary["hold_max"].isin([winner, BASELINE])].head(2)["hold_max"]]
selected = []
for hm in [BASELINE, winner] + runnerups:
    if hm not in selected:
        selected.append(hm)

print("Selected HOLD_MAX values:", selected)
print(summary[summary["hold_max"].isin(selected)][["hold_max", "avg_sharpe", "pass_rate_pct", "avg_return_pct", "avg_max_dd_pct", "total_trades"]].to_string(index=False))

eq = pd.read_csv(EQUITY)
eq = eq[(eq["universe"] == UNIVERSE) & (eq["hold_max"].isin(selected))].copy()
eq = eq.sort_values(["hold_max", "window", "step"])

curves = {}
for hm in selected:
    sub = eq[eq["hold_max"] == hm]
    xs, ys = [], []
    global_step = 0
    cumulative = 1.0
    for window in sorted(sub["window"].unique()):
        w = sub[sub["window"] == window].sort_values("step")
        if w.empty:
            continue
        first = float(w.iloc[0]["equity"])
        if first == 0:
            first = 1.0
        for val in w["equity"].astype(float):
            xs.append(global_step)
            ys.append(cumulative * (val / first))
            global_step += 1
        last = float(w.iloc[-1]["equity"])
        cumulative *= last / first
    curves[hm] = (xs, ys)

fig, ax = plt.subplots(figsize=(14, 8))
colors = ["#2563eb", "#dc2626", "#16a34a", "#f97316", "#7c3aed"]
styles = ["-", "-", "--", "-.", ":"]

all_y = []
for i, hm in enumerate(selected):
    xs, ys = curves[hm]
    label_bits = []
    if hm == BASELINE:
        label_bits.append("Baseline")
    if hm == winner:
        label_bits.append("Winner")
    if not label_bits:
        label_bits.append("Runner-up")
    row = summary[summary["hold_max"] == hm].iloc[0]
    label = f"{' / '.join(label_bits)} HM={hm} | Sharpe {row['avg_sharpe']:.3f} | pass {row['pass_rate_pct']:.1f}%"
    ax.plot(xs, ys, label=label, color=colors[i % len(colors)], linestyle=styles[i % len(styles)], linewidth=2.0, alpha=0.92)
    all_y.extend(ys)

if all_y and min(all_y) > 0:
    ax.set_yscale("log")
    ymin, ymax = min(all_y), max(all_y)
    ax.set_ylim(ymin * 0.90, ymax * 1.10)
    ax.yaxis.set_major_formatter(mticker.FuncFormatter(lambda v, _: f"{v:.2f}x" if v < 100 else f"{v:.0f}x"))
else:
    ymin, ymax = min(all_y), max(all_y)
    pad = (ymax - ymin) * 0.08 if ymax > ymin else 0.1
    ax.set_ylim(ymin - pad, ymax + pad)

ax.set_title("HOLD_MAX Full-Range Hyperopt — Base5 Walk-Forward Equity\nCurrent production params, HM 1..100 step 1", fontsize=14, fontweight="bold")
ax.set_xlabel("Walk-forward step (concatenated windows)", fontsize=11)
ax.set_ylabel("Compounded portfolio equity", fontsize=11)
ax.grid(True, which="both", linestyle="--", alpha=0.35)
ax.legend(loc="best", fontsize=10, frameon=True)

# Annotation with global top-five summary
lines = ["Global avg Sharpe top 5:"]
for _, row in summary.head(5).iterrows():
    lines.append(f"HM={int(row['hold_max']):>3}: sh {row['avg_sharpe']:.3f}, pass {row['pass_rate_pct']:.1f}%")
ax.text(0.01, 0.02, "\n".join(lines), transform=ax.transAxes, fontsize=9,
        va="bottom", ha="left", bbox=dict(boxstyle="round", facecolor="white", alpha=0.75, edgecolor="#cccccc"))

OUT.parent.mkdir(parents=True, exist_ok=True)
fig.tight_layout()
fig.savefig(OUT, dpi=180, bbox_inches="tight", facecolor="white")
print(f"Saved {OUT}")
