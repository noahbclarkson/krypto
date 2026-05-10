#!/usr/bin/env python3
"""
Chart generator for TURTLE_ATR_MULT extensive hyperopt.
Reads:
  snapshots/turtle_atr_mult_live_summary.csv — robustness metrics per multiplier
  snapshots/turtle_atr_mult_live_equity.csv — Base5 full-history equity time-series
Output:
  charts/comparison_chart.png
"""

import csv
import sys
import math
from pathlib import Path

import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import matplotlib.ticker as mticker

BASE_DIR = Path("/home/ubuntu/.openclaw/workspace-krypto/krypto")
SUMMARY_CSV = BASE_DIR / "snapshots/turtle_atr_mult_live_summary.csv"
EQUITY_CSV  = BASE_DIR / "snapshots/turtle_atr_mult_live_equity.csv"
OUT_PNG     = Path("/home/ubuntu/.openclaw/workspace-krypto/charts/comparison_chart.png")

# ── load summary ─────────────────────────────────────────────────────────────
rows = []
with open(SUMMARY_CSV) as f:
    for line in f:
        if line.startswith("atr_mult") or not line.strip():
            continue
        parts = line.strip().split(",")
        if len(parts) < 9:
            continue
        mult    = float(parts[0])
        pcount  = int(parts[1])
        total   = int(parts[2])
        pass_rt = float(parts[3])
        sh      = float(parts[4])
        ret     = float(parts[5])
        dd      = float(parts[6])
        rows.append((mult, pcount, total, pass_rt, sh, ret, dd))

# rank: pass_rate desc, sharpe desc, dd asc
rank_order = sorted(rows, key=lambda r: (-r[3], -r[4], r[5]))
baseline_mult = 2.0

# winner + runner-ups
top5 = rank_order[:5]
print("=== TURTLE_ATR_MULT Robustness Rankings ===")
for i, (m, pc, tot, pr, sh, ret, dd) in enumerate(top5):
    tag = " ← WINNER" if i == 0 else (" ← BASELINE" if abs(m - baseline_mult) < 0.001 else "")
    print(f"  M={m:.2f} | {pc}/{tot} pass ({pr:.1f}%) | Sharpe {sh:.3f} | Ret {ret:.2f}% | DD {dd:.2f}%{tag}")

winner_mult   = top5[0][0]
runner1_mult  = top5[1][0] if len(top5) > 1 else None
runner2_mult  = top5[2][0] if len(top5) > 2 else None
baseline_row  = next((r for r in rows if abs(r[0] - baseline_mult) < 0.001), None)
winner_row    = top5[0]
runner1_row   = top5[1] if len(top5) > 1 else None
runner2_row   = top5[2] if len(top5) > 2 else None

# ── load equity curves ───────────────────────────────────────────────────────
equity_by_mult = {}
with open(EQUITY_CSV) as f:
    for line in f:
        if line.startswith("atr_mult") or not line.strip():
            continue
        parts = line.strip().split(",")
        if len(parts) < 3:
            continue
        mult = float(parts[0])
        step = int(parts[1])
        eq   = float(parts[2])
        equity_by_mult.setdefault(mult, []).append((step, eq))

# pad to common length
max_len = max(len(v) for v in equity_by_mult.values())
for mult in equity_by_mult:
    cur = equity_by_mult[mult]
    if len(cur) < max_len:
        equity_by_mult[mult] = cur + [(cur[-1][0], cur[-1][1])] * (max_len - len(cur))

# sort by step
for mult in equity_by_mult:
    equity_by_mult[mult].sort(key=lambda x: x[0])

def get_steps(mult):
    return [s for s, _ in equity_by_mult[mult]]

def get_eqs(mult):
    return [e for _, e in equity_by_mult[mult]]

steps = get_steps(baseline_mult)

# ── plot ─────────────────────────────────────────────────────────────────────
fig, axes = plt.subplots(1, 2, figsize=(16, 6))
fig.patch.set_facecolor('#0d1117')
for ax in axes:
    ax.set_facecolor('#161b22')

AXIS_COLOR = '#8b949e'
LABEL_COLOR = '#c9d1d9'
TITLE_COLOR = '#e6edf3'

# --- Left: Equity curves ---
ax = axes[0]

PLOT_MULTS = [baseline_mult, winner_mult]
if runner1_mult and abs(runner1_mult - winner_mult) > 0.01:
    PLOT_MULTS.append(runner1_mult)
if runner2_mult and abs(runner2_mult - winner_mult) > 0.01 and abs(runner2_mult - baseline_mult) > 0.01:
    PLOT_MULTS.append(runner2_mult)

COLORS = {
    baseline_mult: '#58a6ff',  # blue — baseline
    winner_mult:   '#3fb950',  # green — winner
}
FALLBACK_COLORS = ['#f0883e', '#a371f7', '#db61a2']
for i, m in enumerate(PLOT_MULTS):
    if m not in COLORS:
        COLORS[m] = FALLBACK_COLORS[i % len(FALLBACK_COLORS)]

LINESTYLES = {baseline_mult: '--', winner_mult: '-'}
FALLBACK_LS = ['-.', ':']

for idx, mult in enumerate(PLOT_MULTS):
    if mult not in equity_by_mult:
        print(f"  WARNING: no equity data for M={mult}")
        continue
    eqs = get_eqs(mult)
    ls = LINESTYLES.get(mult, FALLBACK_LS[idx % len(FALLBACK_LS)])
    label = f"M={mult:.2f}"
    if abs(mult - baseline_mult) < 0.001:
        label += " (baseline)"
    if abs(mult - winner_mult) < 0.001:
        label += " [WINNER]"
    ax.plot(steps, eqs, color=COLORS[mult], linewidth=1.5,
            linestyle=ls, label=label, alpha=0.9)

ax.set_xlabel("Bar (daily steps)", color=LABEL_COLOR, fontsize=11)
ax.set_ylabel("Portfolio Equity (×)", color=LABEL_COLOR, fontsize=11)
ax.set_title("Base5 Full-History Equity — TURTLE_ATR_MULT Sweep\nExtensive range 0.50–5.00 step 0.05 (91 values)",
             color=TITLE_COLOR, fontsize=12, pad=10)
ax.tick_params(colors=AXIS_COLOR)
ax.grid(True, alpha=0.15, color=AXIS_COLOR)
ax.legend(loc="upper left", fontsize=9, framealpha=0.3, labelcolor=LABEL_COLOR)
# dynamic y — don't force 0
ax.set_ylim(bottom=max(0.1, min(eqs) * 0.95))
# Use log scale for equity (important for comparing strategies with very different equity)
ax.set_yscale('log')
ax.yaxis.set_major_formatter(mticker.FuncFormatter(lambda v, _: f"{v:.2f}x"))

# spine colors
for spine in ax.spines.values():
    spine.set_edgecolor('#30363d')

# --- Right: pass rate + Sharpe bar chart ---
ax2 = axes[1]
all_mults_sorted = sorted(rows, key=lambda r: r[0])  # ascending M
ms  = [r[0] for r in all_mults_sorted]
prs = [r[3] for r in all_mults_sorted]
shs = [r[4] for r in all_mults_sorted]

bar_color = []
for r in all_mults_sorted:
    if abs(r[0] - winner_mult) < 0.001:
        bar_color.append('#3fb950')
    elif abs(r[0] - baseline_mult) < 0.001:
        bar_color.append('#58a6ff')
    else:
        bar_color.append('#6e7681')

xpos = list(range(len(ms)))
width = 0.6

bars = ax2.bar(xpos, prs, width, color=bar_color, alpha=0.85, label="Pass Rate (%)")
ax2_twin = ax2.twinx()
ax2_twin.plot(xpos, shs, color='#f0883e', linewidth=2, marker='o',
              markersize=4, alpha=0.9, label="Avg Sharpe", zorder=5)

ax2.set_xticks(xpos)
ax2.set_xticklabels([f"{m:.2f}" for m in ms], rotation=45, ha='right', fontsize=7, color=LABEL_COLOR)
ax2.set_xlabel("TURTLE_ATR_MULT", color=LABEL_COLOR, fontsize=11)
ax2.set_ylabel("Pass Rate (%)", color='#3fb950', fontsize=10)
ax2_twin.set_ylabel("Avg Sharpe", color='#f0883e', fontsize=10)
ax2.set_title("Robustness: Pass Rate + Avg Sharpe by Multiplier\n(green=winner, blue=baseline)",
              color=TITLE_COLOR, fontsize=11, pad=10)
ax2.tick_params(colors=AXIS_COLOR, axis='y', labelcolor='#3fb950')
ax2_twin.tick_params(colors=AXIS_COLOR, axis='y', labelcolor='#f0883e')
ax2.grid(True, alpha=0.15, axis='y', color=AXIS_COLOR)
for spine in ax2.spines.values():
    spine.set_edgecolor('#30363d')

# ── caption below ─────────────────────────────────────────────────────────────
caption = (
    f"TURTLE_ATR_MULT Hyperopt — Extensively Swept 0.50–5.00 step 0.05 (91 values), 9 universes × 7 WF windows.\n"
    f"Winner: M={winner_mult:.2f} → {winner_row[1]}/{winner_row[2]} pass ({winner_row[3]:.1f}%), Sharpe {winner_row[4]:.3f}, Ret {winner_row[5]:.1f}%, DD {winner_row[6]:.2f}%.\n"
    f"Baseline: M={baseline_mult:.2f} → {baseline_row[1] if baseline_row else '?'}/{baseline_row[2] if baseline_row else '?'} pass ({baseline_row[3] if baseline_row else 0:.1f}%), Sharpe {baseline_row[4] if baseline_row else 0:.3f}."
)
fig.text(0.5, 0.01, caption, ha='center', fontsize=8.5, color=AXIS_COLOR,
         wrap=True, transform=fig.transFigure)

plt.tight_layout(rect=[0, 0.06, 1, 1])
OUT_PNG.parent.mkdir(parents=True, exist_ok=True)
plt.savefig(OUT_PNG, dpi=150, bbox_inches='tight', facecolor=fig.get_facecolor())
plt.close()
print(f"\nChart saved → {OUT_PNG}")
print(f"  Baseline M={baseline_mult:.2f}: {baseline_row[1] if baseline_row else '?'}/{baseline_row[2] if baseline_row else '?'} pass, Sharpe {baseline_row[4] if baseline_row else 0:.3f}")
print(f"  Winner   M={winner_mult:.2f}: {winner_row[1]}/{winner_row[2]} pass, Sharpe {winner_row[4]:.3f}")