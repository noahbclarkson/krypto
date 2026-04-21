#!/usr/bin/env python3
"""Plot progress equity curves (fixed: turtle equity forward-filled, CTREND removed — in-sample artifact)"""
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

# NOTE: MACD+Regime, Blend, and CTREND removed — all GRAVEYARD.
# CSV has 5 columns: day, ad_equity, small_equity, ddbudget_equity, turtle_equity
strategies = {
    'Turtle+Chandelier\n(🦐 OOS validated)': [r[4] for r in rows],
    'A/D Momentum\n(🟢 OOS validated)':     [r[1] for r in rows],
    'FactorSmallByDV\n(🟡 OOS marginal)':    [r[2] for r in rows],
    'DDBudget 3-Sleeve\n(⚠️ milestone-agg)': [r[3] for r in rows],
}

fig, axes = plt.subplots(2, 1, figsize=(14, 10))
colors = {
    'Turtle+Chandelier\n(🦐 OOS validated)': '#2196F3',
    'A/D Momentum\n(🟢 OOS validated)':     '#4CAF50',
    'FactorSmallByDV\n(🟡 OOS marginal)':    '#FF9800',
    'DDBudget 3-Sleeve\n(⚠️ milestone-agg)': '#F44336',
}

ax = axes[0]
for name, eq in strategies.items():
    short_name = name.split('\n')[0]
    ax.plot(days, eq, label=short_name, color=colors[name], linewidth=1.5)
ax.set_yscale('log')
ax.set_ylabel('Equity (log scale)')
ax.set_title('Strategy Equity Curves — Base5 Universe (2074 days, 2018–2026)\n'
             '⚠️ CTREND removed: in-sample artifact (fixed hold, no OOS validation)')
ax.legend(loc='upper left', fontsize=9)
ax.grid(True, which='both', ls='--', alpha=0.4)

# Compute per-year stats for caption
turtle_eq = np.array([r[4] for r in rows])

# Panel 2: Drawdown
ax2 = axes[1]
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
finals = {k.split('\n')[0]: v[-1] for k, v in strategies.items()}
print(f"Final equities: {', '.join([f'{k}={v:.1f}x' for k,v in finals.items()])}")
print(f"Turtle equity Sharpe (daily): compute from daily returns...")

# Compute honest Sharpe for turtle
daily_rets = []
for i in range(1, len(rows)):
    prev = rows[i-1][4]
    curr = rows[i][4]
    if prev > 0:
        daily_rets.append(curr / prev - 1.0)
mean_r = np.mean(daily_rets)
std_r = np.std(daily_rets)
sharpe = mean_r / std_r * np.sqrt(252) if std_r > 1e-10 else 0.0
print(f"Turtle daily equity Sharpe: {sharpe:.2f}")