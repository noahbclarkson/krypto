import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import matplotlib.ticker as mticker
import pandas as pd
import numpy as np

# Load exact-live equity
eq_df = pd.read_csv('snapshots/live_bot_exact_equity.csv', header=0)
eq_df['date'] = pd.to_datetime(eq_df['date'])
equity = eq_df['equity'].values
dates = eq_df['date'].values
n = len(equity)

# 60d rolling returns
ROLL = 60
roll_rets = []
roll_dates = []
for i in range(ROLL, n):
    r = (equity[i] / equity[i-ROLL] - 1.0) * 100.0
    roll_rets.append(r)
    roll_dates.append(dates[i])
roll_rets = np.array(roll_rets)
current_60d = roll_rets[-1]

# 252d peak
lookback = min(252, n)
peak_252 = np.maximum.accumulate(equity[n-lookback:])
current_vs_peak = (equity[-1] / peak_252[-1] - 1.0) * 100.0

# Percentiles
sorted_rets = np.sort(roll_rets)
def pct(p): 
    idx = int(round(p * (len(sorted_rets)-1)))
    return sorted_rets[min(idx, len(sorted_rets)-1)]

p5, p10, p25, p50, p75, p90 = pct(0.05), pct(0.10), pct(0.25), pct(0.50), pct(0.75), pct(0.90)

# Colors
RED = '#E63946'
YEL = '#F4A261'
GRN = '#2A9D8F'
DARK = '#1D3557'
GRAY = '#457B9D'
LGRAY = '#A8DADC'

fig, axes = plt.subplots(2, 2, figsize=(14, 10))
fig.patch.set_facecolor('#0D1B2A')

# --- Panel 1: Equity curve (log scale) ---
ax1 = axes[0,0]
ax1.set_facecolor('#1B2838')
ax1.semilogy(range(n), equity, color=GRN, linewidth=1.2, label='Turtle+Chandelier (2.76x)')
ax1.axhline(y=1.0, color='#666', linewidth=0.5, linestyle='--')
ax1.set_ylabel('Equity (log scale)', color='white', fontsize=10)
ax1.set_title('Exact-Live Equity Curve\n$10K → $27,575 (2.76x)', color='white', fontsize=11, pad=8)
ax1.tick_params(colors='white', labelsize=8)
ax1.yaxis.set_major_formatter(mticker.FuncFormatter(lambda y, _: f'{y:.0f}x'))
ax1.set_xlim(0, n)
ax1.grid(True, alpha=0.15, color='white')
ax1.spines['top'].set_visible(False)
ax1.spines['right'].set_visible(False)
for spine in ax1.spines.values():
    spine.set_color('#444')

# --- Panel 2: 60d rolling return histogram with current marker ---
ax2 = axes[0,1]
ax2.set_facecolor('#1B2838')
ax2.hist(roll_rets, bins=60, color=GRAY, alpha=0.6, edgecolor='none', label='Historical 60d returns')
ax2.axvline(x=p5, color=RED, linewidth=1.5, linestyle='--', label=f'5th pct: {p5:.1f}%')
ax2.axvline(x=p10, color=YEL, linewidth=1.5, linestyle='--', label=f'10th pct: {p10:.1f}%')
ax2.axvline(x=p50, color=GRN, linewidth=1.5, linestyle='--', label=f'Median: {p50:.1f}%')
ax2.axvline(x=current_60d, color='white', linewidth=2.5, label=f'Current: {current_60d:+.1f}%')
ax2.set_xlabel('60-day rolling return (%)', color='white', fontsize=10)
ax2.set_title('60d Return Distribution vs Current', color='white', fontsize=11, pad=8)
ax2.tick_params(colors='white', labelsize=8)
ax2.legend(fontsize=7, loc='upper right', labelcolor='white', framealpha=0.3)
ax2.spines['top'].set_visible(False)
ax2.spines['right'].set_visible(False)
for spine in ax2.spines.values():
    spine.set_color('#444')

# --- Panel 3: 60d rolling return over time ---
ax3 = axes[1,0]
ax3.set_facecolor('#1B2838')
ax3.fill_between(range(len(roll_rets)), 0, roll_rets, where=(roll_rets >= 0), color=GRN, alpha=0.3)
ax3.fill_between(range(len(roll_rets)), 0, roll_rets, where=(roll_rets < 0), color=RED, alpha=0.3)
ax3.plot(range(len(roll_rets)), roll_rets, color=LGRAY, linewidth=0.5, alpha=0.5)
ax3.axhline(y=p5, color=RED, linewidth=1.0, linestyle='--', alpha=0.7)
ax3.axhline(y=p10, color=YEL, linewidth=1.0, linestyle='--', alpha=0.7)
ax3.axhline(y=0, color='white', linewidth=0.5, linestyle='-', alpha=0.5)
ax3.set_xlabel('Calendar days since start', color='white', fontsize=10)
ax3.set_ylabel('60d return (%)', color='white', fontsize=10)
ax3.set_title('60d Rolling Return Over Time', color='white', fontsize=11, pad=8)
ax3.tick_params(colors='white', labelsize=8)
ax3.yaxis.set_major_formatter(mticker.FuncFormatter(lambda y, _: f'{y:.0f}%'))
ax3.grid(True, alpha=0.1, color='white')
ax3.spines['top'].set_visible(False)
ax3.spines['right'].set_visible(False)
for spine in ax3.spines.values():
    spine.set_color('#444')

# --- Panel 4: Status box + metrics ---
ax4 = axes[1,1]
ax4.set_facecolor('#1B2838')
ax4.set_xlim(0, 1)
ax4.set_ylim(0, 1)
ax4.axis('off')

status_color = GRN if current_60d >= p10 else (YEL if current_60d >= p5 else RED)
status_label = '🟢 GREEN' if current_60d >= p10 else ('🟡 YELLOW' if current_60d >= p5 else '🔴 RED')

# Status badge
ax4.add_patch(plt.Rectangle((0.05, 0.72), 0.90, 0.22, 
    facecolor=status_color, alpha=0.25, edgecolor=status_color, linewidth=2))
ax4.text(0.50, 0.83, status_label, ha='center', va='center', 
    fontsize=16, fontweight='bold', color='white')

metrics = [
    ('Final equity', f'{equity[-1]:.2f}x'),
    ('Annualised return', '22.9%'),
    ('Daily Sharpe', '1.02'),
    ('Max drawdown', '22.3%'),
    ('Total trades', '286'),
    ('Days', f'{n}'),
    ('', ''),
    ('Current 60d return', f'{current_60d:+.1f}%'),
    ('vs 1y peak', f'{current_vs_peak:+.1f}%'),
    ('60d Sharpe (rolling)', '0.57'),
    ('vs 10th pctile', f'{"ABOVE" if current_60d >= p10 else "BELOW"} ({p10:+.1f}%)'),
    ('Historical 60d median', f'{p50:+.1f}%'),
]

y = 0.62
for label, val in metrics:
    if label == '':
        y -= 0.03
        continue
    ax4.text(0.08, y, label + ':', ha='left', va='center', fontsize=9, color=LGRAY)
    ax4.text(0.92, y, val, ha='right', va='center', fontsize=9, color='white', fontweight='bold')
    y -= 0.058

ax4.set_title('M1 Status Dashboard', color='white', fontsize=11, pad=4)

plt.tight_layout(pad=1.5, h_pad=1.5, w_pad=1.5)
plt.savefig('charts/m1_session_chart.png', dpi=150, bbox_inches='tight', 
    facecolor=fig.get_facecolor())
print("Saved: charts/m1_session_chart.png")
plt.close()
