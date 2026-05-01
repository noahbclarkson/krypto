#!/usr/bin/env python3
"""
VOL_LOOKBACK Hyperopt: Equity Curve Comparison Chart
Generates comparison_chart.png from hyperopt equity CSVs.
"""
import csv
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import matplotlib.ticker as mticker
import sys
import os

os.chdir('/home/ubuntu/.openclaw/workspace-krypto/krypto')

def read_equity(path):
    bars = []
    equity = []
    try:
        with open(path) as f:
            reader = csv.DictReader(f)
            for row in reader:
                bars.append(int(row['bar']))
                equity.append(float(row['equity']))
        return bars, equity
    except (FileNotFoundError, KeyError):
        return None, None

configs = [
    ('snapshots/vl_hyperopt_live_compat_equity_baseline.csv', 'VL=8 (baseline)', '#888888', '--'),
    ('snapshots/vl_hyperopt_live_compat_equity_winner.csv', 'VL=5 (winner)', '#00C853', '-'),
    ('snapshots/vl_hyperopt_live_compat_equity_runner1_sharpe.csv', 'VL=95 (runner-up Sharpe)', '#2979FF', '-.'),
    ('snapshots/vl_hyperopt_live_compat_equity_runner2_sharpe.csv', 'VL=85 (runner-up 2)', '#FF6D00', ':'),
]

fig, (ax1, ax2) = plt.subplots(2, 1, figsize=(14, 10), gridspec_kw={'height_ratios': [3, 1]})
fig.patch.set_facecolor('#0D1117')
ax1.set_facecolor('#161B22')
ax2.set_facecolor('#161B22')

for path, label, color, ls in configs:
    bars, equity = read_equity(path)
    if bars is None:
        print(f"WARNING: {path} not found or empty")
        continue
    ax1.plot(bars, equity, label=label, color=color, linewidth=1.8, linestyle=ls, alpha=0.9)

ax1.set_title(
    'VOL_LOOKBACK Hyperopt — Equity Curves\n'
    'Live-Compatible Harness: Turtle-only + ATR_rank(12,42,5.0) + USDT Hedge | 9 Universes × 7 WF Windows',
    color='#E6EDF3', fontsize=13, pad=12
)
ax1.set_ylabel('Equity (× starting capital)', color='#E6EDF3', fontsize=11)
ax1.tick_params(colors='#8B949E', labelsize=9)
ax1.yaxis.set_major_formatter(mticker.FormatStrFormatter('%.1f×'))
ax1.grid(True, alpha=0.15, color='#30363D', linestyle='-')
ax1.legend(loc='upper left', fontsize=9, facecolor='#21262D', edgecolor='#30363D', labelcolor='#E6EDF3')
ax1.set_xlim(left=0)

# Add horizontal line at 1.0x
ax1.axhline(y=1.0, color='#F85149', linewidth=1.0, linestyle=':', alpha=0.6)

# ── Pass-rate bar chart ─────────────────────────────────────────────────────────
sweep_path = 'snapshots/vl_hyperopt_live_compat_summary.csv'
vl_vals = []
pass_pcts = []
avg_sharpes = []

try:
    with open(sweep_path) as f:
        reader = csv.DictReader(f)
        for row in reader:
            vl_vals.append(int(row['vl']))
            pass_pcts.append(float(row['pass_pct']))
            avg_sharpes.append(float(row['avg_sharpe']))
except Exception as e:
    print(f"WARNING: could not read {sweep_path}: {e}")
    sys.exit(1)

# Color bars: winner=red, baseline=gray, others=blue
colors = []
for vl in vl_vals:
    if vl == 5:
        colors.append('#00C853')
    elif vl == 8:
        colors.append('#888888')
    elif vl in (90, 95):
        colors.append('#2979FF')
    else:
        colors.append('#30363D')

bars = ax2.bar(vl_vals, pass_pcts, color=colors, width=4, alpha=0.85, edgecolor='none')
ax2.set_xlabel('VOL_LOOKBACK', color='#E6EDF3', fontsize=11)
ax2.set_ylabel('Pass Rate (%)', color='#E6EDF3', fontsize=11)
ax2.tick_params(colors='#8B949E', labelsize=9)
ax2.set_xlim(left=0, right=205)
ax2.set_ylim(bottom=70, top=90)
ax2.yaxis.set_major_formatter(mticker.FormatStrFormatter('%.0f%%'))
ax2.grid(True, alpha=0.15, color='#30363D', axis='y')

# Annotate winner
ax2.annotate('VL=5\n85.7%', xy=(5, 85.7), xytext=(20, 87.5),
             color='#00C853', fontsize=8,
             arrowprops=dict(arrowstyle='->', color='#00C853', lw=1.2))
ax2.annotate('VL=8\n82.5%', xy=(8, 82.5), xytext=(22, 84),
             color='#888888', fontsize=8,
             arrowprops=dict(arrowstyle='->', color='#888888', lw=1.2))

plt.tight_layout(pad=2.0)
out_path = 'charts/comparison_chart.png'
plt.savefig(out_path, dpi=150, bbox_inches='tight', facecolor='#0D1117')
plt.close()
print(f"Saved: {out_path}")
