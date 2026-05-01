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
with open('/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/progress_equity_curves.csv') as f:
    reader = csv.reader(f)
    headers = next(reader)
    for row in reader:
        rows.append([float(x) for x in row])

days = [r[0] for r in rows]
equities = {
    'Turtle+Chandelier': [r[4] for r in rows],
    'A/D Momentum':       [r[1] for r in rows],
    'FactorSmallByDV':   [r[2] for r in rows],
    'DDBudget 3-Sleeve': [r[3] for r in rows],
}

# Load ATR_RANK=5 variant CSV (turtle column is the rank5 variant)
rows_r5 = []
with open('/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/progress_equity_curves_atrrank5.csv') as f:
    reader = csv.reader(f)
    headers = next(reader)
    for row in reader:
        rows_r5.append([float(x) for x in row])
turtle_rank5_eq = [r[4] for r in rows_r5]

fig, axes = plt.subplots(2, 1, figsize=(14, 10))

# Panel 1: Log-scale equity — all strategies
ax = axes[0]
colors = {'Turtle+Chandelier': '#2196F3', 'A/D Momentum': '#4CAF50',
          'FactorSmallByDV': '#FF9800', 'DDBudget 3-Sleeve': '#F44336'}
for name, eq in equities.items():
    ax.plot(days, eq, label=name, color=colors[name], linewidth=1.5)
ax.plot(days, turtle_rank5_eq, label='Turtle ATR_RANK=5', color='#9C27B0', linewidth=1.5, linestyle='--')
ax.set_yscale('log')
ax.set_ylabel('Equity (log scale)')
ax.set_title('Strategy Equity Curves — Base5 Universe (2089 days, 2018–2026)\nTurtle+Chandelier: 108.1x (Sharpe 0.98) | Turtle ATR_RANK=5: 41.0x (Sharpe 0.87) | A/D: 40.3x | DDBudget: 63.1x | SmallCap: 16.9x')
ax.legend(loc='upper left')
ax.grid(True, which='both', ls='--', alpha=0.4)

# Panel 2: Drawdown comparison — baseline vs ATR_RANK=5
ax2 = axes[1]
turtle_eq = np.array(equities['Turtle+Chandelier'])
turtle_r5 = np.array(turtle_rank5_eq)
peak_baseline = np.maximum.accumulate(turtle_eq)
dd_baseline = (turtle_eq / peak_baseline - 1) * 100
peak_r5 = np.maximum.accumulate(turtle_r5)
dd_r5 = (turtle_r5 / peak_r5 - 1) * 100
ax2.fill_between(days, dd_baseline, 0, color='#2196F3', alpha=0.3, label='Turtle baseline')
ax2.plot(days, dd_baseline, color='#2196F3', linewidth=1.0)
ax2.fill_between(days, dd_r5, 0, color='#9C27B0', alpha=0.2, label='Turtle ATR_RANK=5')
ax2.plot(days, dd_r5, color='#9C27B0', linewidth=1.0, linestyle='--')
ax2.set_ylabel('Drawdown %')
ax2.set_xlabel('Day')
ax2.set_title('Turtle+Chandelier Drawdown: Baseline vs ATR_RANK=5\nBaseline: 108.1x / Sharpe 0.98 | ATR_RANK=5: 41.0x / Sharpe 0.87 (regime filter removes low-vol chop; higher per-trade Sharpe but fewer trades)')
ax2.legend(loc='lower left')
ax2.grid(True, ls='--', alpha=0.4)

plt.tight_layout()
plt.savefig(f'{OUTDIR}/progress_equity_curves_daily.png', dpi=200, bbox_inches='tight')
print(f'Saved {OUTDIR}/progress_equity_curves_daily.png')
plt.close()

# Caption info
print(f"Final equities: {', '.join([f'{k}={v[-1]:.1f}x' for k,v in equities.items()])}")
print(f"Turtle ATR_RANK=5: {turtle_rank5_eq[-1]:.1f}x")