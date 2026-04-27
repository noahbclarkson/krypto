#!/usr/bin/env python3
"""
CHAND_PERIOD Fine Sweep — Equity Curve Comparison Chart
Plots Baseline (P=7), Winner (P=5 by pass rate), and runner-ups.
Data: snapshots/chand_period_fine_equity.csv
"""

import pandas as pd
import matplotlib.pyplot as plt
import matplotlib.ticker as ticker
import numpy as np

# Load equity data
df = pd.read_csv('snapshots/chand_period_fine_equity.csv')
print(f"Loaded {len(df)} rows: {df['chand_period'].unique()}")

# Aggregate: compound equity across all universes per day and period
# For each period, average the equity curves across all windows/universes that contributed to each day
# But to keep it simple and comparable: use the last value per day per period (the "global" accumulated equity)

# Better approach: for each period, take the MAXIMUM equity across all universes per day
# (representing the "best case" spread of what the strategy could do)
# OR: aggregate by taking the mean of window-equity at each day index

# Actually for a cleaner comparison: compute a "representative equity" per P by taking
# the equity of Base5 (our production universe) per day. That's the most relevant signal.
base5 = df[df['universe'] == 'Base5'].copy()
print(f"Base5 rows: {len(base5)}")
print(f"Base5 periods: {sorted(base5['chand_period'].unique())}")

# For Base5, compound across windows (each window's equity is already standalone)
# Group by day and period — average the equity across windows for that day
base5_agg = base5.groupby(['chand_period', 'day'])['equity'].mean().reset_index()
print(f"Aggregated Base5 rows: {len(base5_agg)}")

# Define candidates to plot
candidates = {
    7: {'label': 'P=7 (Baseline)', 'color': '#2196F3', 'style': '-'},
    5: {'label': 'P=5 (Best Pass 75.6%)', 'color': '#4CAF50', 'style': '-'},
    10: {'label': 'P=10 (Runner-up)', 'color': '#FF9800', 'style': '--'},
    11: {'label': 'P=11 (2nd Runner-up)', 'color': '#9C27B0', 'style': '-.'},
}

fig, axes = plt.subplots(1, 2, figsize=(16, 7), dpi=120)

# ── Plot 1: Base5 equity curves (log scale) ──────────────────────────────────
ax = axes[0]
for p in [7, 5, 10, 11]:
    subset = base5_agg[base5_agg['chand_period'] == p].sort_values('day')
    ax.plot(subset['day'], subset['equity'],
            label=candidates[p]['label'],
            color=candidates[p]['color'],
            linestyle=candidates[p]['style'],
            linewidth=1.8)

ax.set_yscale('log')
ax.set_xlabel('Days', fontsize=11)
ax.set_ylabel('Portfolio Equity (log scale)', fontsize=11)
ax.set_title('Base5 Universe — CHAND_PERIOD Sweep\n(EP=21, CM=2.30, HM=12)', fontsize=12)
ax.legend(fontsize=9)
ax.grid(True, alpha=0.3)
ax.yaxis.set_major_formatter(ticker.FuncFormatter(lambda x, _: f'{x:.1f}x' if x < 100 else f'{x:.0f}x'))

# ── Plot 2: Global (all-universe aggregated) equity curves ─────────────────────
global_agg = df.groupby(['chand_period', 'day'])['equity'].mean().reset_index()
ax2 = axes[1]
for p in [7, 5, 10, 11]:
    subset = global_agg[global_agg['chand_period'] == p].sort_values('day')
    ax2.plot(subset['day'], subset['equity'],
             label=candidates[p]['label'],
             color=candidates[p]['color'],
             linestyle=candidates[p]['style'],
             linewidth=1.8)

ax2.set_yscale('log')
ax2.set_xlabel('Days', fontsize=11)
ax2.set_ylabel('Portfolio Equity (log scale)', fontsize=11)
ax2.set_title('Global (All 9 Universes) — CHAND_PERIOD Sweep\n(Averaged across universes)', fontsize=12)
ax2.legend(fontsize=9)
ax2.grid(True, alpha=0.3)
ax2.yaxis.set_major_formatter(ticker.FuncFormatter(lambda x, _: f'{x:.1f}x' if x < 100 else f'{x:.0f}x'))

plt.tight_layout()
plt.savefig('charts/chand_period_fine_comparison.png', dpi=120, bbox_inches='tight')
print("Saved: charts/chand_period_fine_comparison.png")
plt.close()

# ── Summary bar chart ─────────────────────────────────────────────────────────
import json
with open('snapshots/chand_period_fine_summary.json') as f:
    summary = json.load(f)

fig2, axes2 = plt.subplots(1, 3, figsize=(16, 5), dpi=120)

ps = [r['chand_period'] for r in summary['results']]
pass_rates = [r['pass_rate'] for r in summary['results']]
sharpes = [r['avg_sharpe'] for r in summary['results']]
rets = [r['avg_ret'] for r in summary['results']]

colors = ['#4CAF50' if r['chand_period'] == 5 else '#2196F3' if r['chand_period'] == 7 else '#90A4AE' for r in summary['results']]

axes2[0].bar(ps, pass_rates, color=colors, edgecolor='black', linewidth=0.5)
axes2[0].set_xlabel('CHAND_PERIOD')
axes2[0].set_ylabel('Pass Rate (%)')
axes2[0].set_title('Pass Rate by P Value')
axes2[0].axhline(y=70, color='red', linestyle='--', alpha=0.6, label='70% threshold')
axes2[0].legend()

axes2[1].bar(ps, sharpes, color=colors, edgecolor='black', linewidth=0.5)
axes2[1].set_xlabel('CHAND_PERIOD')
axes2[1].set_ylabel('Avg Sharpe')
axes2[1].set_title('Avg Sharpe by P Value')

axes2[2].bar(ps, rets, color=colors, edgecolor='black', linewidth=0.5)
axes2[2].set_xlabel('CHAND_PERIOD')
axes2[2].set_ylabel('Avg Return (%)')
axes2[2].set_title('Avg Return by P Value')

plt.suptitle('CHAND_PERIOD Fine Sweep: P ∈ [5..15] step 1\n(11 values × 9 universes × 10 windows = 990 runs)', fontsize=12)
plt.tight_layout()
plt.savefig('charts/chand_period_fine_bars.png', dpi=120, bbox_inches='tight')
print("Saved: charts/chand_period_fine_bars.png")
plt.close()

print("\nSummary:")
print(f"  Baseline P=7: pass={summary['results'][2]['pass_rate']:.1f}%, Sharpe={summary['results'][2]['avg_sharpe']:.3f}")
print(f"  Winner P=5:  pass={summary['results'][0]['pass_rate']:.1f}%, Sharpe={summary['results'][0]['avg_sharpe']:.3f}")
print(f"  Runner-up P=10: pass={summary['results'][5]['pass_rate']:.1f}%, Sharpe={summary['results'][5]['avg_sharpe']:.3f}")
print(f"\nVerdict: P=5 is best pass rate (75.6%) but Sharpe worse than baseline.")
print(f"P=7 (baseline) remains the best choice — no parameter change warranted.")