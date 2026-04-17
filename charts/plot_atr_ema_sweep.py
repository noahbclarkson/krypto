#!/usr/bin/env python3
"""Plot ATR EMA smoothing sweep results + equity curve comparison chart."""
import pandas as pd
import matplotlib.pyplot as plt
import numpy as np
import sys
import os

os.chdir('/home/ubuntu/.openclaw/workspace-krypto/krypto')

# Load sweep results
df = pd.read_csv('snapshots/turtle_atr_ema_sweep.csv')
df.columns = df.columns.str.strip()

# Load equity curves
eq_df = pd.read_csv('snapshots/turtle_atr_ema_equity.csv')

fig, axes = plt.subplots(1, 2, figsize=(14, 5))

# --- Left: Sharpe by ATR_EMA value ---
ax1 = axes[0]
colors = ['#e63946' if e == 1 else ('#2a9d8f' if e == 3 else '#457b9d') for e in df['atr_ema']]
bars = ax1.bar(df['atr_ema'], df['avg_sharpe'], color=colors, width=0.8, edgecolor='none')
ax1.axhline(df[df['atr_ema'] == 1]['avg_sharpe'].values[0], color='#e63946', linestyle='--', linewidth=1.5, alpha=0.8, label='Baseline (raw ATR, EMA=1)')
ax1.axhline(df[df['atr_ema'] == 3]['avg_sharpe'].values[0], color='#2a9d8f', linestyle='--', linewidth=1.5, alpha=0.8, label='Winner (EMA=3)')
ax1.set_xlabel('ATR EMA Period', fontsize=11)
ax1.set_ylabel('Average Walk-Forward Sharpe', fontsize=11)
ax1.set_title('ATR EMA Smoothing: 30-Value Parameter Sweep\n(9 universes, 54 walk-forward windows)', fontsize=12)
ax1.set_xlim(0, 31)
ax1.grid(axis='y', alpha=0.3)
ax1.legend(fontsize=9)

# Annotate winner
winner_sharpe = df[df['atr_ema'] == 3]['avg_sharpe'].values[0]
baseline_sharpe = df[df['atr_ema'] == 1]['avg_sharpe'].values[0]
ax1.annotate(f'Winner: EMA=3\nSharpe {winner_sharpe:.4f}\n(+{winner_sharpe-baseline_sharpe:.4f} vs baseline)',
    xy=(3, winner_sharpe), xytext=(10, winner_sharpe + 0.3),
    arrowprops=dict(arrowstyle='->', color='#2a9d8f'), fontsize=9,
    color='#2a9d8f')

# --- Right: Equity curve comparison ---
ax2 = axes[1]
param_colors = {1: '#e63946', 3: '#2a9d8f', 12: '#f4a261', 23: '#9b5de5'}
for param in sorted(eq_df['param'].unique()):
    subset = eq_df[eq_df['param'] == param]
    label = f'EMA={param}'
    color = param_colors.get(param, '#457b9d')
    lw = 2.5 if param in [1, 3] else 1.2
    alpha = 1.0 if param in [1, 3] else 0.6
    ls = '-' if param in [1, 3] else ':'
    ax2.plot(subset['bar'], subset['equity'], label=label, color=color, linewidth=lw, alpha=alpha, linestyle=ls)

ax2.set_xlabel('Trading Bar (Base5 W00)', fontsize=11)
ax2.set_ylabel('Portfolio Equity (BTCUSDT, $1 → $)', fontsize=11)
ax2.set_title('BTCUSDT Equity Curves: ATR EMA Comparison\n(Base5 Universe, Walk-Forward Window 0)', fontsize=12)
ax2.set_yscale('log')
ax2.grid(alpha=0.3)
ax2.legend(fontsize=9)
ax2.yaxis.set_major_formatter(plt.FuncFormatter(lambda x, _: f'{x:.2f}'))

plt.tight_layout()
plt.savefig('charts/turtle_atr_ema_sweep.png', dpi=150, bbox_inches='tight', facecolor='white')
plt.close()
print("Saved charts/turtle_atr_ema_sweep.png")

# Also save the sweep results as a summary chart
fig2, ax = plt.subplots(figsize=(10, 5))
ax.plot(df['atr_ema'], df['avg_sharpe'], 'o-', color='#457b9d', linewidth=1.5, markersize=4, label='Avg Sharpe')
ax.fill_between(df['atr_ema'], df['avg_sharpe'], alpha=0.15, color='#457b9d')

# Mark baseline and winner
ax.scatter([1], [df[df['atr_ema']==1]['avg_sharpe'].values[0]], color='#e63946', s=120, zorder=5, label='Baseline (raw ATR)', marker='D')
ax.scatter([3], [df[df['atr_ema']==3]['avg_sharpe'].values[0]], color='#2a9d8f', s=120, zorder=5, label='Winner (EMA=3)', marker='*')

# Pass rate as secondary
ax2_t = ax.twinx()
ax2_t.plot(df['atr_ema'], df['pass_pct'], 's--', color='#aaa', linewidth=1, markersize=3, alpha=0.5, label='Pass Rate %')
ax2_t.set_ylabel('Pass Rate (%)', fontsize=10, color='#aaa')
ax2_t.tick_params(axis='y', labelcolor='#aaa')
ax2_t.set_ylim(70, 100)

ax.set_xlabel('ATR EMA Period', fontsize=11)
ax.set_ylabel('Average Walk-Forward Sharpe', fontsize=11, color='#457b9d')
ax.set_title('Turtle+Chandelier: ATR EMA Smoothing Sensitivity\n(30 values, 9 universes, 54 windows each)', fontsize=12, fontweight='bold')
ax.set_xlim(0, 31)
ax.grid(alpha=0.3)

# Combined legend
lines1, labels1 = ax.get_legend_handles_labels()
lines2, labels2 = ax2_t.get_legend_handles_labels()
ax.legend(lines1 + lines2, labels1 + labels2, loc='upper right', fontsize=9)

plt.tight_layout()
plt.savefig('charts/turtle_atr_ema_sweep_full.png', dpi=150, bbox_inches='tight', facecolor='white')
plt.close()
print("Saved charts/turtle_atr_ema_sweep_full.png")

# Print summary
print("\n=== ATR EMA SWEEP SUMMARY ===")
print(f"Baseline (ATR_EMA=1, raw ATR): Sharpe {baseline_sharpe:.4f}")
print(f"Winner (ATR_EMA=3):              Sharpe {winner_sharpe:.4f}")
print(f"Delta:                           {winner_sharpe-baseline_sharpe:+.4f} ({+(winner_sharpe-baseline_sharpe)/baseline_sharpe*100:.1f}%)")
print(f"Conclusion: ATR EMA smoothing provides NEGLIGIBLE improvement (+0.3%)")
print(f"The effect is essentially noise — raw ATR (EMA=1) is the practical default.")
