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

days = np.array([r[0] for r in rows])  # length 2099, day 0-2098
equities = {
    'Turtle+Chandelier': np.array([r[4] for r in rows]),
    'A/D Momentum':       np.array([r[1] for r in rows]),
    'FactorSmallByDV':   np.array([r[2] for r in rows]),
    'DDBudget 3-Sleeve': np.array([r[3] for r in rows]),
}

# Load ATR_RANK=24 variant CSV
rows_r24 = []
with open('snapshots/progress_equity_curves_atrrank24.csv') as f:
    reader = csv.reader(f)
    headers = next(reader)
    for row in reader:
        rows_r24.append([float(x) for x in row])
turtle_rank24_eq = np.array([r[4] for r in rows_r24])

# Load live bot exact equity (bar-indexed 300-2099)
live_rows = []
with open('snapshots/live_bot_exact_equity.csv') as f:
    reader = csv.reader(f)
    headers = next(reader)
    for row in reader:
        live_rows.append([int(row[0]), float(row[2])])

# Build aligned array of length 2099 (matching days array), forward-fill
live_bot_aligned = np.full(len(days), np.nan)
for bar, equity in live_rows:
    day_idx = bar - 300  # bar 300 -> day 0
    if 0 <= day_idx < len(days):
        live_bot_aligned[day_idx] = equity

# Forward fill NaN gaps (live bot starts at bar 300, not day 0)
prev = 1.0
for i in range(len(live_bot_aligned)):
    if np.isnan(live_bot_aligned[i]):
        live_bot_aligned[i] = prev
    else:
        prev = live_bot_aligned[i]

fig, axes = plt.subplots(2, 1, figsize=(14, 10))

# Panel 1: Log-scale equity — all strategies
ax = axes[0]
colors = {'Turtle+Chandelier': '#2196F3', 'A/D Momentum': '#4CAF50',
          'FactorSmallByDV': '#FF9800', 'DDBudget 3-Sleeve': '#F44336'}
for name, eq in equities.items():
    ax.plot(days, eq, label=name, color=colors[name], linewidth=1.5)
ax.plot(days, turtle_rank24_eq, label='Turtle ATR_RANK=24 (AP=64, T=24)',
        color='#9C27B0', linewidth=1.5, linestyle='--')
ax.plot(days, live_bot_aligned, label='Turtle ATR-only (LIVE BOT): 2.76x',
        color='#FF5722', linewidth=2.0, linestyle=':')
ax.set_yscale('log')
ax.set_ylabel('Equity (log scale)')
ax.set_title(
    'Strategy Equity Curves — Base5 Universe (2099 days, 2018-2026)\n'
    'Turtle+Chandelier: 636x | Turtle ATR_RANK=24: 76.5x | A/D: 39.3x | DDBudget: 60.5x | '
    'FactorSmallByDV: 15.7x | Live Bot (Turtle ATR-only): 2.76x'
)
ax.legend(loc='upper left')
ax.grid(True, which='both', ls='--', alpha=0.4)

# Panel 2: Drawdown — live bot only
ax2 = axes[1]
peak_live = np.maximum.accumulate(live_bot_aligned)
dd_live = (live_bot_aligned / peak_live - 1) * 100
ax2.fill_between(days, dd_live, 0, color='#FF5722', alpha=0.3, label='Live Bot (Turtle ATR-only, 2.76x)')
ax2.plot(days, dd_live, color='#FF5722', linewidth=1.0, linestyle=':')
ax2.set_ylabel('Drawdown %')
ax2.set_xlabel('Day')
ax2.set_title('Drawdown: Live Bot Turtle ATR-only (2.76x / Sharpe 1.03 / MaxDD 22.3%)')
ax2.legend(loc='lower left')
ax2.grid(True, ls='--', alpha=0.4)

plt.tight_layout()
plt.savefig(f'{OUTDIR}/progress_equity_curves_daily.png', dpi=150, bbox_inches='tight')
print(f"Saved {OUTDIR}/progress_equity_curves_daily.png")
plt.close()

print(f"Final equities: {', '.join([f'{k}={v[-1]:.1f}x' for k,v in equities.items()])}")
print(f"Turtle ATR_RANK=24 (production): {turtle_rank24_eq[-1]:.1f}x")
print(f"Live Bot Turtle ATR-only: {live_bot_aligned[-1]:.2f}x")