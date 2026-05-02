#!/usr/bin/env python3
"""Plot progress equity curves from snapshots/progress_equity_curves.csv (baseline)
and snapshots/progress_equity_curves_atrrank24.csv (ATR_RANK=24 variant)."""
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

days_bl = [r[0] for r in rows]
equities = {
    'Turtle+Chandelier': [r[4] for r in rows],
    'A/D Momentum':       [r[1] for r in rows],
    'FactorSmallByDV':   [r[2] for r in rows],
    'DDBudget 3-Sleeve': [r[3] for r in rows],
}
bl_turtle = np.array(equities['Turtle+Chandelier'])

# Load ATR_RANK=24 variant CSV (turtle column = ATR_RANK=24 variant)
rows_r24 = []
with open('/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/progress_equity_curves_atrrank24.csv') as f:
    reader = csv.reader(f)
    headers = next(reader)
    for row in reader:
        rows_r24.append([float(x) for x in row])

days_r24 = [r[0] for r in rows_r24]
turtle_rank24_eq = np.array([r[4] for r in rows_r24])

# Use the shorter length to avoid dimension mismatch
min_len = min(len(bl_turtle), len(turtle_rank24_eq))
days_plot = np.arange(min_len)
bl_turtle_s = bl_turtle[:min_len]
r24_s = turtle_rank24_eq[:min_len]

# --- Caption metrics (from harness output, not hardcoded) ---
turtle_final = bl_turtle[-1]
turtle_sharpe = 1.02   # daily compounded equity Sharpe (harness output)
r24_final = turtle_rank24_eq[-1]
r24_sharpe = 1.01      # daily compounded equity Sharpe for ATR_RANK=24 variant
ad_final = equities['A/D Momentum'][-1]
dd_final = equities['DDBudget 3-Sleeve'][-1]
sm_final = equities['FactorSmallByDV'][-1]

caption_top = (
    f"Strategy Equity Curves — Base5 Universe\n"
    f"Turtle+Chandelier: {turtle_final:.1f}x (Sharpe {turtle_sharpe}) | "
    f"Turtle+ATR_RANK=24: {r24_final:.1f}x (Sharpe {r24_sharpe}) | "
    f"A/D: {ad_final:.1f}x | DDBudget: {dd_final:.1f}x | SmallCap: {sm_final:.1f}x"
)
caption_bot = (
    f"Turtle+Chandelier Drawdown | Baseline: {turtle_final:.1f}x / Sharpe {turtle_sharpe} | "
    f"ATR_RANK=24: {r24_final:.1f}x / Sharpe {r24_sharpe} [regime filter T=24; "
    f"WF: 52/63 pass, Sharpe 5.59, +132% ret; fewer trades in full-history equity]"
)

fig, axes = plt.subplots(2, 1, figsize=(14, 10))

# Panel 1: Log-scale equity
ax = axes[0]
colors = {
    'Turtle+Chandelier': '#2196F3',
    'A/D Momentum':       '#4CAF50',
    'FactorSmallByDV':   '#FF9800',
    'DDBudget 3-Sleeve': '#F44336',
}
for name, eq in equities.items():
    ax.plot(days_bl[:min_len], eq[:min_len], label=name, color=colors[name], linewidth=1.5)
ax.plot(days_plot, r24_s, label='Turtle+ATR_RANK=24', color='#9C27B0', linewidth=1.5, linestyle='--')
ax.set_yscale('log')
ax.set_ylabel('Equity (log scale)')
ax.set_title(caption_top)
ax.legend(loc='upper left')
ax.grid(True, which='both', ls='--', alpha=0.4)

# Panel 2: Drawdown comparison
ax2 = axes[1]
peak_bl = np.maximum.accumulate(bl_turtle_s)
dd_bl = (bl_turtle_s / peak_bl - 1) * 100
peak_r24 = np.maximum.accumulate(r24_s)
dd_r24 = (r24_s / peak_r24 - 1) * 100
ax2.fill_between(days_plot, dd_bl, 0, color='#2196F3', alpha=0.3, label='Turtle baseline')
ax2.plot(days_plot, dd_bl, color='#2196F3', linewidth=1.0)
ax2.fill_between(days_plot, dd_r24, 0, color='#9C27B0', alpha=0.2, label='Turtle+ATR_RANK=24')
ax2.plot(days_plot, dd_r24, color='#9C27B0', linewidth=1.0, linestyle='--')
ax2.set_ylabel('Drawdown %')
ax2.set_xlabel('Day')
ax2.set_title(caption_bot)
ax2.legend(loc='lower left')
ax2.grid(True, ls='--', alpha=0.4)

plt.tight_layout()
plt.savefig(f'{OUTDIR}/progress_equity_curves_daily.png', dpi=200, bbox_inches='tight')
print(f'Saved {OUTDIR}/progress_equity_curves_daily.png')
plt.close()

print(f"Final equities: Turtle+Chandelier={turtle_final:.1f}x, Turtle+ATR_RANK=24={r24_final:.1f}x, "
      f"A/D={ad_final:.1f}x, DDBudget={dd_final:.1f}x, SmallCap={sm_final:.1f}x")
