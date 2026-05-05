#!/usr/bin/env python3
"""Plot LB sweep equity curves: baseline (LB=42), winner (LB=41), and runner-ups."""

import csv
import sys

def read_timeseries(path):
    rows = {}
    with open(path) as f:
        reader = csv.DictReader(f)
        for row in reader:
            w = int(row['window'])
            for k, v in row.items():
                if k == 'window':
                    continue
                if k not in rows:
                    rows[k] = []
                rows[k].append(float(v))
    return rows

def read_summary(path):
    rows = []
    with open(path) as f:
        reader = csv.DictReader(f)
        for row in reader:
            rows.append({
                'lb': int(row['lb']),
                'pass': int(row['pass']),
                'pass_pct': float(row['pass_pct']),
                'sharpe': float(row['avg_sharpe']),
                'ret': float(row['avg_ret']),
                'dd': float(row['avg_dd']),
                'trades': int(row['total_trades']),
                'pos_universes': int(row['pos_universes']),
                'base5_equity': float(row['base5_equity']),
            })
    return rows

ts = read_timeseries('snapshots/lb_sweep_timeseries.csv')
summary = read_summary('snapshots/lb_sweep_summary.csv')

import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import matplotlib.ticker as mticker
import numpy as np

# Select representative LBs to plot
# Baseline = LB=42 (current default)
# Winner = LB=41 (best Sharpe)
# Runner-ups: LB=30 (left plateau), LB=55 (right plateau), LB=100 (far right — degraded)
plot_keys = ['lb_41', 'lb_42', 'lb_30', 'lb_55', 'lb_100']

# Color scheme
colors = {
    'lb_41': '#00d4ff',  # cyan — WINNER
    'lb_42': '#ff9900',  # orange — baseline
    'lb_30': '#aaaaaa',  # grey — runner-up
    'lb_55': '#aaaaaa',  # grey — runner-up
    'lb_100': '#cccccc', # light grey — degraded
}
labels = {
    'lb_41': 'LB=41 (WINNER — Sharpe 8.11, 59/63 pass)',
    'lb_42': 'LB=42 (baseline — Sharpe 7.58, 59/63 pass)',
    'lb_30': 'LB=30 (Sharpe 6.89)',
    'lb_55': 'LB=55 (Sharpe 7.36)',
    'lb_100': 'LB=100 (Sharpe 6.77, degraded)',
}

fig, (ax1, ax2) = plt.subplots(2, 1, figsize=(14, 9), gridspec_kw={'height_ratios': [2.5, 1]})
fig.suptitle('REGIME_LOOKBACK Extensive Sweep: Equity Curve Comparison\n'
             'Turtle-only + ATR_RANK(AP=17,LB,T=5) + HEDGE(PCT=45,SM=0.40) | Base5 × 7 WF windows',
             fontsize=13, fontweight='bold')

# ─── Top: Equity curves (log scale) ─────────────────────────────────────────
for key in plot_keys:
    if key in ts:
        equity = np.array(ts[key])
        windows = np.arange(len(equity))
        color = colors.get(key, '#333333')
        lw = 2.5 if key in ('lb_41', 'lb_42') else 1.5
        ls = '-' if key in ('lb_41', 'lb_42') else '--'
        ax1.plot(windows, equity, label=labels[key], color=color, linewidth=lw, linestyle=ls)

ax1.set_yscale('log')
ax1.yaxis.set_major_formatter(mticker.FuncFormatter(lambda v, _: f'{v:.1f}x'))
ax1.set_ylabel('Compound Equity (log scale)', fontsize=11)
ax1.set_xlabel('Walk-Forward Window', fontsize=11)
ax1.set_title('Base5 Compound Equity per WF Window', fontsize=11, style='italic')
ax1.legend(loc='upper left', fontsize=9)
ax1.grid(True, alpha=0.3)
ax1.set_xlim(0, len(list(ts.values())[0]) - 1)

# ─── Bottom: Sharpe bar chart ────────────────────────────────────────────────
lb_vals = sorted(set(int(k.split('_')[1]) for k in ts.keys()))
sharpes = {}
passes = {}
for row in summary:
    sharpes[row['lb']] = row['sharpe']
    passes[row['lb']] = row['pass_pct']

# Plot all LB values as thin bars
all_lbs = sorted(sharpes.keys())
all_sharpes = [sharpes.get(lb, 0) for lb in all_lbs]
all_passes = [passes.get(lb, 0) for lb in all_lbs]

bar_colors = ['#00d4ff' if lb == 41 else '#ff9900' if lb == 42 else '#888888' for lb in all_lbs]
ax2.bar(all_lbs, all_sharpes, color=bar_colors, width=1.0, alpha=0.85)
ax2.axhline(y=sharpes.get(42, 0), color='#ff9900', linestyle='--', linewidth=1.5, alpha=0.7, label=f'Baseline LB=42: {sharpes.get(42,0):.2f}')
ax2.axvline(x=41, color='#00d4ff', linestyle=':', linewidth=2, alpha=0.8, label=f'Winner LB=41: {sharpes.get(41,0):.2f}')
ax2.set_xlabel('REGIME_LOOKBACK', fontsize=11)
ax2.set_ylabel('Avg Walk-Forward Sharpe', fontsize=11)
ax2.set_title('Sharpe by REGIME_LOOKBACK (step 1, 5–200)', fontsize=11, style='italic')
ax2.set_xlim(4, 201)
ax2.legend(fontsize=9)
ax2.grid(True, alpha=0.3, axis='y')

plt.tight_layout()
out = 'charts/lb_comparison_chart.png'
plt.savefig(out, dpi=150, bbox_inches='tight')
print(f"Saved {out}")

# Also save a zoomed in bar chart around LB=30-60 (the plateau)
fig2, ax3 = plt.subplots(figsize=(12, 4))
plateau_lbs = list(range(30, 61))
plateau_sharpes = [sharpes.get(lb, 0) for lb in plateau_lbs]
bar_colors2 = ['#00d4ff' if lb == 41 else '#ff9900' if lb == 42 else '#2196F3' for lb in plateau_lbs]
ax3.bar(plateau_lbs, plateau_sharpes, color=bar_colors2, width=0.9)
ax3.axhline(y=sharpes.get(42, 0), color='#ff9900', linestyle='--', linewidth=2, label=f'Baseline LB=42: {sharpes.get(42,0):.3f}')
ax3.scatter([41], [sharpes.get(41, 0)], color='#00d4ff', s=120, zorder=5, label=f'Winner LB=41: {sharpes.get(41,0):.3f}')
ax3.set_xlabel('REGIME_LOOKBACK', fontsize=11)
ax3.set_ylabel('Avg Walk-Forward Sharpe', fontsize=11)
ax3.set_title('Sharpe Detail: LB ∈ [30–60] — Winner (LB=41) vs Baseline (LB=42)', fontsize=11)
ax3.set_xticks(range(30, 61, 2))
ax3.legend(fontsize=10)
ax3.grid(True, alpha=0.3, axis='y')
plt.tight_layout()
out2 = 'charts/lb_comparison_detail.png'
plt.savefig(out2, dpi=150, bbox_inches='tight')
print(f"Saved {out2}")