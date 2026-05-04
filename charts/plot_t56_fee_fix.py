#!/usr/bin/env python3
"""T56: Plot corrected progress equity curves after fee fix.

Fee bug FIXED: long entry now uses (1 + TAKER_FEE), not (1 - TAKER_FEE).
Prior equity figures were OVERSTATED.
"""
import csv
import os
import numpy as np
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt

OUTDIR = '/home/ubuntu/.openclaw/workspace-krypto/krypto/charts'
os.makedirs(OUTDIR, exist_ok=True)

# Load main CSV (turtle column = Turtle+Chandelier baseline)
rows = []
with open('snapshots/progress_equity_curves.csv') as f:
    reader = csv.reader(f)
    next(reader)  # skip header
    for row in reader:
        rows.append([float(x) for x in row])

days = np.array([r[0] for r in rows])
equities = {
    'Turtle+Chandelier (baseline)': np.array([r[4] for r in rows]),
    'A/D Momentum':                  np.array([r[1] for r in rows]),
    'FactorSmallByDV':              np.array([r[2] for r in rows]),
    'DDBudget 3-Sleeve':            np.array([r[3] for r in rows]),
}

# Load ATR_RANK=5 variant CSV
rows_r5 = []
with open('snapshots/progress_equity_curves_atrrank5.csv') as f:
    reader = csv.reader(f)
    next(reader)
    for row in reader:
        rows_r5.append([float(x) for x in row])
turtle_rank5 = np.array([r[4] for r in rows_r5])

def sharpe(eq):
    rets = np.diff(eq)
    if len(rets) < 2 or np.std(rets) < 1e-10:
        return 0.0
    return np.mean(rets) / np.std(rets) * np.sqrt(252)

def max_dd(eq):
    peak = np.maximum.accumulate(eq)
    dd = (eq / peak - 1) * 100
    return dd.min()

# Compute metrics
metrics = {}
for name, eq in equities.items():
    metrics[name] = {
        'final': eq[-1],
        'sharpe': sharpe(eq),
        'maxdd': max_dd(eq),
    }
metrics['Turtle ATR_RANK=5 (production)'] = {
    'final': turtle_rank5[-1],
    'sharpe': sharpe(turtle_rank5),
    'maxdd': max_dd(turtle_rank5),
}

print("=== T56 Fee Fix — Corrected Results ===")
for name, m in metrics.items():
    print(f"  {name}: {m['final']:.1f}x, Sharpe {m['sharpe']:.2f}, MaxDD {m['maxdd']:.1f}%")

# ── Chart 1: Log-scale equity all strategies ──────────────────────────────
fig, axes = plt.subplots(2, 1, figsize=(14, 10))

ax = axes[0]
colors = {
    'Turtle+Chandelier (baseline)': '#2196F3',
    'A/D Momentum':                  '#4CAF50',
    'FactorSmallByDV':              '#FF9800',
    'DDBudget 3-Sleeve':            '#F44336',
}
for name, eq in equities.items():
    ax.plot(days, eq, label=name, color=colors[name], linewidth=1.5)
ax.plot(days, turtle_rank5, label='Turtle ATR_RANK=5 (production)', 
        color='#9C27B0', linewidth=1.5, linestyle='--')
ax.set_yscale('log')
ax.set_ylabel('Equity (log scale)')
ax.set_title(
    'Strategy Equity Curves — Base5 Universe (2018–2026, 2092 days)\n'
    'T56 FEE FIX: entry_px * (1 + TAKER_FEE) for longs. Prior figures OVERSTATED.\n'
    f"Turtle+Chandelier: {metrics['Turtle+Chandelier (baseline)']['final']:.1f}x "
    f"(Sharpe {metrics['Turtle+Chandelier (baseline)']['sharpe']:.2f}) | "
    f"Turtle ATR_RANK=5: {metrics['Turtle ATR_RANK=5 (production)']['final']:.1f}x "
    f"(Sharpe {metrics['Turtle ATR_RANK=5 (production)']['sharpe']:.2f})"
)
ax.legend(loc='upper left', fontsize=9)
ax.grid(True, which='both', ls='--', alpha=0.4)

# ── Chart 2: Drawdown comparison ──────────────────────────────────────────
ax2 = axes[1]
turtle_bl = equities['Turtle+Chandelier (baseline)']
turtle_r5 = turtle_rank5
for eq, label, col, alpha in [
    (turtle_bl, 'Turtle+Chandelier baseline', '#2196F3', 0.3),
    (turtle_r5, 'Turtle ATR_RANK=5', '#9C27B0', 0.2),
]:
    peak = np.maximum.accumulate(eq)
    dd = (eq / peak - 1) * 100
    ax2.fill_between(days, dd, 0, color=col, alpha=alpha)
    ax2.plot(days, dd, color=col, linewidth=1.0, label=f'{label} ({dd.min():.1f}% DD)')

ax2.set_ylabel('Drawdown %')
ax2.set_xlabel('Day')
ax2.set_title('Drawdown: Turtle+Chandelier baseline vs Turtle ATR_RANK=5')
ax2.legend(loc='lower left', fontsize=9)
ax2.grid(True, ls='--', alpha=0.4)

plt.tight_layout()
out_path = f'{OUTDIR}/t56_fee_fix_comparison.png'
plt.savefig(out_path, dpi=150, bbox_inches='tight')
print(f"\nChart saved: {out_path}")
plt.close()
