#!/usr/bin/env python3
"""
T88 HOLD_MAX comparison chart — winner HM=1 vs baseline HM=15 vs HM=12.
Reads t88_hm_equity.csv (hold_max, step, equity) and generates
comparison_chart.png with log-scale equity curves + pass rate bar chart.
"""

import csv
import math
from pathlib import Path
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt

OUT_DIR = Path("/home/ubuntu/.openclaw/workspace-krypto/charts")
OUT_PNG = OUT_DIR / "comparison_chart.png"
SRC_CSV = Path("/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/t88_hm_summary.csv")
EQUITY_CSV = Path("/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/t88_hm_equity.csv")

# ── load summary ──────────────────────────────────────────────────────────────
rows = []
with open(SRC_CSV) as f:
    reader = csv.DictReader(f)
    for row in reader:
        rows.append({
            'hm': int(row['hold_max']),
            'pass': int(row['global_pass']),
            'total': int(row['global_total']),
            'rate': float(row['pass_rate_pct']),
            'sharpe': float(row['avg_sharpe']),
            'ret': float(row['avg_return_pct']),
            'dd': float(row['avg_max_dd_pct']),
            'trades': int(row['total_trades']),
        })

# ── equity loader (sample every N bars for speed) ──────────────────────────────
EQUITY_SAMPLE = 10  # every 10 bars

def load_equity(hm: int):
    curve = []
    with open(EQUITY_CSV) as f:
        reader = csv.DictReader(f)
        for row in reader:
            if int(row['hold_max']) == hm:
                step = int(row['step'])
                if step % EQUITY_SAMPLE == 0:
                    curve.append((step, float(row['equity'])))
    return zip(*curve) if curve else ([], [])

# ── determine key lines to plot ───────────────────────────────────────────────
# Winner: HM=1 (pass=69/69=100%, Sharpe 2.13, highest Sharpe at 100% pass)
# Baseline: HM=15 (current config, pass=69/69=100%, Sharpe 1.54)
# Runner-up: HM=12 (prior baseline, pass=69/69=100%, Sharpe 1.58)
# Also include HM=8 (good Sharpe+low trade count) and HM=20 (plateau start)

PLOT_HMS = [1, 12, 15, 8, 20]

hm_data = {}
for hm in PLOT_HMS:
    steps, eqs = load_equity(hm)
    hm_data[hm] = (steps, eqs)
    print(f"HM={hm:3d}: {len(steps)} points, final equity {eqs[-1]:.4f}" if eqs else f"HM={hm}: no data")

# ── colour scheme ──────────────────────────────────────────────────────────────
COLORS = {
    1:  '#27ae60',   # winner — green
    12: '#2980b9',   # prior baseline — blue
    15: '#7f8c8d',   # current config — grey
    8:  '#e67e22',   # runner-up — orange
    20: '#9b59b6',   # plateau reference — purple
}
LABELS = {
    1:  'HM=1  (WINNER, 100% pass, Sharpe 2.13)',
    12: 'HM=12 (prior baseline, Sharpe 1.58)',
    15: 'HM=15 (current config, Sharpe 1.54)',
    8:  'HM=8  (runner-up, Sharpe 1.61)',
    20: 'HM=20 (plateau, Sharpe 1.55)',
}

# ── plot ──────────────────────────────────────────────────────────────────────
fig, axes = plt.subplots(1, 2, figsize=(18, 7))

# --- Left: equity curves (log scale) -----------------------------------------
ax = axes[0]
for hm in PLOT_HMS:
    steps, eqs = hm_data[hm]
    if not steps:
        continue
    color = COLORS.get(hm, '#333333')
    label = LABELS.get(hm, f'HM={hm}')
    linewidth = 2.5 if hm == 1 else 1.4
    linestyle = '-' if hm in (1, 15) else '--'
    ax.plot(steps, eqs, color=color, linewidth=linewidth, linestyle=linestyle, label=label)

ax.set_yscale('log')
ax.set_xlabel('Simulation bar (daily)', fontsize=12)
ax.set_ylabel('Portfolio equity (log scale)', fontsize=12)
ax.set_title('T88 HOLD_MAX Sweep — Base5 Equity Curves (1..=100, 9 universes × 8 WF windows)\nWinner: HM=1 → 100% pass, Sharpe 2.13 | Current HM=15: Sharpe 1.54', fontsize=11)
ax.legend(fontsize=9)
ax.grid(True, alpha=0.3)

# --- Right: pass rate + Sharpe scatter ----------------------------------------
ax2 = axes[1]
hm_vals = [r['hm'] for r in rows]
pass_rates = [r['rate'] for r in rows]
sharpes = [r['sharpe'] for r in rows]

scatter = ax2.scatter(hm_vals, pass_rates, c=sharpes, cmap='YlOrRd', s=40, zorder=3)
cbar = plt.colorbar(scatter, ax=ax2, shrink=0.8)
cbar.set_label('Avg Sharpe', fontsize=10)

# Mark winner and baseline
for r in rows:
    if r['hm'] in (1, 12, 15):
        ax2.annotate(f"HM={r['hm']}", (r['hm'], r['rate']),
                     textcoords='offset points', xytext=(5, 5), fontsize=8,
                     color='darkgreen' if r['hm'] == 1 else 'darkblue')

ax2.set_xlabel('HOLD_MAX', fontsize=12)
ax2.set_ylabel('Pass rate (%)', fontsize=12)
ax2.set_title('HOLD_MAX Robustness — Pass Rate × Sharpe (all 100 values)', fontsize=11)
ax2.set_xlim(0, 102)
ax2.set_ylim(95, 101.5)
ax2.grid(True, alpha=0.3)

plt.tight_layout(rect=[0, 0, 1, 0.93])
plt.savefig(OUT_PNG, dpi=150, bbox_inches='tight', facecolor=fig.get_facecolor())
plt.close()

print(f"\nSaved: {OUT_PNG}")