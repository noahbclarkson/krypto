#!/usr/bin/env python3
"""
Freshness Cooldown Sweep — Comparison Chart
Plots equity curves for baseline (cd=0), winner (cd=0), and runner-up (cd=35, cd=40)
from the FRESHNESS_COOLDOWN hyperopt sweep.

Data: snapshots/freshness_sweep.csv (summary)
      snapshots/freshness_sweep_equity.csv (time-series)
"""

import pandas as pd
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import matplotlib.ticker as mticker
import numpy as np
import sys
import os

os.chdir('/home/ubuntu/.openclaw/workspace-krypto/krypto')

# ── Load summary ──────────────────────────────────────────────────────────────
summary = pd.read_csv('snapshots/freshness_sweep.csv')
print("Summary:")
print(summary.to_string(index=False))

# ── Load equity time-series ──────────────────────────────────────────────────
eq_df = pd.read_csv('snapshots/freshness_sweep_equity.csv')
print(f"\nEquity CSV: {len(eq_df)} rows, cd values: {sorted(eq_df['cd'].unique())}")

# ── Select curves to plot ────────────────────────────────────────────────────
# Winner: cd=0 (Sharpe 8.5649, 6/6 pass)
# Runner-ups: cd=35 (Sharpe 7.3171, 6/6 pass), cd=40 (Sharpe 7.1448, 6/6 pass)
# Baseline (live bot default): cd=10 (Sharpe 5.4760, 3/6 pass) — for comparison
PLOT_CDS = [0, 10, 35, 40]
COLORS = {
    0:  '#00d4ff',   # cyan — winner (no filter)
    10: '#ff4444',   # red — current live default
    35: '#44ff88',   # green — runner-up
    40: '#ffaa00',   # orange — runner-up
}
LABELS = {
    0:  'cd=0  (WINNER, 6/6 pass, Sharpe 8.56)',
    10: 'cd=10 (live bot default, 3/6 pass, Sharpe 5.48)',
    35: 'cd=35 (runner-up, 6/6 pass, Sharpe 7.32)',
    40: 'cd=40 (runner-up, 6/6 pass, Sharpe 7.14)',
}

# ── Build per-cd equity series (chain windows into one timeline) ─────────────
# Each cd has 6 windows worth of bars, concatenated
series = {}
for cd in PLOT_CDS:
    sub = eq_df[eq_df['cd'] == cd].sort_values('bar_idx')
    series[cd] = sub['equity'].values
    print(f"  cd={cd}: {len(series[cd])} bars, final equity={series[cd][-1]:.2f}x")

# ── Figure setup ─────────────────────────────────────────────────────────────
fig, axes = plt.subplots(1, 2, figsize=(16, 7))
fig.suptitle('FRESHNESS_COOLDOWN Hyperopt — Base5 Universe (6 windows)\n'
             'Turtle+Chandelier: EP=21, Chand(20,2.15), ATR(24,2.0), HM=45, CAP=3',
             fontsize=13, fontweight='bold')

# ── Panel 1: Equity curves (log scale) ─────────────────────────────────────
ax1 = axes[0]
for cd in PLOT_CDS:
    ax1.plot(series[cd], label=LABELS[cd], color=COLORS[cd], linewidth=1.5, alpha=0.9)

ax1.set_xlabel('Bar index (daily bars, all windows concatenated)', fontsize=10)
ax1.set_ylabel('Equity (vs start=1.0)', fontsize=10)
ax1.set_title('Equity Curves — Log Scale', fontsize=11, fontweight='bold')
ax1.set_yscale('log')
ax1.yaxis.set_major_formatter(mticker.FuncFormatter(lambda v, _: f'{v:.0f}x' if v >= 1 else f'{v:.2f}'))
ax1.grid(True, alpha=0.3, which='both')
ax1.legend(fontsize=9, loc='upper left')
ax1.set_xlim(0, max(len(s) for s in series.values()))

# ── Panel 2: Parameter sweep summary bar chart ────────────────────────────────
ax2 = axes[1]
all_cds = sorted(summary['cd'].values)
sharpes = [summary[summary['cd'] == cd]['avg_sharpe'].values[0] for cd in all_cds]
pass_rates = [summary[summary['cd'] == cd]['pass_pct'].values[0] for cd in all_cds]
bar_colors = ['#00d4ff' if cd == 0 else '#ff4444' if cd == 10 else '#aaaaaa' for cd in all_cds]

x = np.arange(len(all_cds))
width = 0.6

bars = ax2.bar(x, sharpes, width, color=bar_colors, alpha=0.85, edgecolor='white', linewidth=0.5)
ax2.set_xlabel('FRESHNESS_COOLDOWN (bars)', fontsize=10)
ax2.set_ylabel('Avg Walk-Forward Sharpe', fontsize=10)
ax2.set_title('Sharpe by Cooldown Value', fontsize=11, fontweight='bold')
ax2.set_xticks(x)
ax2.set_xticklabels([str(cd) for cd in all_cds], fontsize=8)
ax2.grid(True, alpha=0.3, axis='y')

# Annotate winner
winner_sharpe = summary[summary['cd'] == 0]['avg_sharpe'].values[0]
ax2.annotate(f'WINNER\ncd=0\nSharpe {winner_sharpe:.2f}',
             xy=(0, winner_sharpe), xytext=(0 + 1.5, winner_sharpe - 0.5),
             fontsize=8, color='#00d4ff', fontweight='bold',
             arrowprops=dict(arrowstyle='->', color='#00d4ff', lw=1.2))

# Annotate cd=10 (live default)
cd10_sharpe = summary[summary['cd'] == 10]['avg_sharpe'].values[0]
ax2.annotate(f'cd=10\nlive default\nSharpe {cd10_sharpe:.2f}',
             xy=(2, cd10_sharpe), xytext=(2 + 1.5, cd10_sharpe + 0.5),
             fontsize=8, color='#ff4444', fontweight='bold',
             arrowprops=dict(arrowstyle='->', color='#ff4444', lw=1.2))

# Legend for colors
from matplotlib.patches import Patch
legend_elements = [
    Patch(facecolor='#00d4ff', label='cd=0  (winner — no filter)'),
    Patch(facecolor='#ff4444', label='cd=10 (live bot default)'),
    Patch(facecolor='#aaaaaa', label='other cd values'),
]
ax2.legend(handles=legend_elements, fontsize=8, loc='upper right')

plt.tight_layout(rect=[0, 0, 1, 0.93])
out_path = 'charts/freshness_sweep_comparison.png'
plt.savefig(out_path, dpi=150, bbox_inches='tight', facecolor='white')
print(f"\nSaved: {out_path}")

# ── Also save a per-window breakdown table ────────────────────────────────────
print("\n=== Summary Table ===")
print(f"{'cd':>4} {'Pass':>6} {'Pass%':>7} {'Sharpe':>8} {'AvgRet%':>9} {'AvgDD%':>8} {'Trades':>7}")
print("-" * 55)
for _, row in summary.iterrows():
    print(f"{int(row['cd']):>4} {int(row['pass']):>6} {row['pass_pct']:>7.1f} {row['avg_sharpe']:>8.4f} {row['avg_ret']:>9.2f} {row['avg_dd']:>8.2f} {int(row['total_trades']):>7}")
