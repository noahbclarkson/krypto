#!/usr/bin/env python3
"""Chart: Donchian vs Turtle Entry Walk-Forward comparison"""

import csv
import sys

windows = []
don_sharpe, tur_sharpe = [], []
don_ret, tur_ret = [], []
don_pass, tur_pass = [], []
delta_s = []

with open('snapshots/donchian_walkforward.csv') as f:
    reader = csv.DictReader(f)
    for row in reader:
        w = int(row['window'])
        windows.append(f"W{w:02d}")
        don_sharpe.append(float(row['don_sharpe']))
        tur_sharpe.append(float(row['tur_sharpe']))
        don_ret.append(float(row['don_return']))
        tur_ret.append(float(row['tur_return']))
        don_pass.append(row['don_pass'] == 'true')
        tur_pass.append(row['tur_pass'] == 'true')
        delta_s.append(float(row['don_sharpe']) - float(row['tur_sharpe']))

n = len(windows)

import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import matplotlib.patches as mpatches

fig, axes = plt.subplots(1, 2, figsize=(14, 5))

colors_d = ['#2196F3' if p else '#F44336' for p in don_pass]
colors_t = ['#2196F3' if p else '#F44336' for p in tur_pass]

x = range(n)
w = 0.35

ax = axes[0]
ax.bar([i - w/2 for i in x], don_sharpe, w, label='Donchian', color=colors_d, alpha=0.85)
ax.bar([i + w/2 for i in x], tur_sharpe, w, label='Turtle', color=colors_t, alpha=0.85)
ax.axhline(0, color='black', lw=0.5)
ax.set_xticks(x)
ax.set_xticklabels(windows)
ax.set_ylabel('Sharpe Ratio')
ax.set_title('Donchian vs Turtle — Per-Window Sharpe\n(Blue=PASS, Red=FAIL)')
ax.legend(fontsize=9)
ax.grid(axis='y', alpha=0.3)

ax2 = axes[1]
ax2.bar([i - w/2 for i in x], don_ret, w, label='Donchian', color=colors_d, alpha=0.85)
ax2.bar([i + w/2 for i in x], tur_ret, w, label='Turtle', color=colors_t, alpha=0.85)
ax2.axhline(0, color='black', lw=0.5)
ax2.set_xticks(x)
ax2.set_xticklabels(windows)
ax2.set_ylabel('Return (%)')
ax2.set_title('Donchian vs Turtle — Per-Window Return %')
ax2.legend(fontsize=9)
ax2.grid(axis='y', alpha=0.3)

plt.suptitle('T19: Donchian(high) vs Turtle(close) Entry — Base5 Walk-Forward', fontsize=12, fontweight='bold')
plt.tight_layout()
plt.savefig('charts/donchian_vs_turtle_wf.png', dpi=150, bbox_inches='tight')
print("Saved: charts/donchian_vs_turtle_wf.png")

# Summary stats
print(f"\nDonchian: {sum(don_pass)}/{n} pass | avg Sharpe {sum(don_sharpe)/n:+.3f} | avg Return {sum(don_ret)/n:+.1f}%")
print(f"Turtle:   {sum(tur_pass)}/{n} pass | avg Sharpe {sum(tur_sharpe)/n:+.3f} | avg Return {sum(tur_ret)/n:+.1f}%")
print(f"Delta avg Sharpe: {sum(delta_s)/n:+.3f}")