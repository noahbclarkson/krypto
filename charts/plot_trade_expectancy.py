#!/usr/bin/env python3
"""
Plot trade expectancy analysis from snapshots/trade_expectancy.csv
"""
import csv
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import numpy as np

rets = []
symbols = []
years = []
exit_reasons = []
bars_held = []

with open('/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/trade_expectancy.csv') as f:
    reader = csv.DictReader(f)
    for row in reader:
        rets.append(float(row['ret_pct']))
        symbols.append(row['symbol'])
        years.append(int(row['year']))
        exit_reasons.append(row['exit_reason'])
        bars_held.append(int(row['bars_held']))

rets = np.array(rets)
symbols = np.array(symbols)
years = np.array(years)
exit_reasons = np.array(exit_reasons)
bars_held = np.array(bars_held)

n = len(rets)
win_rate = (rets > 0).sum() / n * 100
wins = rets[rets > 0]
losses = rets[rets < 0]
avg_win = wins.mean() if len(wins) > 0 else 0
avg_loss = losses.mean() if len(losses) > 0 else 0
avg_trade = rets.mean()
expectancy = (win_rate / 100) * (avg_win / 100) - ((100 - win_rate) / 100) * (abs(avg_loss) / 100)
pf = (len(wins) * avg_win) / (len(losses) * abs(avg_loss)) if len(losses) > 0 and abs(avg_loss) > 0 else 0
median = np.median(rets)
sorted_rets = np.sort(rets)
p5 = sorted_rets[int(n * 0.05)]
p95 = sorted_rets[int(n * 0.95)]

fig, axes = plt.subplots(2, 2, figsize=(14, 10))

# 1. Return distribution histogram
ax = axes[0, 0]
ax.hist(rets, bins=30, edgecolor='black', alpha=0.7, color='steelblue')
ax.axvline(0, color='red', lw=1.5, ls='--')
ax.axvline(avg_trade, color='green', lw=2, label=f'Mean: {avg_trade:.1f}%')
ax.axvline(median, color='orange', lw=2, ls='--', label=f'Median: {median:.1f}%')
ax.set_xlabel('Return (%)')
ax.set_ylabel('Count')
ax.set_title(f'Return Distribution (n={n})')
ax.legend()

# 2. Cumulative P&L (sorted returns)
ax = axes[0, 1]
sorted_idx = np.argsort(rets)
cumsum = np.cumsum(np.sort(rets))
ax.fill_between(range(n), cumsum, 0, where=(cumsum >= 0), color='green', alpha=0.3, label='Win')
ax.fill_between(range(n), cumsum, 0, where=(cumsum < 0), color='red', alpha=0.3, label='Loss')
ax.plot(range(n), cumsum, color='black', lw=1.5)
ax.axhline(0, color='red', lw=1, ls='--')
ax.set_xlabel('Trade Rank')
ax.set_ylabel('Cumulative Return (%)')
ax.set_title('Cumulative Return by Trade')
ax.legend()

# 3. Per-year avg returns
ax = axes[1, 0]
year_data = {}
for yr in np.unique(years):
    mask = years == yr
    yr_rets = rets[mask]
    year_data[yr] = (yr_rets.mean(), yr_rets.sum(), len(yr_rets))

all_years = sorted(year_data.keys())
avgs = [year_data[yr][0] for yr in all_years]
counts = [year_data[yr][2] for yr in all_years]
colors = ['green' if a > 0 else 'red' for a in avgs]
bars = ax.bar([str(yr) for yr in all_years], avgs, color=colors, alpha=0.7, edgecolor='black')
for bar, c in zip(bars, counts):
    ax.text(bar.get_x() + bar.get_width()/2, bar.get_height() + 0.5,
            f'n={c}', ha='center', va='bottom', fontsize=8)
ax.axhline(0, color='black', lw=1)
ax.set_xlabel('Year')
ax.set_ylabel('Avg Return (%)')
ax.set_title('Avg Return by Year')
ax.tick_params(axis='x', rotation=45)

# 4. Exit reason breakdown
ax = axes[1, 1]
reason_data = {}
for reason in ['chandelier', 'turtle_atr', 'hold_max']:
    mask = exit_reasons == reason
    if mask.sum() > 0:
        reason_data[reason] = {
            'n': mask.sum(),
            'avg': rets[mask].mean(),
            'wr': (rets[mask] > 0).sum() / mask.sum() * 100
        }
labels = list(reason_data.keys())
ns = [reason_data[r]['n'] for r in labels]
avgs_r = [reason_data[r]['avg'] for r in labels]
x = np.arange(len(labels))
width = 0.35
bars1 = ax.bar(x - width/2, ns, width, label='Trade Count', color='steelblue', alpha=0.7)
ax2 = ax.twinx()
bars2 = ax2.bar(x + width/2, avgs_r, width, label='Avg Return %', color='orange', alpha=0.7)
ax.set_ylabel('Trade Count')
ax2.set_ylabel('Avg Return (%)')
ax.set_xticks(x)
ax.set_xticklabels(labels)
ax.set_title('Exit Reason Breakdown')
ax.legend(loc='upper left')
ax2.legend(loc='upper right')

plt.suptitle(
    f'Trade Expectancy Analysis — Turtle+Chandelier (NoDOGE)\n'
    f'Win Rate={win_rate:.0f}% | Avg Win={avg_win:.1f}% | Avg Loss={avg_loss:.1f}% | '
    f'E={expectancy*100:.2f} | PF={pf:.2f}',
    fontsize=12, y=1.01
)
plt.tight_layout()
plt.savefig('/home/ubuntu/.openclaw/workspace-krypto/krypto/charts/trade_expectancy.png', dpi=150, bbox_inches='tight')
print("Saved charts/trade_expectancy.png")