#!/usr/bin/env python3
"""Plot unranked vs ranked walk-forward comparison."""
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import matplotlib.ticker as mticker
import pandas as pd

df = pd.read_csv('snapshots/unranked_vs_ranked_wf.csv')
df = df[df['window'].str.startswith('W')].copy()
df['w'] = df['window'].str.replace('W','').astype(int)

windows = [f"W{w:02d}" for w in sorted(df['w'])]
ranked_sharpe = [float(df[df['window']==w]['ranked_sharpe'].values[0]) for w in windows]
unranked_sharpe = [float(df[df['window']==w]['unranked_sharpe'].values[0]) for w in windows]
ranked_dd = [float(df[df['window']==w]['ranked_dd'].values[0]) for w in windows]
unranked_dd = [float(df[df['window']==w]['unranked_dd'].values[0]) for w in windows]

fig, axes = plt.subplots(1, 2, figsize=(14, 5))

# Sharpe comparison
ax = axes[0]
x = range(len(windows))
w_sharpe = [v/100 for v in ranked_sharpe]  # pct
u_sharpe = [v/100 for v in unranked_sharpe]
ax.bar([i-0.2 for i in x], ranked_sharpe, width=0.35, label='Ranked (prod)', color='#2196F3', alpha=0.85)
ax.bar([i+0.2 for i in x], unranked_sharpe, width=0.35, label='Unranked', color='#FF5722', alpha=0.85)
ax.axhline(0, color='black', linewidth=0.5)
ax.set_xticks(list(x))
ax.set_xticklabels(windows, fontsize=11)
ax.set_ylabel('Sharpe Ratio', fontsize=12)
ax.set_title('Sharpe Ratio by Window: Ranked vs Unranked', fontsize=13, fontweight='bold')
ax.legend(fontsize=11)
ax.yaxis.set_major_formatter(mticker.FormatStrFormatter('%.2f'))

# Add pass/fail markers
for i, (rs, us) in enumerate(zip(ranked_sharpe, unranked_sharpe)):
    mark_r = '✓' if rs > 0 else '✗'
    mark_u = '✓' if us > 0 else '✗'
    ax.annotate(mark_r, (i-0.2, rs + 0.05 if rs >= 0 else rs - 0.15), ha='center', fontsize=12, color='green' if rs>0 else 'red')
    ax.annotate(mark_u, (i+0.2, us + 0.05 if us >= 0 else us - 0.15), ha='center', fontsize=12, color='green' if us>0 else 'red')

# DD comparison
ax2 = axes[1]
ax2.bar([i-0.2 for i in x], ranked_dd, width=0.35, label='Ranked (prod)', color='#2196F3', alpha=0.85)
ax2.bar([i+0.2 for i in x], unranked_dd, width=0.35, label='Unranked', color='#FF5722', alpha=0.85)
ax2.set_xticks(list(x))
ax2.set_xticklabels(windows, fontsize=11)
ax2.set_ylabel('Max Drawdown (%)', fontsize=12)
ax2.set_title('Max Drawdown by Window: Ranked vs Unranked', fontsize=13, fontweight='bold')
ax2.legend(fontsize=11)
ax2.yaxis.set_major_formatter(mticker.FormatStrFormatter('%.0f%%'))

# Summary text
summary = "RANKED: 6/6 pass, Sharpe=2.54, DD=59.3%\nUNRANKED: 5/6 pass, Sharpe=0.97, DD=40.3%"
fig.text(0.5, -0.04, summary, ha='center', fontsize=11, style='italic',
         bbox=dict(boxstyle='round', facecolor='#f0f0f0', alpha=0.8))

plt.tight_layout()
plt.savefig('charts/unranked_vs_ranked_wf.png', dpi=150, bbox_inches='tight')
print("Saved charts/unranked_vs_ranked_wf.png")
