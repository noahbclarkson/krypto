#!/usr/bin/env python3
"""
CHAND_MULT sweep comparison chart.
Reads sweep results from snapshots/chand_mult_sweep.csv and
generates comparison_chart.png with equity-style visualization.
"""

import csv, os
import numpy as np

OUTDIR = '/home/ubuntu/.openclaw/workspace-krypto/krypto/charts'
os.makedirs(OUTDIR, exist_ok=True)

# Load sweep summary
sweep_data = []
with open('/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/chand_mult_sweep.csv') as f:
    reader = csv.DictReader(f)
    for row in reader:
        sweep_data.append({
            'M': float(row['chand_mult']),
            'return': float(row['avg_return']),
            'sharpe': float(row['avg_sharpe']),
            'dd': float(row['avg_max_dd']),
            'trades': float(row['avg_trades']),
            'wr': float(row['avg_win_rate']),
            'pass_rate': float(row['pass_rate']),
        })

# Load 9-universe validation
val_data = []
with open('/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/chand_mult_9u_validation.csv') as f:
    reader = csv.DictReader(f)
    for row in reader:
        val_data.append(row)

import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import matplotlib.ticker as mticker

# ── Chart 1: Sharpe vs CHAND_MULT (with pass rate overlay) ────────────────────
fig, axes = plt.subplots(2, 1, figsize=(14, 10))

ms = [d['M'] for d in sweep_data]
sharpes = [d['sharpe'] for d in sweep_data]
rets = [d['return'] for d in sweep_data]
dds = [d['dd'] for d in sweep_data]
pass_rates = [d['pass_rate'] for d in sweep_data]

ax = axes[0]
color_sharpe = '#2196F3'
color_pass = '#4CAF50'

ax.plot(ms, sharpes, 'o-', color=color_sharpe, linewidth=2, markersize=6, label='Avg Sharpe', zorder=3)
ax.axvline(1.50, color='red', linewidth=1.5, linestyle='--', alpha=0.7, label='Current M=1.50')
ax.axvline(2.25, color='green', linewidth=1.5, linestyle='--', alpha=0.7, label='Winner M=2.25')

# Mark the winner
winner_idx = sharpes.index(max(sharpes))
ax.plot(ms[winner_idx], sharpes[winner_idx], '*', color='gold', markersize=20, zorder=5, markeredgecolor='black')

ax.set_xlabel('CHAND_MULT', fontsize=12)
ax.set_ylabel('Average Walk-Forward Sharpe', fontsize=12, color=color_sharpe)
ax.tick_params(axis='y', labelcolor=color_sharpe)
ax.set_title('CHAND_MULT Hyperparameter Sweep — Base5 Universe (6 windows)\n'
             'Fixed: CP=15, EP=21, ATR=24, ATR_M=2.0, HM=45, CAP=3 | Step 0.25')
ax.legend(loc='lower right')
ax.grid(True, ls='--', alpha=0.4)

# Pass rate on secondary axis
ax2t = ax.twinx()
ax2t.bar(ms, pass_rates, width=0.18, color=color_pass, alpha=0.3, label='Pass Rate %')
ax2t.set_ylabel('Pass Rate %', fontsize=12, color=color_pass)
ax2t.tick_params(axis='y', labelcolor=color_pass)
ax2t.set_ylim(0, 110)

# ── Chart 2: Return and MaxDD vs CHAND_MULT ────────────────────────────────────
ax2 = axes[1]
color_ret = '#FF9800'
color_dd = '#F44336'

ax2.plot(ms, rets, 's-', color=color_ret, linewidth=2, markersize=6, label='Avg Return %')
ax2.axvline(1.50, color='red', linewidth=1.5, linestyle='--', alpha=0.7, label='Current M=1.50')
ax2.axvline(2.25, color='green', linewidth=1.5, linestyle='--', alpha=0.7, label='Winner M=2.25')
ax2.set_xlabel('CHAND_MULT', fontsize=12)
ax2.set_ylabel('Average Return %', fontsize=12, color=color_ret)
ax2.tick_params(axis='y', labelcolor=color_ret)
ax2.grid(True, ls='--', alpha=0.4)

# MaxDD on secondary axis
ax2t2 = ax2.twinx()
ax2t2.plot(ms, dds, '^-', color=color_dd, linewidth=2, markersize=6, label='Avg MaxDD %')
ax2t2.set_ylabel('Average MaxDD %', fontsize=12, color=color_dd)
ax2t2.tick_params(axis='y', labelcolor=color_dd)

# Combined legend
lines1, labels1 = ax2.get_legend_handles_labels()
lines2, labels2 = ax2t2.get_legend_handles_labels()
ax2.legend(lines1 + lines2, labels1 + labels2, loc='lower right')

plt.tight_layout()
out = f'{OUTDIR}/comparison_chart.png'
plt.savefig(out, dpi=180, bbox_inches='tight')
print(f"Saved: {out}")
plt.close()

# ── Chart 3: 9-Universe bar chart ──────────────────────────────────────────────
fig2, ax3 = plt.subplots(figsize=(14, 7))

# Group by universe, show Sharpe for each M value
m_vals = [1.50, 2.00, 2.25, 2.50, 3.00]
m_labels = ['M=1.50\n(current)', 'M=2.00', 'M=2.25\n(winner)', 'M=2.50', 'M=3.00']
universes = ['Base5', 'NoDOGE', 'Legacy4', 'Legacy5BNB', 'OldGuardNoBNB', 'LargeCaps5', 'Legacy3', 'LowVolume5', 'OldGuard4']

x = np.arange(len(universes))
width = 0.15
colors = ['#F44336', '#FF9800', '#4CAF50', '#2196F3', '#9E9E9E']

for i, (m, label) in enumerate(zip(m_vals, m_labels)):
    sharpes_u = []
    for u in universes:
        matches = [float(r['avg_sharpe']) for r in val_data if float(r['chand_mult']) == m and r['universe'] == u]
        sharpes_u.append(matches[0] if matches else 0)
    ax3.bar(x + i * width - 2*width, sharpes_u, width, label=label, color=colors[i], alpha=0.85)

ax3.set_xticks(x)
ax3.set_xticklabels(universes, rotation=30, ha='right', fontsize=9)
ax3.set_ylabel('Average Walk-Forward Sharpe')
ax3.set_title('CHAND_MULT Validation — 9 Universes × 7 Walk-Forward Windows\n'
              'M=2.25 (green) beats M=1.50 (red) in ALL major universes')
ax3.legend(loc='upper right')
ax3.axhline(0, color='black', linewidth=0.5)
ax3.grid(True, axis='y', ls='--', alpha=0.4)

plt.tight_layout()
out2 = f'{OUTDIR}/chand_mult_9u_comparison.png'
plt.savefig(out2, dpi=180, bbox_inches='tight')
print(f"Saved: {out2}")
plt.close()

print("\n=== Sweep Summary ===")
for d in sorted(sweep_data, key=lambda x: -x['sharpe'])[:5]:
    marker = " ← current" if abs(d['M'] - 1.50) < 0.01 else " ← WINNER" if d['sharpe'] == max(x['sharpe'] for x in sweep_data) else ""
    print(f"  M={d['M']:.2f}: Sharpe={d['sharpe']:.3f} Return={d['return']:.1f}% DD={d['dd']:.1f}% PassRate={d['pass_rate']:.0f}%{marker}")

print("\n=== 9-Universe Global ===")
for m in m_vals:
    matches = [float(r['avg_sharpe']) for r in val_data if float(r['chand_mult']) == m]
    passes = [r for r in val_data if float(r['chand_mult']) == m and 'PASS' in r.get('pass_rate', '')]
    total_matches = [r for r in val_data if float(r['chand_mult']) == m]
    avg_s = sum(matches) / len(matches) if matches else 0
    marker = " ← current" if abs(m - 1.50) < 0.01 else " ← WINNER" if abs(m - 2.25) < 0.01 else ""
    print(f"  M={m:.2f}: Global Avg Sharpe={avg_s:.3f}{marker}")

print("\nDone.")
