#!/usr/bin/env python3
"""
HEDGE_LOOKBACK comparison chart - baseline (252) vs winner (147) vs alternatives
"""
import pandas as pd
import matplotlib.pyplot as plt
import sys

# Load T84 summary data
df = pd.read_csv('/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/t84_hedge_lookback_summary.csv')
df.columns = df.columns.str.strip()

# Create comparison bar chart
fig, axes = plt.subplots(1, 3, figsize=(15, 5))
fig.suptitle('HEDGE_LOOKBACK Sweep: Current (252) vs Winner (147)', fontsize=14, fontweight='bold')

# Subplot 1: Pass Rate
ax1 = axes[0]
colors = ['#2ecc71' if x == 147 else '#e74c3c' if x == 252 else '#3498db' for x in df['hedge_lb']]
ax1.bar(df['hedge_lb'], df['pass_pct'], color=colors)
ax1.axhline(y=90, color='orange', linestyle='--', alpha=0.7, label='90% threshold')
ax1.set_xlabel('HEDGE_LOOKBACK')
ax1.set_ylabel('Pass Rate (%)')
ax1.set_title('Pass Rate by Lookback')
ax1.legend()
ax1.grid(True, alpha=0.3)

# Subplot 2: Sharpe
ax2 = axes[1]
colors = ['#2ecc71' if x == 147 else '#e74c3c' if x == 252 else '#3498db' for x in df['hedge_lb']]
ax2.bar(df['hedge_lb'], df['avg_sharpe'], color=colors)
ax2.set_xlabel('HEDGE_LOOKBACK')
ax2.set_ylabel('Average Sharpe')
ax2.set_title('Sharpe by Lookback')
ax2.grid(True, alpha=0.3)

# Subplot 3: Max Drawdown
ax3 = axes[2]
colors = ['#2ecc71' if x == 147 else '#e74c3c' if x == 252 else '#3498db' for x in df['hedge_lb']]
ax3.bar(df['hedge_lb'], df['avg_dd_pct'], color=colors)
ax3.set_xlabel('HEDGE_LOOKBACK')
ax3.set_ylabel('Avg Max Drawdown (%)')
ax3.set_title('Max Drawdown by Lookback')
ax3.grid(True, alpha=0.3)

plt.tight_layout()
plt.savefig('/home/ubuntu/.openclaw/workspace-krypto/charts/comparison_chart.png', dpi=150, bbox_inches='tight')
print("Chart: /home/ubuntu/.openclaw/workspace-krypto/charts/comparison_chart.png")

# Print key comparison
current = df[df['hedge_lb'] == 252].iloc[0]
winner = df[df['hedge_lb'] == 147].iloc[0]
print(f"\n=== HEDGE_LOOKBACK Comparison ===")
print(f"Current (252): Pass={current['pass_pct']:.1f}%, Sharpe={current['avg_sharpe']:.3f}, DD={current['avg_dd_pct']:.1f}%")
print(f"Winner (147):  Pass={winner['pass_pct']:.1f}%, Sharpe={winner['avg_sharpe']:.3f}, DD={winner['avg_dd_pct']:.1f}%")
print(f"Improvement: Sharpe +{winner['avg_sharpe']-current['avg_sharpe']:.3f}, DD {current['avg_dd_pct']-winner['avg_dd_pct']:.1f}pp better")