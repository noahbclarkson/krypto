#!/usr/bin/env python3
"""
FRESHNESS_COOLDOWN Comparison Chart
Reads T70 equity curves and plots FC=0 vs FC=53 vs FC=55 vs FC=75 comparison.
"""

import csv
import matplotlib.pyplot as plt
import numpy as np

# Read equity curves from T70
equities = {}
with open('/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/t70_freshness_cooldown_equity.csv', 'r') as f:
    reader = csv.DictReader(f)
    for row in reader:
        fc = int(row['cooldown'])
        step = int(row['step'])
        equity = float(row['equity'])
        
        if fc not in equities:
            equities[fc] = []
        # Ensure we keep track properly (step should match index)
        if len(equities[fc]) == step:
            equities[fc].append(equity)
        elif len(equities[fc]) < step:
            # Fill gaps if any
            while len(equities[fc]) < step:
                equities[fc].append(equities[fc][-1] if equities[fc] else 1.0)
            equities[fc].append(equity)

# Select key values to plot
plot_fcs = [0, 25, 53, 55, 75, 100]
colors = {
    0: '#1f77b4',    # blue - baseline
    25: '#2ca02c',   # green - moderate
    53: '#ff7f0e',  # orange - winner plateau
    55: '#d62728',  # red - high plateau
    75: '#9467bd',  # purple - high cooldown
    100: '#8c564b'  # brown - max cooldown
}

labels = {
    0: 'FC=0 (baseline)',
    25: 'FC=25',
    53: 'FC=53 (plateau winner)',
    55: 'FC=55',
    75: 'FC=75',
    100: 'FC=100'
}

# Create chart
fig, (ax1, ax2) = plt.subplots(2, 1, figsize=(14, 10))

# Plot equity curves (log scale for visibility)
for fc in plot_fcs:
    if fc in equities and len(equities[fc]) > 0:
        eq = np.array(equities[fc])
        # Use log scale for equity to see differences
        eq_log = np.log10(eq + 0.001)  # Add small value to avoid log(0)
        x = np.arange(len(eq))
        ax1.plot(x, eq_log, label=labels[fc], color=colors[fc], linewidth=1.5, alpha=0.8)

ax1.set_xlabel('Days', fontsize=12)
ax1.set_ylabel('Log10(Equity)', fontsize=12)
ax1.set_title('T70: FRESHNESS_COOLDOWN Equity Curves (Stale Params: HM=12, HSM=0.40)', fontsize=14, fontweight='bold')
ax1.legend(loc='upper left', fontsize=9)
ax1.grid(True, alpha=0.3)

# Summary bar chart
fcs = [0, 25, 50, 53, 55, 58, 75, 100]
pass_rates = [78.3, 83.3, 81.7, 96.7, 95.0, 96.7, 68.3, 80.0]
sharpes = [1.294, 1.217, 0.890, 1.389, 1.482, 1.409, 0.956, 0.822]
trades = [3118, 1481, 1071, 1020, 1005, 995, 870, 714]

x_pos = np.arange(len(fcs))
width = 0.25

bars = ax2.bar(x_pos, pass_rates, width, label='Pass Rate %', color='#1f77b4', alpha=0.8)
ax2.set_ylabel('Pass Rate (%)', fontsize=12)
ax2.set_xlabel('FRESHNESS_COOLDOWN', fontsize=12)
ax2.set_title('T70 Summary: Pass Rate by Cooldown Value', fontsize=14, fontweight='bold')
ax2.set_xticks(x_pos)
ax2.set_xticklabels([f'FC={f}' for f in fcs])
ax2.axhline(y=78.3, color='red', linestyle='--', alpha=0.5, label='Baseline FC=0')
ax2.axhline(y=95, color='green', linestyle='--', alpha=0.5, label='95% threshold')
ax2.legend(loc='upper right')
ax2.grid(True, alpha=0.3, axis='y')

# Add text annotation for the key finding
ax2.annotate('Robustness\nplateau: FC=53-58', 
            xy=(3, 96.7), xytext=(4.5, 92),
            fontsize=10, fontweight='bold',
            arrowprops=dict(arrowstyle='->', color='gray'),
            bbox=dict(boxstyle='round', facecolor='wheat', alpha=0.5))

plt.tight_layout()
plt.savefig('/home/ubuntu/.openclaw/workspace-krypto/charts/comparison_chart.png', dpi=150, bbox_inches='tight')
print("Chart saved to /home/ubuntu/.openclaw/workspace-krypto/charts/comparison_chart.png")

# Print key metrics
print("\n" + "=" * 60)
print("CHART SUMMARY")
print("=" * 60)
print("Top-Left: Equity curves (log scale) for FC values")
print("Top-Right: Pass rate bar chart showing FC=53-58 plateau")
print("\nKey finding: FC=53-58 shows 95-97% pass rate vs baseline 78%")
print("Note: This is with stale params. Exact-live verification needed.")