#!/usr/bin/env python3
"""Equity curve chart for Turtle+Chandelier production params."""
import sys
import subprocess
import json

try:
    import polars as pl
    HAS_POLARS = True
except ImportError:
    HAS_POLARS = False
    import csv

def load_csv(path):
    if HAS_POLARS:
        df = pl.read_csv(path)
        return df.get_column('equity').to_list(), df.get_column('btc_buyhold').to_list()
    else:
        equity, btc = [], []
        with open(path) as f:
            reader = csv.DictReader(f)
            for row in reader:
                equity.append(float(row['equity']))
                btc.append(float(row['btc_buyhold']))
        return equity, btc

# Load data
eq, btc = load_csv('snapshots/turtle_chandelier_equity.csv')
bars = list(range(len(eq)))

# Log-scale equity
eq_log = [max(e, 1) for e in eq]
btc_log = [max(b, 1) for b in btc]

# Drawdown series
peak_eq = max(eq)
dd = [(peak_eq - e) / peak_eq * 100 for e in eq]

import matplotlib.pyplot as plt
import matplotlib.ticker as mticker

fig, axes = plt.subplots(2, 1, figsize=(14, 10), sharex=True)
fig.suptitle('Turtle+Chandelier Daily Equity (Production Params)\n'
            'EP=24, Chand(7,2.30), ATR(24,2.0), HM=12, CAP=3 — Base5 Universe',
            fontsize=13, fontweight='bold')

ax1, ax2 = axes

# Panel 1: log equity
ax1.semilogy(bars, eq_log, color='#1E88E5', linewidth=1.4, label='Turtle+Chandelier', zorder=3)
ax1.semilogy(bars, btc_log, color='#FFC107', linewidth=1.0, linestyle='--', label='BTC buy-hold', zorder=2)
ax1.set_ylabel('Portfolio Value ($)', fontsize=11)
ax1.set_title('Log Equity', fontsize=11)
ax1.legend(loc='upper left', fontsize=10)
ax1.grid(True, alpha=0.3, which='both')
ax1.yaxis.set_major_formatter(mticker.FuncFormatter(lambda v, _: f'${v:,.0f}'))

# Mark year boundaries (approx: bar ~365/yr)
for yr in range(2019, 2027):
    bar_yr = (yr - 2018) * 365
    if bar_yr < len(bars):
        ax1.axvline(bar_yr, color='gray', linestyle=':', linewidth=0.7, alpha=0.5)
        ax1.text(bar_yr + 5, ax1.get_ylim()[1] * 0.7, str(yr), fontsize=8, color='gray')

# Panel 2: drawdown
ax2.fill_between(bars, dd, 0, color='#E53935', alpha=0.35, label='Turtle+Chandelier DD')
ax2.set_ylabel('Drawdown (%)', fontsize=11)
ax2.set_xlabel('Trading Days', fontsize=11)
ax2.set_title('Drawdown', fontsize=11)
ax2.grid(True, alpha=0.3)
ax2.set_ylim(0, max(dd) * 1.05)

# Summary metrics box
total_ret = (eq[-1] / eq[0] - 1) * 100
btc_ret   = (btc[-1] / btc[0] - 1) * 100
max_dd    = max(dd)
sharpe    = 1.28  # from harness

textstr = (f'Total Return: {total_ret:,.0f}%  |  BTC: {btc_ret:,.0f}%\n'
           f'MaxDD: {max_dd:.1f}%  |  Daily Sharpe: {sharpe:.2f}  |  Trades: 355')
fig.text(0.5, 0.01, textstr, ha='center', fontsize=10,
         bbox=dict(boxstyle='round', facecolor='#F5F5F5', alpha=0.8))

plt.tight_layout(rect=[0, 0.04, 1, 0.97])
plt.savefig('charts/turtle_chandelier_production_equity.png', dpi=150, bbox_inches='tight')
print('Saved: charts/turtle_chandelier_production_equity.png')
plt.close()
