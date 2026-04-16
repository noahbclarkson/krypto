#!/usr/bin/env python3
import csv
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import numpy as np

OUTDIR = '/home/ubuntu/.openclaw/workspace-krypto/krypto/charts'

rows = []
with open('/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/progress_equity_curves.csv') as f:
    reader = csv.reader(f)
    headers = next(reader)
    for row in reader:
        rows.append([float(x) for x in row])

days = [r[0] for r in rows]
equities = {
    'Turtle+Chandelier': [r[7] for r in rows],
    'A/D Momentum':     [r[1] for r in rows],
    'FactorSmallByDV':   [r[3] for r in rows],
    'CTREND':           [r[4] for r in rows],
    'DDBudget 3-Sleeve': [r[5] for r in rows],
}

fig, axes = plt.subplots(2, 1, figsize=(14, 10))
colors = {'Turtle+Chandelier': '#2196F3', 'A/D Momentum': '#4CAF50',
          'FactorSmallByDV': '#FF9800', 'CTREND': '#9C27B0', 'DDBudget 3-Sleeve': '#F44336'}

ax = axes[0]
for name, eq in equities.items():
    ax.plot(days, eq, label=name, color=colors[name], linewidth=1.5)
ax.set_yscale('log')
ax.set_ylabel('Equity (log scale)')
ax.set_title('Strategy Equity Curves — Base5 Universe (2074 days, 2018–2026)')
ax.legend(loc='upper left')
ax.grid(True, which='both', ls='--', alpha=0.4)

ax2 = axes[1]
turtle_eq = np.array(equities['Turtle+Chandelier'])
peak = np.maximum.accumulate(turtle_eq)
dd = (turtle_eq / peak - 1) * 100
ax2.fill_between(days, dd, 0, color='#2196F3', alpha=0.3)
ax2.plot(days, dd, color='#2196F3', linewidth=1.0)
ax2.set_ylabel('Drawdown %')
ax2.set_xlabel('Day')
ax2.set_title('Turtle+Chandelier Drawdown (Base5)')
ax2.grid(True, ls='--', alpha=0.4)

plt.tight_layout()
plt.savefig(f'{OUTDIR}/progress_equity_curves_daily.png', dpi=200, bbox_inches='tight')
print(f'Saved {OUTDIR}/progress_equity_curves_daily.png')
print(f"Final equities: {', '.join([f'{k}={v[-1]:.1f}x' for k,v in equities.items()])}")