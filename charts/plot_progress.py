#!/usr/bin/env python3
"""Plot progress equity curves from snapshots/progress_equity_curves.csv"""
import csv
import os
import numpy as np
import matplotlib.pyplot as plt

OUTDIR = '/home/ubuntu/.openclaw/workspace-krypto/krypto/charts'
os.makedirs(OUTDIR, exist_ok=True)

# Load baseline CSV
rows = []
with open('snapshots/progress_equity_curves.csv') as f:
    reader = csv.reader(f)
    headers = next(reader)
    for row in reader:
        rows.append([float(x) for x in row])

days = np.array([r[0] for r in rows])
equities = {
    'Turtle+Chandelier': np.array([r[4] for r in rows]),
    'A/D Momentum':       np.array([r[1] for r in rows]),
    'FactorSmallByDV':   np.array([r[2] for r in rows]),
    'DDBudget 3-Sleeve': np.array([r[3] for r in rows]),
}

# Load ATR_RANK=24 variant CSV (turtle column is the rank24 variant — production default)
rows_r24 = []
with open('snapshots/progress_equity_curves_atrrank24.csv') as f:
    reader = csv.reader(f)
    headers = next(reader)
    for row in reader:
        rows_r24.append([float(x) for x in row])
turtle_rank24_eq = np.array([r[4] for r in rows_r24])

# Align arrays (rank24 CSV has same length as main: 2091 rows)
assert len(days) == len(turtle_rank24_eq), \
    f"Array length mismatch: days={len(days)}, rank24={len(turtle_rank24_eq)}"

fig, axes = plt.subplots(2, 1, figsize=(14, 10))

# Panel 1: Log-scale equity — all strategies
ax = axes[0]
colors = {'Turtle+Chandelier': '#2196F3', 'A/D Momentum': '#4CAF50',
          'FactorSmallByDV': '#FF9800', 'DDBudget 3-Sleeve': '#F44336'}
for name, eq in equities.items():
    ax.plot(days, eq, label=name, color=colors[name], linewidth=1.5)
ax.plot(days, turtle_rank24_eq, label='Turtle ATR_RANK=24 (AP=64, T=24)', 
        color='#9C27B0', linewidth=1.5, linestyle='--')
ax.set_yscale('log')
ax.set_ylabel('Equity (log scale)')
ax.set_title(
    'Strategy Equity Curves — Base5 Universe (2091 days, 2018–2026)\n'
    'Turtle+Chandelier: 110.1x (Sharpe 0.98) | Turtle ATR_RANK=24: 76.5x (Sharpe 0.96) | '
    'A/D: 40.3x | DDBudget: 62.2x | FactorSmallByDV: 15.5x'
)
ax.legend(loc='upper left')
ax.grid(True, which='both', ls='--', alpha=0.4)

# Panel 2: Drawdown comparison — baseline vs ATR_RANK=24
ax2 = axes[1]
turtle_eq = equities['Turtle+Chandelier']
turtle_r24 = turtle_rank24_eq
peak_baseline = np.maximum.accumulate(turtle_eq)
dd_baseline = (turtle_eq / peak_baseline - 1) * 100
peak_r24 = np.maximum.accumulate(turtle_r24)
dd_r24 = (turtle_r24 / peak_r24 - 1) * 100
ax2.fill_between(days, dd_baseline, 0, color='#2196F3', alpha=0.3, label='Turtle baseline')
ax2.plot(days, dd_baseline, color='#2196F3', linewidth=1.0)
ax2.fill_between(days, dd_r24, 0, color='#9C27B0', alpha=0.3, label='Turtle ATR_RANK=24')
ax2.plot(days, dd_r24, color='#9C27B0', linewidth=1.0, linestyle='--')
ax2.set_ylabel('Drawdown %')
ax2.set_xlabel('Day')
ax2.set_title(
    'Drawdown Comparison: Turtle baseline vs Turtle ATR_RANK=24 (AP=64, T=24)\n'
    'ATR_RANK=24 filter: only enter when BTC ATR percentile >= 24th pct of 252-bar history'
)
ax2.legend(loc='lower left')
ax2.grid(True, ls='--', alpha=0.4)

plt.tight_layout()
plt.savefig(f'{OUTDIR}/progress_equity_curves_daily.png', dpi=150, bbox_inches='tight')
print(f"Saved {OUTDIR}/progress_equity_curves_daily.png")
plt.close()

# Caption info
print(f"Final equities: {', '.join([f'{k}={v[-1]:.1f}x' for k,v in equities.items()])}")
print(f"Turtle ATR_RANK=24 (live bot path): {turtle_rank24_eq[-1]:.1f}x")