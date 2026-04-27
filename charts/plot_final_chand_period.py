#!/usr/bin/env python3
import pandas as pd
import matplotlib.pyplot as plt
import matplotlib.ticker as ticker
import json
import os

os.chdir('/home/ubuntu/.openclaw/workspace-krypto/krypto')

equity_df = pd.read_csv('snapshots/chand_period_fine_equity.csv')
with open('snapshots/chand_period_fine_summary.json') as f:
    summary_json = json.load(f)

results = summary_json['results']

candidates = {
    7:  {'label': 'P=7 Baseline', 'color': '#1565C0', 'style': '-'},
    10: {'label': 'P=10 Winner (pass 77.8%)', 'color': '#2E7D32', 'style': '--'},
    5:  {'label': 'P=5 Runner-up (Sharpe 38.7)', 'color': '#E65100', 'style': ':'},
}

base5_eq = equity_df[equity_df['universe'] == 'Base5']
base5_agg = base5_eq.groupby(['chand_period', 'day'])['equity'].mean().reset_index()
global_agg = equity_df.groupby(['chand_period', 'day'])['equity'].mean().reset_index()

fig, axes = plt.subplots(1, 2, figsize=(18, 7), dpi=130)

ax = axes[0]
for p in [7, 10, 5]:
    subset = base5_agg[base5_agg['chand_period'] == p].sort_values('day')
    ax.plot(subset['day'], subset['equity'],
            label=candidates[p]['label'],
            color=candidates[p]['color'],
            linestyle=candidates[p]['style'],
            linewidth=2.0, alpha=0.95)
ax.set_yscale('log')
ax.set_xlabel('Days', fontsize=12)
ax.set_ylabel('Portfolio Equity (log scale)', fontsize=12)
ax.set_title('Base5 Universe (Production)\nCHAND_PERIOD Comparison — Equity Curves', fontsize=13, fontweight='bold')
ax.legend(fontsize=10, loc='upper left')
ax.grid(True, alpha=0.3, which='both')
ax.yaxis.set_major_formatter(ticker.FuncFormatter(lambda x, _: f'{x:.0f}x' if x >= 1 else f'{x:.2f}'))

ax2 = axes[1]
for p in [7, 10, 5]:
    subset = global_agg[global_agg['chand_period'] == p].sort_values('day')
    ax2.plot(subset['day'], subset['equity'],
             label=candidates[p]['label'],
             color=candidates[p]['color'],
             linestyle=candidates[p]['style'],
             linewidth=2.0, alpha=0.95)
ax2.set_yscale('log')
ax2.set_xlabel('Days', fontsize=12)
ax2.set_ylabel('Portfolio Equity (log scale)', fontsize=12)
ax2.set_title('Global (All 9 Universes)\nCHAND_PERIOD Comparison — Equity Curves', fontsize=13, fontweight='bold')
ax2.legend(fontsize=10, loc='upper left')
ax2.grid(True, alpha=0.3, which='both')
ax2.yaxis.set_major_formatter(ticker.FuncFormatter(lambda x, _: f'{x:.0f}x' if x >= 1 else f'{x:.2f}'))

plt.suptitle(
    'CHAND_PERIOD Fine Sweep: P in [5..15] step 1 -- 990 walk-forward runs\n'
    'Winner: P=10 (pass rate 77.8%) | Baseline: P=7 | Runner-up: P=5 (Sharpe 38.7)',
    fontsize=12, fontweight='bold'
)
plt.tight_layout(rect=[0, 0, 1, 0.92])
plt.savefig('charts/comparison_chart.png', dpi=130, bbox_inches='tight')
plt.close()
print("Saved: charts/comparison_chart.png")

# Bar chart
fig2, (ax3, ax4) = plt.subplots(1, 2, figsize=(16, 6), dpi=120)
ps = [r['chand_period'] for r in results]
pass_rates = [r['pass_rate'] for r in results]
sharpes = [r['avg_sharpe'] for r in results]

colors_bar = []
for r in results:
    p = r['chand_period']
    if p == 10: colors_bar.append('#2E7D32')
    elif p == 7: colors_bar.append('#1565C0')
    elif p == 5: colors_bar.append('#E65100')
    else: colors_bar.append('#90A4AE')

ax3.bar(ps, pass_rates, color=colors_bar, edgecolor='black', linewidth=0.5, width=0.7)
ax3.axhline(y=70, color='red', linestyle='--', alpha=0.7, linewidth=1.2, label='70% threshold')
ax3.set_xlabel('CHAND_PERIOD', fontsize=11)
ax3.set_ylabel('Pass Rate (%)', fontsize=11)
ax3.set_title('Pass Rate by CHAND_PERIOD', fontsize=12, fontweight='bold')
ax3.set_xticks(ps)
ax3.set_ylim(0, 90)
ax3.legend(fontsize=9)
ax3.grid(True, alpha=0.3, axis='y')
for bar, pr in zip(ax3.patches, pass_rates):
    ax3.text(bar.get_x() + bar.get_width()/2, bar.get_height() + 0.5,
             f'{pr:.1f}%', ha='center', va='bottom', fontsize=7.5, color='black')

sh_baseline = next(r['avg_sharpe'] for r in results if r['chand_period'] == 7)
ax4.bar(ps, sharpes, color=colors_bar, edgecolor='black', linewidth=0.5, width=0.7)
ax4.axhline(y=sh_baseline, color='#1565C0', linestyle=':', alpha=0.7, linewidth=1.2, label=f'P=7 baseline ({sh_baseline:.1f})')
ax4.set_xlabel('CHAND_PERIOD', fontsize=11)
ax4.set_ylabel('Avg Sharpe Ratio', fontsize=11)
ax4.set_title('Avg Sharpe by CHAND_PERIOD', fontsize=12, fontweight='bold')
ax4.set_xticks(ps)
ax4.legend(fontsize=9)
ax4.grid(True, alpha=0.3, axis='y')

plt.suptitle(
    'CHAND_PERIOD Fine Sweep: P in [5..15] step 1 (990 runs)\n'
    'Verdict: P=7 (baseline) confirmed -- no parameter change warranted',
    fontsize=11
)
plt.tight_layout(rect=[0, 0, 1, 0.92])
plt.savefig('charts/comparison_chart_bars.png', dpi=120, bbox_inches='tight')
plt.close()
print("Saved: charts/comparison_chart_bars.png")

print("\nFinal Results:")
for r in results:
    p = r['chand_period']
    note = ""
    if p == 10: note = "WINNER (pass rate)"
    elif p == 7: note = "BASELINE"
    elif p == 5: note = "Runner-up (Sharpe)"
    print(f"  P={p:2d}: pass={r['pass_rate']:.1f}%, Sharpe={r['avg_sharpe']:.3f}, Ret={r['avg_ret']:.0f}%  [{note}]")
print("\nNO parameter change warranted.")