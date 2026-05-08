#!/usr/bin/env python3
"""M1 Status Chart — compact operational dashboard."""
import sys, math
import csv

equity_file = 'snapshots/live_bot_exact_equity.csv'
trades_file = 'snapshots/live_bot_exact_trades.csv'

# Load equity
dates, equity = [], []
with open(equity_file) as f:
    reader = csv.DictReader(f)
    for row in reader:
        dates.append(row['date'])
        equity.append(float(row['equity']))

n = len(equity)
rets = [math.log(equity[i]/equity[i-1]) if equity[i-1]>0 else 0 for i in range(1,n)]

# 60d rolling return
ROLL = 60
roll_rets = []
for i in range(ROLL, n):
    roll_rets.append((equity[i]/equity[i-ROLL]-1)*100)

curr_60d = roll_rets[-1]
curr_date = dates[n-ROLL]

# Percentiles
sorted_rolls = sorted(roll_rets)
def pct(p): return sorted_rolls[int(p*(len(sorted_rolls)-1))]

p5, p10, p50 = pct(0.05), pct(0.10), pct(0.50)

# Drawdown
peak = max(equity)
current_eq = equity[-1]
dd_pct = (1 - current_eq/peak)*100
dd_from_peak = (current_eq/peak - 1)*100

# Peak in last 252d
peak_252 = max(equity[max(0,n-252):])
vs_252 = (current_eq/peak_252 - 1)*100

# Equity growth
total_mult = equity[-1]
ann_days = n
ann_ret = (total_mult**(365.0/ann_days) - 1)*100 if total_mult > 0 else 0

# Daily Sharpe
mean_ret = sum(rets)/len(rets)
var_ret = sum((r-mean_ret)**2 for r in rets)/len(rets)
daily_sharpe = (mean_ret/var_ret**0.5) if var_ret>0 else 0
ann_sharpe = daily_sharpe * math.sqrt(365)

# Tail concentration
log_rets = []
with open(trades_file) as f:
    reader = csv.DictReader(f)
    for row in reader:
        eq = max(float(row['equity_mult']), 1e-12)
        ln = math.log(eq)
        if ln > -50:
            log_rets.append(ln)
log_rets.sort(reverse=True)
total_log = sum(log_rets)
top10_share = sum(log_rets[:10])/total_log*100 if total_log != 0 else 0
eq_no_top10 = math.exp(total_log - sum(log_rets[:10]))

# Status
if curr_60d < p5 or vs_252 < -20:
    status_icon = "🔴"
    status_text = "RED ALERT"
elif curr_60d < p10:
    status_icon = "🟡"
    status_text = "YELLOW ALERT"
else:
    status_icon = "🟢"
    status_text = "GREEN"

# Plot
try:
    import matplotlib.pyplot as plt
    import matplotlib.dates as mdates
    from datetime import datetime
except ImportError:
    print("matplotlib not available, skipping chart")
    sys.exit(0)

fig, axes = plt.subplots(2, 1, figsize=(10, 7), gridspec_kw={'height_ratios': [3, 1]})

# Equity (log scale)
ax1 = axes[0]
xs = range(n)
ax1.semilogy(xs, equity, color='#2196F3', linewidth=1.2, label='Live Bot Equity')
ax1.axhline(y=1.0, color='gray', linewidth=0.5, linestyle='--')
ax1.set_ylabel('Equity (USD, log scale)', fontsize=9)
ax1.set_title(
    f'M1 Status — {curr_date[:10]} | {status_icon} {status_text} | '
    f'{total_mult:.2f}x ({ann_ret:+.1f}%/yr) | Sharpe {ann_sharpe:.2f} | MaxDD {dd_pct:.1f}%',
    fontsize=10
)
ax1.text(0.5, 0.02,
    f'60d: {curr_60d:+.1f}% | P10: {p10:+.1f}% P50: {p50:+.1f}% | vs 1y peak: {vs_252:+.1f}% | '
    f'Top10: {top10_share:.0f}% of equity',
    transform=ax1.transAxes, fontsize=8, ha='center', va='bottom',
    color='red' if status_icon == '🔴' else ('orange' if status_icon == '🟡' else 'green'),
    bbox=dict(boxstyle='round', facecolor='white', alpha=0.7)
)
ax1.grid(True, alpha=0.2)
ax1.set_xlim(0, n)

# Rolling 60d return with pctile bands
ax2 = axes[1]
ax2.fill_between(range(len(roll_rets)), p5, p95, color='#eee', alpha=0.6, label='5-95th pctile')
ax2.fill_between(range(len(roll_rets)), p10, p90, color='#ddd', alpha=0.6, label='10-90th pctile')
ax2.axhline(y=0, color='gray', linewidth=0.5, linestyle='--')
ax2.plot(range(len(roll_rets)), roll_rets, color='#2196F3', linewidth=0.8, label='60d return')
ax2.scatter([len(roll_rets)-1], [curr_60d], color='#2196F3', s=30, zorder=5)
ax2.set_ylabel('60d Return (%)', fontsize=9)
ax2.set_xlabel('Bar index', fontsize=9)
ax2.legend(fontsize=7, loc='upper right')
ax2.grid(True, alpha=0.2)
ax2.set_xlim(0, n)

plt.tight_layout()
out = 'charts/m1_status.png'
plt.savefig(out, dpi=150, bbox_inches='tight')
print(f'Saved {out}')
plt.close()
