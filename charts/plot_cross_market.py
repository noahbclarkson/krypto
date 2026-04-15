#!/usr/bin/env python3
"""Chart cross-market equity walk-forward results."""

import pandas as pd
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import numpy as np

df = pd.read_csv('snapshots/cross_market_equity_wf.csv')

fig, axes = plt.subplots(3, 1, figsize=(14, 12))

for i, (asset, ax) in enumerate(zip(['SPY', 'QQQ', 'GLD'], axes)):
    sub = df[df['asset'] == asset].copy()
    windows = sub['window'].values
    rets   = sub['return_pct'].values / 100.0
    sh     = sub['sharpe'].values
    dd     = sub['max_dd_pct'].values / 100.0
    passed = sub['pass'].values

    # Equity curve
    equity = np.cumprod(1 + rets)

    ax2 = ax.twinx()
    ax.plot(windows, equity, color='steelblue', lw=2, label=f'{asset} equity')
    ax2.bar(windows, dd * -1, alpha=0.3, color='coral', width=0.6, label='Drawdown')

    for j, (w, r, s, p) in enumerate(zip(windows, rets, sh, passed)):
        col = 'green' if p else 'red'
        ax.annotate(f'{r*100:+.0f}%', (w, equity[j]), fontsize=7,
                    ha='center', va='bottom', color=col)
        ax2.annotate(f'sh={s:.1f}', (w, -dd[j]*1.05), fontsize=6,
                    ha='center', va='top', color='coral')

    # Mark pass/fail
    fail_windows = windows[~passed]
    fail_equity  = equity[~passed]
    ax.scatter(fail_windows, fail_equity, color='red', s=80, zorder=5, marker='x', label='Fail')

    ax.set_title(f'{asset} Walk-Forward: {sum(passed)}/{len(passed)} pass, '
                 f'avg sharpe={sh.mean():.2f}, worst DD={dd.max()*100:.1f}%')
    ax.set_ylabel('Equity (0=start)')
    ax2.set_ylabel('Drawdown')
    ax.axhline(1.0, color='gray', lw=0.8, ls='--')
    ax.legend(loc='upper left')
    ax.grid(True, alpha=0.3)

plt.suptitle('Cross-Market Equity Walk-Forward: Turtle+Chandelier (FROZEN crypto params)\n'
             '252-bar train / 252-bar test | 0.1% taker each side', fontsize=12, y=1.01)
plt.tight_layout()
plt.savefig('charts/cross_market_equity_wf.png', dpi=150, bbox_inches='tight')
plt.close()
print('Saved charts/cross_market_equity_wf.png')

# Summary bar chart
fig2, ax = plt.subplots(figsize=(8, 5))
assets = ['SPY', 'QQQ', 'GLD']
pass_rates = [df[df['asset']==a]['pass'].mean()*100 for a in assets]
avg_sharpes = [df[df['asset']==a]['sharpe'].mean() for a in assets]
colors = ['forestgreen' if r >= 70 else 'orange' if r >= 50 else 'crimson' for r in pass_rates]
bars = ax.bar(assets, pass_rates, color=colors, edgecolor='black')
for bar, pr, sh in zip(bars, pass_rates, avg_sharpes):
    ax.text(bar.get_x() + bar.get_width()/2, bar.get_height() + 1,
            f'{pr:.0f}%\nsh={sh:.1f}', ha='center', va='bottom', fontsize=11)
ax.axhline(70, color='green', ls='--', lw=1.5, label='70% threshold')
ax.axhline(50, color='orange', ls='--', lw=1.5, label='50% threshold')
ax.set_ylabel('Pass Rate (%)')
ax.set_title('Cross-Market Equity Walk-Forward Pass Rate\nTurtle+Chandelier (frozen crypto params)')
ax.set_ylim(0, 100)
ax.legend()
ax.grid(True, axis='y', alpha=0.3)
plt.tight_layout()
plt.savefig('charts/cross_market_equity_summary.png', dpi=150, bbox_inches='tight')
plt.close()
print('Saved charts/cross_market_equity_summary.png')