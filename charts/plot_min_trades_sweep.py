#!/usr/bin/env python3
"""
MIN_TRADES Sweep Analysis Chart
Generates: snapshots/min_trades_comparison.png

Key finding: MIN_TRADES is completely insensitive from 1-10.
All values produce IDENTICAL equity curves and Sharpe ratios.
Only MT=20 shows degradation (pass rate 13% vs 69%).
"""
import pandas as pd
import matplotlib.pyplot as plt
import matplotlib.ticker as mticker
import numpy as np

# Load data
eq_df   = pd.read_csv('snapshots/min_trades_equity.csv')
sweep_df = pd.read_csv('snapshots/min_trades_sweep.csv')

MT_PLATEAU = [1, 3, 10]  # show these distinct MT lines
MT_FAIL    = 20           # the one that degrades

UNIVERSE_COLORS = {
    'Base5':         '#2196F3',  # blue
    'NoDOGE':        '#4CAF50',  # green
    'Legacy4':       '#FF9800',  # orange
    'Legacy5BNB':    '#9C27B0',  # purple
    'OldGuardNoBNB': '#F44336',  # red
    'LargeCaps5':    '#00BCD4',  # cyan
    'Legacy3':       '#795548',  # brown
    'LowVolume5':    '#607D8B',  # blue-grey
    'OldGuard4':     '#E91E63',  # pink
}

fig = plt.figure(figsize=(16, 12))
fig.suptitle('MIN_TRADES Hyperopt — Extensive Sweep (2026-04-26)\n'
             'Strategy: Turtle+Chandelier | Walk-Forward: 9 Universes × 6 Windows',
             fontsize=14, fontweight='bold', y=0.98)

# ── Layout ────────────────────────────────────────────────────────────────────
# Row 1: Equity curves for MT=1, MT=3, MT=10 (all identical → overlapping)
# Row 2 left:  Pass rate bar chart by MT value
# Row 2 right: Sharpe bar chart by MT value

ax1 = fig.add_subplot(2, 2, 1)   # equity: MT=1
ax2 = fig.add_subplot(2, 2, 2)   # equity: MT=3
ax3 = fig.add_subplot(2, 2, 3)   # pass rate bars
ax4 = fig.add_subplot(2, 2, 4)   # sharpe bars

# ── Equity curves (per universe, log scale) ───────────────────────────────────
for ax, mt_val in [(ax1, 1), (ax2, 3)]:
    sub = eq_df[eq_df['min_trades'] == mt_val]
    for univ, color in UNIVERSE_COLORS.items():
        u_df = sub[sub['universe'] == univ].sort_values('window_idx')
        if u_df.empty:
            continue
        ax.plot(u_df['window_idx'], u_df['cumulative_equity'],
                color=color, linewidth=1.5, alpha=0.85, label=univ,
                marker='o', markersize=3)
    ax.set_title(f'Cumulative Equity — MT={mt_val} (Baseline)', fontsize=11, fontweight='bold')
    ax.set_xlabel('Walk-Forward Window')
    ax.set_ylabel('Cumulative Equity (log scale)')
    ax.set_yscale('log')
    ax.yaxis.set_major_formatter(mticker.FuncFormatter(lambda v, _: f'{v:.0f}x'))
    ax.grid(True, alpha=0.3, linestyle='--')
    ax.legend(loc='upper left', fontsize=7, ncol=2)
    ax.set_xlim(-0.5, 6.5)

# ── Pass rate bars ─────────────────────────────────────────────────────────────
mt_vals     = sweep_df['min_trades'].tolist()
pass_rates  = sweep_df['pass_rate_pct'].tolist()
sharpes     = sweep_df['avg_sharpe'].tolist()
colors_bar  = ['#4CAF50' if r >= 60 else '#FF9800' if r >= 40 else '#F44336'
               for r in pass_rates]

bars = ax3.bar([str(v) for v in mt_vals], pass_rates, color=colors_bar,
               edgecolor='white', linewidth=0.5)
# Highlight MT=3
mt_labels = [str(v) for v in mt_vals]
if '3' in mt_labels:
    idx = mt_labels.index('3')
    bars[idx].set_edgecolor('#1a237e')
    bars[idx].set_linewidth(2.0)
    bars[idx].set_hatch('///')

ax3.axhline(69, color='#4CAF50', linewidth=1.5, linestyle='--', alpha=0.7,
            label='Plateau (MT 1-10): 69%')
ax3.set_title('Walk-Forward Pass Rate by MIN_TRADES', fontsize=11, fontweight='bold')
ax3.set_xlabel('MIN_TRADES value')
ax3.set_ylabel('Pass Rate (%)')
ax3.set_ylim(0, 100)
ax3.grid(True, axis='y', alpha=0.3, linestyle='--')
for bar, pr in zip(bars, pass_rates):
    ax3.text(bar.get_x() + bar.get_width()/2, bar.get_height() + 1.5,
             f'{pr:.0f}%', ha='center', va='bottom', fontsize=7.5)
ax3.legend(fontsize=9)

# ── Sharpe bars ────────────────────────────────────────────────────────────────
sharpe_colors = ['#2196F3' if s >= 2.8 else '#FF9800' if s >= 2.0 else '#F44336'
                  for s in sharpes]
bars2 = ax4.bar([str(v) for v in mt_vals], sharpes, color=sharpe_colors,
                edgecolor='white', linewidth=0.5)
if '3' in mt_labels:
    idx = mt_labels.index('3')
    bars2[idx].set_edgecolor('#1a237e')
    bars2[idx].set_linewidth(2.0)
    bars2[idx].set_hatch('///')

ax4.axhline(2.8317, color='#2196F3', linewidth=1.5, linestyle='--', alpha=0.7,
            label='Plateau Sharpe: 2.8317 (MT 1-10)')
ax4.set_title('Walk-Forward Avg Sharpe by MIN_TRADES', fontsize=11, fontweight='bold')
ax4.set_xlabel('MIN_TRADES value')
ax4.set_ylabel('Annualised Sharpe Ratio')
ax4.set_ylim(0, 3.2)
ax4.grid(True, axis='y', alpha=0.3, linestyle='--')
for bar, sh in zip(bars2, sharpes):
    ax4.text(bar.get_x() + bar.get_width()/2, bar.get_height() + 0.05,
             f'{sh:.2f}', ha='center', va='bottom', fontsize=7.5)
ax4.legend(fontsize=9)

plt.tight_layout(rect=[0, 0, 1, 0.96])
plt.savefig('snapshots/min_trades_comparison.png', dpi=150, bbox_inches='tight',
            facecolor='white')
print("Saved: snapshots/min_trades_comparison.png")
plt.close()
