#!/usr/bin/env python3
"""Plot progress equity curves from snapshots/progress_equity_curves.csv"""
import pandas as pd
import matplotlib.pyplot as plt
import numpy as np
import os

OUTDIR = '/home/ubuntu/.openclaw/workspace-krypto/krypto/charts'
os.makedirs(OUTDIR, exist_ok=True)

df = pd.read_csv('/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/progress_equity_curves.csv')
# Fix: first row header parsing issue — day col is actually first col name
df.columns = ['day', 'ad_equity', 'macd_equity', 'small_equity', 'ctrend_equity', 'ddbudget_equity', 'blend_equity', 'turtle_equity']
# Recompute from raw CSV directly
import csv
rows = []
with open('/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/progress_equity_curves.csv') as f:
    reader = csv.reader(f)
    headers = next(reader)
    for row in reader:
        rows.append([float(x) for x in row])

days = [r[0] for r in rows]
equities = {
    'Turtle+Chandelier': [r[7] for r in rows],  # index 7
    'A/D Momentum':     [r[1] for r in rows],  # index 1
    'FactorSmallByDV':   [r[3] for r in rows],  # index 3
    'CTREND':           [r[4] for r in rows],  # index 4
    'DDBudget 3-Sleeve': [r[5] for r in rows],  # index 5
}
# macd (r[2]) and blend (r[6]) are GRAVEYARD — excluded

# Skip macd_equity and blend_equity (graveyard)

fig, axes = plt.subplots(2, 1, figsize=(14, 10))

# Panel 1: Log-scale equity
ax = axes[0]
colors = {'Turtle+Chandelier': '#2196F3', 'A/D Momentum': '#4CAF50', 
          'FactorSmallByDV': '#FF9800', 'CTREND': '#9C27B0', 'DDBudget 3-Sleeve': '#F44336'}
for name, eq in equities.items():
    ax.plot(days, eq, label=name, color=colors[name], linewidth=1.5)
ax.set_yscale('log')
ax.set_ylabel('Equity (log scale)')
ax.set_title('Strategy Equity Curves — Base5 Universe (2074 days, 2018–2026)')
ax.legend(loc='upper left')
ax.grid(True, which='both', ls='--', alpha=0.4)

# Panel 2: Drawdown (from peak) — only for Turtle+Chandelier (production candidate)
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
plt.close()

# Caption info
print(f"Final equities: {', '.join([f'{k}={v[-1]:.1f}x' for k,v in equities.items()])}")