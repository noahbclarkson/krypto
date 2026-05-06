#!/usr/bin/env python3
"""Render the exact live-bot equity curve and drawdown.

Input:  snapshots/live_bot_exact_equity.csv
Output: charts/live_bot_exact_equity.png
"""

from pathlib import Path
import csv
import math

import matplotlib.pyplot as plt

ROOT = Path(__file__).resolve().parent.parent
IN = ROOT / "snapshots" / "live_bot_exact_equity.csv"
OUT = ROOT / "charts" / "live_bot_exact_equity.png"

rows = []
with IN.open() as f:
    for row in csv.DictReader(f):
        rows.append((row["date"][:10], float(row["equity"])))

if not rows:
    raise SystemExit(f"no rows in {IN}")

dates = [r[0] for r in rows]
equity = [r[1] for r in rows]
peak = []
dd = []
p = equity[0]
for e in equity:
    p = max(p, e)
    peak.append(p)
    dd.append((e / p - 1.0) * 100.0)

max_dd = min(dd)
final_eq = equity[-1]
rets = [math.log(equity[i] / equity[i - 1]) for i in range(1, len(equity)) if equity[i - 1] > 0]
mean = sum(rets) / len(rets)
var = sum((r - mean) ** 2 for r in rets) / max(1, len(rets) - 1)
sharpe = mean / math.sqrt(var) * math.sqrt(365.0) if var > 0 else 0.0

fig, (ax1, ax2) = plt.subplots(2, 1, figsize=(13, 8), sharex=True, gridspec_kw={"height_ratios": [2, 1]})
fig.suptitle("Exact live-bot source-of-truth equity (T67)", fontsize=15, fontweight="bold")

x = list(range(len(dates)))
ax1.plot(x, equity, color="#1f77b4", linewidth=1.8)
ax1.set_yscale("log")
ax1.set_ylabel("Equity multiple (log)")
ax1.grid(True, which="both", alpha=0.25)
ax1.text(0.01, 0.97, f"Final {final_eq:.2f}x | daily Sharpe {sharpe:.2f} | MaxDD {abs(max_dd):.1f}% | {len(rows)} days", transform=ax1.transAxes, va="top", fontsize=11, bbox=dict(facecolor="white", alpha=0.8, edgecolor="none"))

ax2.fill_between(x, dd, 0, color="#d62728", alpha=0.35)
ax2.plot(x, dd, color="#d62728", linewidth=1.0)
ax2.set_ylabel("Drawdown (%)")
ax2.set_xlabel("Date")
ax2.grid(True, alpha=0.25)
ax2.set_ylim(min(dd) * 1.15, 2)

step = max(1, len(x) // 8)
ticks = list(range(0, len(x), step))
if ticks[-1] != len(x) - 1:
    ticks.append(len(x) - 1)
ax2.set_xticks(ticks)
ax2.set_xticklabels([dates[i] for i in ticks], rotation=30, ha="right")

fig.tight_layout(rect=[0, 0.02, 1, 0.96])
OUT.parent.mkdir(parents=True, exist_ok=True)
fig.savefig(OUT, dpi=160)
print(f"Wrote {OUT}")
