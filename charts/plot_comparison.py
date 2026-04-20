#!/usr/bin/env python3
"""
Comprehensive comparison chart for Turtle+Chandelier strategy.
Reads equity data from snapshots/progress_equity_curves.csv and generates:
  1. Log-scale equity curves for all strategy families
  2. Turtle drawdown (linear scale)
  3. Cumulative equity from inception (calendar-year aligned)

Chart rules:
  - Equity: LOG scale (critical for comparing strategies with very different magnitudes)
  - Drawdown: LINEAR scale
  - Dynamic Y-axis (do NOT force 0 as minimum for log charts)
  - Clear legend, axis labels, title, grid lines
  - Save as comparison_chart.png
"""

import csv, os, sys
import numpy as np

OUTDIR = '/home/ubuntu/.openclaw/workspace-krypto/krypto/charts'
DATA   = '/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/progress_equity_curves.csv'
os.makedirs(OUTDIR, exist_ok=True)

# ── Load data ──────────────────────────────────────────────────────────────────
rows = []
with open(DATA) as f:
    reader = csv.reader(f)
    next(reader)  # skip header
    for row in reader:
        rows.append([float(x) for x in row])

days     = [r[0] for r in rows]
equities = {
    'Turtle+Chandelier': [r[4] for r in rows],
    'A/D Momentum':      [r[1] for r in rows],
    'FactorSmallByDV':   [r[2] for r in rows],
    'DDBudget 3-Sleeve': [r[3] for r in rows],
}

# Calendar year for each day (day 0 = 2018-01-01)
year_for_day = lambda d: 2018 + int(d // 365)

# ── Compute key metrics ─────────────────────────────────────────────────────────
def metrics(eq):
    eq = np.array(eq)
    rets = np.diff(eq) / eq[:-1]
    rets = rets[np.isfinite(rets) & (rets != 0)]
    if len(rets) == 0 or np.std(rets) < 1e-10:
        return {'sharpe': 0.0, 'maxdd': 0.0, 'final': float(eq[-1]), 'ann_ret': 0.0}
    sharpe  = np.mean(rets) / np.std(rets) * np.sqrt(365)
    peak    = np.maximum.accumulate(eq)
    dd      = (eq / peak - 1) * 100
    maxdd   = float(dd.min())
    ann_ret = (float(eq[-1]) / float(eq[0])) ** (365.0 / len(eq)) - 1
    return {'sharpe': sharpe, 'maxdd': maxdd, 'final': float(eq[-1]), 'ann_ret': ann_ret}

def annual_returns(eq, days):
    """Return cumulative equity at end of each calendar year (Dec 31 ~= day 364 + y*365)."""
    year_vals = {}
    for d, v in zip(days, eq):
        y = year_for_day(d)
        year_vals.setdefault(y, []).append(v)

    annual = {}
    for y, vals in sorted(year_vals.items()):
        if len(vals) >= 2:
            annual[y] = (vals[-1], vals[0])  # (year-end equity, year-start equity)
    return annual

ann = annual_returns(equities['Turtle+Chandelier'], days)

print("\n=== Strategy Comparison Metrics ===")
for name, eq in equities.items():
    m = metrics(eq)
    print(f"  {name:20s}: Final {m['final']:.1f}x | Sharpe {m['sharpe']:.2f} | MaxDD {m['maxdd']:.1f}% | Ann.Ret {m['ann_ret']*100:.1f}%")

print("\n=== Turtle+Chandelier Year-End Equity ===")
for y, (end_eq, start_eq) in sorted(ann.items()):
    ret = (end_eq / start_eq - 1) * 100 if start_eq > 0 else 0
    print(f"  {y}: {start_eq:.2f}x → {end_eq:.2f}x  ({ret:+.0f}%)")

# ── Generate charts ─────────────────────────────────────────────────────────────
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import matplotlib.ticker as mticker

# ── Chart 1: Equity curves (log scale) + drawdown ─────────────────────────────
fig, axes = plt.subplots(2, 1, figsize=(14, 10))

ax = axes[0]
colors = {
    'Turtle+Chandelier': '#2196F3',
    'A/D Momentum':      '#4CAF50',
    'FactorSmallByDV':   '#FF9800',
    'DDBudget 3-Sleeve': '#F44336',
}
for name, eq in equities.items():
    ax.plot(days, eq, label=name, color=colors[name], linewidth=1.5, alpha=0.9)
ax.set_yscale('log')
ax.set_ylabel('Equity (log scale)')
ax.set_title('Strategy Equity Curves — Base5 Universe (2079 days, 2018–2026)\n'
             'Turtle+Chandelier: CHAND(15,1.50)+ATR(24,2.0) DUAL EXIT | Params fixed 2026-04-20')
ax.legend(loc='upper left', fontsize=10)
ax.grid(True, which='both', ls='--', alpha=0.4)
ax.yaxis.set_major_formatter(mticker.FuncFormatter(lambda x, _: f'{x:.0f}x' if x >= 1 else f'{x:.2f}'))

# Add year markers on x-axis
year_starts = [(y, 365*(y-2018)) for y in range(2018, 2027)]
for y, start_day in year_starts:
    ax.axvline(start_day, color='gray', linewidth=0.3, alpha=0.5)
    ax.text(start_day+2, ax.get_ylim()[1]*0.5, str(y), fontsize=7, color='gray', alpha=0.7)

# Turtle drawdown
turtle_eq  = np.array(equities['Turtle+Chandelier'])
peak       = np.maximum.accumulate(turtle_eq)
dd         = (turtle_eq / peak - 1) * 100
m_turtle   = metrics(equities['Turtle+Chandelier'])

ax2 = axes[1]
ax2.fill_between(days, dd, 0, color='#2196F3', alpha=0.3)
ax2.plot(days, dd, color='#2196F3', linewidth=0.8)
ax2.set_ylabel('Drawdown %')
ax2.set_xlabel('Day')
ax2.set_title(f'Turtle+Chandelier Drawdown | MaxDD: {m_turtle["maxdd"]:.1f}% | '
              f'Sharpe: {m_turtle["sharpe"]:.2f} | Final: {m_turtle["final"]:.1f}x')
ax2.grid(True, ls='--', alpha=0.4)
for y, start_day in year_starts:
    ax2.axvline(start_day, color='gray', linewidth=0.3, alpha=0.5)

plt.tight_layout()
out = f'{OUTDIR}/comparison_chart.png'
plt.savefig(out, dpi=180, bbox_inches='tight')
print(f"\nSaved: {out}")
plt.close()

# ── Chart 2: Year-end equity bar chart ─────────────────────────────────────────
fig2, ax3 = plt.subplots(figsize=(12, 5))

years     = sorted(ann.keys())
end_equity = [ann[y][0] for y in years]

btc_end = {
    2018: 1.0 * (1-0.72),  2019: (1-0.72)*(1+1.27),   2020: (1-0.72)*(1+1.27)*(1+4.41),
    2021: (1-0.72)*(1+1.27)*(1+4.41)*(1-0.16), 2022: (1-0.72)*(1+1.27)*(1+4.41)*(1-0.16)*(1-0.65),
    2023: (1-0.72)*(1+1.27)*(1+4.41)*(1-0.16)*(1-0.65)*(1+1.47),
}
btc_end_equity = [btc_end.get(y, 1.0) for y in years]

x = list(range(len(years)))
bar_width = 0.35
bars1 = ax3.bar([i - bar_width/2 for i in x], end_equity, bar_width,
                label='Turtle+Chandelier', color='#2196F3', alpha=0.85)
bars2 = ax3.bar([i + bar_width/2 for i in x], btc_end_equity, bar_width,
                label='BTC Buy&Hold (approx)', color='#9E9E9E', alpha=0.7)

ax3.set_xticks(x)
ax3.set_xticklabels(years, fontsize=11)
ax3.set_ylabel('Cumulative Equity (x from initial $1)')
ax3.set_title('Year-End Cumulative Equity: Turtle+Chandelier vs BTC Buy&Hold')
ax3.legend()
ax3.set_yscale('log')
ax3.yaxis.set_major_formatter(mticker.FuncFormatter(lambda x, _: f'{x:.0f}x' if x >= 1 else f'{x:.2f}'))
ax3.grid(True, axis='y', ls='--', alpha=0.4)

for bar, val in zip(bars1, end_equity):
    ax3.text(bar.get_x() + bar.get_width()/2, max(val, 0.01),
             f'{val:.1f}x', ha='center', va='bottom', fontsize=8, color='#1565C0')

plt.tight_layout()
out2 = f'{OUTDIR}/comparison_annual_returns.png'
plt.savefig(out2, dpi=180, bbox_inches='tight')
print(f"Saved: {out2}")
plt.close()

print("\n=== Done ===")
