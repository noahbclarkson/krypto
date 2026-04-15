#!/usr/bin/env python3
"""Plot A/D Static Sleeve walk-forward results: Turtle-only vs 80/20 Turtle/A-D"""

import csv
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import matplotlib.patches as mpatches
import numpy as np

data = []
with open('/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/ad_static_sleeve_results.csv') as f:
    reader = csv.DictReader(f)
    for row in reader:
        data.append({
            'universe': row['universe'],
            'window': int(row['win']),
            'turtle': float(row['turtle_sharpe']),
            'ad': float(row['ad_sharpe']),
            'sleeve': float(row['sleeve_sharpe']),
            'imp_pct': float(row['improvement_pct']),
            'winner': row['winner'],
        })

universes = [d['universe'] for d in data]
unique_unis = list(dict.fromkeys(universes))

fig, axes = plt.subplots(2, 1, figsize=(14, 10), gridspec_kw={'height_ratios': [2, 1]})
fig.suptitle('A/D Static Sleeve (80/20) vs Turtle — Walk-Forward Results', fontsize=14, fontweight='bold')

ax1 = axes[0]
n_windows = 21
x = np.arange(n_windows)
width = 0.35

for i, uni in enumerate(unique_unis):
    uni_data = [d for d in data if d['universe'] == uni]
    offsets = (i - len(unique_unis)/2 + 0.5) * width
    t_vals = [d['turtle'] for d in uni_data]
    s_vals = [d['sleeve'] for d in uni_data]
    colors_t = ['#2196F3' if d['winner'] == 'TURTLE' else '#90CAF9' for d in uni_data]
    colors_s = ['#4CAF50' if d['winner'] == 'SLEEVE' else '#A5D6A7' for d in uni_data]
    ax1.bar(x + offsets - width/4, t_vals, width/2, color=colors_t, alpha=0.9, edgecolor='white', linewidth=0.3)
    ax1.bar(x + offsets + width/4, s_vals, width/2, color=colors_s, alpha=0.9, edgecolor='white', linewidth=0.3)

ax1.axhline(0, color='black', linewidth=0.5)
ax1.set_ylabel('Sharpe Ratio')
ax1.set_title('Per-Window Sharpe: Turtle (blue) vs Sleeve (green) — 9 Universes × 21 Windows')
ax1.set_xticks(x)
ax1.set_xticklabels([f'W{i}' for i in range(n_windows)], fontsize=7)
ax1.legend([mpatches.Patch(color='#2196F3'), mpatches.Patch(color='#4CAF50')], ['Turtle-only', '80/20 Sleeve'], loc='upper right')
ax1.grid(axis='y', alpha=0.3)

# Summary bar chart: win rate per universe
ax2 = axes[1]
win_rates = []
sleeve_avg = []
turtle_avg = []
for uni in unique_unis:
    uni_data = [d for d in data if d['universe'] == uni]
    n_win = sum(1 for d in uni_data if d['winner'] == 'SLEEVE')
    n_total = len(uni_data)
    win_rates.append(n_win / n_total * 100)
    sleeve_avg.append(np.mean([d['sleeve'] for d in uni_data]))
    turtle_avg.append(np.mean([d['turtle'] for d in uni_data]))

colors_wr = ['#4CAF50' if wr >= 50 else '#F44336' for wr in win_rates]
bars = ax2.bar(unique_unis, win_rates, color=colors_wr, alpha=0.85, edgecolor='white')
ax2.axhline(50, color='orange', linewidth=1.5, linestyle='--', label='50% threshold')
ax2.set_ylabel('Sleeve Win Rate (%)')
ax2.set_title('Sleeve Beats Turtle: Per-Universe Win Rate (47% overall = rejected)')
ax2.set_ylim(0, 100)
ax2.tick_params(axis='x', rotation=30)
for bar, wr in zip(bars, win_rates):
    ax2.text(bar.get_x() + bar.get_width()/2, bar.get_height() + 1, f'{wr:.0f}%', ha='center', fontsize=8)
ax2.legend()
ax2.grid(axis='y', alpha=0.3)

plt.tight_layout()
out = '/home/ubuntu/.openclaw/workspace-krypto/krypto/charts/ad_static_sleeve_comparison.png'
plt.savefig(out, dpi=120, bbox_inches='tight')
print(f"Saved: {out}")
plt.close()
