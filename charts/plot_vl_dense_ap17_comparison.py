#!/usr/bin/env python3
"""Plot VOL_LOOKBACK dense AP17 hyperopt comparison.

Reads:
  snapshots/vl_dense_ap17_summary.csv
  snapshots/vl_dense_ap17_equity_{baseline,winner,runner1_sharpe,runner2_sharpe}.csv
Writes:
  charts/comparison_chart.png
"""
import os
import pandas as pd
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt

os.chdir('/home/ubuntu/.openclaw/workspace-krypto/krypto')
OUT = 'charts/comparison_chart.png'

summary = pd.read_csv('snapshots/vl_dense_ap17_summary.csv')
# Robust ranking used by harness: pass rate first, then Sharpe.
ranked = summary.sort_values(['pass_pct', 'avg_sharpe'], ascending=[False, False]).reset_index(drop=True)
baseline_vl = 8
winner_vl = int(ranked.iloc[0]['vl'])
runner1_vl = int(ranked.iloc[1]['vl'])
runner2_vl = int(ranked.iloc[2]['vl'])

curves = {
    f'VL={baseline_vl} baseline': pd.read_csv('snapshots/vl_dense_ap17_equity_baseline.csv'),
    f'VL={winner_vl} winner': pd.read_csv('snapshots/vl_dense_ap17_equity_winner.csv'),
    f'VL={runner1_vl} runner-up': pd.read_csv('snapshots/vl_dense_ap17_equity_runner1_sharpe.csv'),
    f'VL={runner2_vl} runner-up': pd.read_csv('snapshots/vl_dense_ap17_equity_runner2_sharpe.csv'),
}

colors = ['#1f77b4', '#2ca02c', '#ff7f0e', '#9467bd']
plt.style.use('seaborn-v0_8-whitegrid')
fig, axes = plt.subplots(2, 2, figsize=(16, 11))
fig.suptitle('VOL_LOOKBACK Dense Hyperopt — Live Turtle Path (AP17, T5)\nRange: VL=1..200 step 1; 9 universes × 7 walk-forward windows',
             fontsize=14, fontweight='bold')

# A: equity curves — actual required line chart
ax = axes[0, 0]
for (label, df), color in zip(curves.items(), colors):
    ax.plot(df['bar'], df['equity'], label=label, color=color, linewidth=2.0)
ax.set_xlabel('Walk-forward test step')
ax.set_ylabel('Mean normalized equity')
ax.set_title('A. Equity curves: baseline vs winner and runner-ups')
ax.grid(True, alpha=0.35)
ax.legend(fontsize=9)
# Dynamic y-axis: no forced zero baseline
all_y = pd.concat([df['equity'] for df in curves.values()])
pad = (all_y.max() - all_y.min()) * 0.12
ax.set_ylim(max(0.01, all_y.min() - pad), all_y.max() + pad)

# B: Sharpe landscape
ax = axes[0, 1]
s = summary.sort_values('vl')
ax.plot(s['vl'], s['avg_sharpe'], color='#1976D2', linewidth=1.8)
ax.scatter(s['vl'], s['avg_sharpe'], color='#1976D2', s=10, alpha=0.7)
for vl, color, name in [(baseline_vl, colors[0], 'baseline'), (winner_vl, colors[1], 'winner'), (runner1_vl, colors[2], 'runner1'), (runner2_vl, colors[3], 'runner2')]:
    ax.axvline(vl, color=color, linestyle='--', linewidth=1.1, alpha=0.75, label=f'{name} VL={vl}')
ax.set_xlabel('VOL_LOOKBACK')
ax.set_ylabel('Avg walk-forward Sharpe')
ax.set_title('B. Sharpe across full integer range')
ax.grid(True, alpha=0.35)
ax.legend(fontsize=8)

# C: pass rate landscape
ax = axes[1, 0]
ax.plot(s['vl'], s['pass_pct'], color='#388E3C', linewidth=1.8)
ax.scatter(s['vl'], s['pass_pct'], color='#388E3C', s=10, alpha=0.7)
for vl, color in [(baseline_vl, colors[0]), (winner_vl, colors[1]), (runner1_vl, colors[2]), (runner2_vl, colors[3])]:
    ax.axvline(vl, color=color, linestyle='--', linewidth=1.1, alpha=0.75)
ax.set_xlabel('VOL_LOOKBACK')
ax.set_ylabel('Pass rate (%)')
ax.set_title('C. Robustness/pass rate across VL')
ax.grid(True, alpha=0.35)

# D: top table as bar chart (metrics context, not substitute for equity chart)
ax = axes[1, 1]
top = ranked.head(12).copy()
ax.bar(top['vl'].astype(str), top['avg_sharpe'], color=['#2ca02c'] + ['#90A4AE'] * (len(top)-1))
ax.set_xlabel('Top VOL_LOOKBACK values by robust ranking')
ax.set_ylabel('Avg Sharpe')
ax.set_title('D. Top robust values (pass rate first, Sharpe second)')
ax.tick_params(axis='x', rotation=45)
ax.grid(True, axis='y', alpha=0.35)

plt.tight_layout(rect=[0, 0, 1, 0.94])
os.makedirs('charts', exist_ok=True)
plt.savefig(OUT, dpi=160, bbox_inches='tight')
print(f'Saved {OUT}')
print('Winner:', winner_vl)
print(ranked.head(5)[['vl','pass_pct','avg_sharpe','avg_ret_pct','avg_dd_pct','total_trades','avg_win_rate']].to_string(index=False))
