#!/usr/bin/env python3
"""Plot hedge overlay parameter sweep results.
Generates equity curve comparison chart from snapshots/hedge_overlay_equity_curves.csv
and a heatmap from snapshots/hedge_overlay_sweep.csv.
"""
import csv
import os
import numpy as np
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import matplotlib.colors as mcolors

OUTDIR = '/home/ubuntu/.openclaw/workspace-krypto/krypto/charts'
os.makedirs(OUTDIR, exist_ok=True)

# ── 1. Equity curve comparison ──────────────────────────────────────────────
equity_data = {}  # label -> list of (bar, equity)
with open('snapshots/hedge_overlay_equity_curves.csv') as f:
    reader = csv.DictReader(f)
    for row in reader:
        label = row['label']
        bar = int(row['bar'])
        equity = float(row['equity'])
        if label not in equity_data:
            equity_data[label] = []
        equity_data[label].append((bar, equity))

# Sort by final equity to determine legend order
label_order = ['BASELINE (0.75/252)', 'WINNER', 'RUNNER-UP-1', 'RUNNER-UP-2']
palette = {
    'BASELINE (0.75/252)': '#aaaaaa',
    'WINNER':              '#00cc44',
    'RUNNER-UP-1':         '#4488ff',
    'RUNNER-UP-2':         '#ff8844',
}

fig, (ax1, ax2) = plt.subplots(1, 2, figsize=(16, 7))

# Equity (log scale)
for label in label_order:
    if label in equity_data:
        bars, eqs = zip(*equity_data[label])
        ax1.semilogy(list(bars), list(eqs), 'o-', linewidth=2, markersize=5,
                     color=palette[label], label=label)

ax1.set_xlabel('Walk-Forward Window', fontsize=11)
ax1.set_ylabel('Portfolio Equity (log scale)', fontsize=11)
ax1.set_title('Hedge Overlay Sweep — Equity Curves\n(7 Walk-Forward Windows, Base5 Universe)', fontsize=12)
ax1.legend(fontsize=10)
ax1.grid(True, which='both', alpha=0.3)
ax1.set_xticks(range(7))

# Annotate final values
for label in label_order:
    if label in equity_data:
        final_eq = equity_data[label][-1][1]
        ax1.annotate(f'{label}: {final_eq:.1f}x',
                    xy=(6, final_eq),
                    fontsize=8, color=palette[label])

# ── 2. Heatmap: Sharpe by pct × lookback ──────────────────────────────────────
pct_labels = []
lb_labels = []
sharpe_matrix = []

seen_pct = set()
with open('snapshots/hedge_overlay_sweep.csv') as f:
    reader = csv.DictReader(f)
    for row in reader:
        pct = row['hedge_pct']
        lb  = row['hedge_lookback']
        sh  = float(row['avg_sharpe'])
        if pct not in seen_pct:
            seen_pct.add(pct)
            pct_labels.append(pct)
        if lb not in lb_labels:
            lb_labels.append(lb)
        while len(sharpe_matrix) < int(float(pct) * 20):  # rough index
            sharpe_matrix.append([])
        # matrix is built row-major: index = pct_idx * n_lb + lb_idx
        pass

pct_order = set()
lb_order  = set()
with open('snapshots/hedge_overlay_sweep.csv') as f:
    reader = csv.DictReader(f)
    for row in reader:
        pct_order.add(float(row['hedge_pct']))
        lb_order.add(int(row['hedge_lookback']))

pct_vals = sorted(pct_order)
lb_vals  = sorted(lb_order)
sh_map = {}
with open('snapshots/hedge_overlay_sweep.csv') as f:
    reader = csv.DictReader(f)
    for row in reader:
        pct = float(row['hedge_pct'])
        lb  = int(row['hedge_lookback'])
        sh  = float(row['avg_sharpe'])
        sh_map[(pct, lb)] = sh

# Build matrix [pct][lb]
matrix = np.zeros((len(pct_vals), len(lb_vals)))
for i, pct in enumerate(pct_vals):
    for j, lb in enumerate(lb_vals):
        matrix[i, j] = sh_map.get((pct, lb), np.nan)

# Mask out crazy values (LB=504 causes numerical issues)
mask = np.abs(matrix) > 1e10
masked_matrix = np.ma.masked_array(matrix, mask=mask)

im = ax2.imshow(masked_matrix, cmap='YlGn', aspect='auto')
ax2.set_xticks(range(len(lb_vals)))
ax2.set_xticklabels([str(l) for l in lb_vals], fontsize=9)
ax2.set_yticks(range(len(pct_vals)))
ax2.set_yticklabels([f'{p*100:.0f}%' for p in pct_vals], fontsize=9)
ax2.set_xlabel('Hedge Lookback (bars)', fontsize=11)
ax2.set_ylabel('Hedge Percentile Threshold', fontsize=11)
ax2.set_title('Avg Sharpe by Hedge Params\n(50 configs × 9 universes × 7 windows)', fontsize=12)

# Annotate cells
for i in range(len(pct_vals)):
    for j in range(len(lb_vals)):
        val = matrix[i, j]
        if abs(val) < 1e10:
            color = 'white' if val < 4 or val > 6 else 'black'
            ax2.text(j, i, f'{val:.2f}', ha='center', va='center',
                     fontsize=7, color=color)

# Highlight baseline cell (0.75, 252) and winner (0.55, 252)
for i, pct in enumerate(pct_vals):
    for j, lb in enumerate(lb_vals):
        if abs(matrix[i,j]) > 1e10:
            continue
        if pct == 0.75 and lb == 252:
            rect = plt.Rectangle((j-0.5, i-0.5), 1, 1, fill=False,
                                  edgecolor='white', linewidth=2)
            ax2.add_patch(rect)
        if pct == 0.55 and lb == 252:
            rect = plt.Rectangle((j-0.5, i-0.5), 1, 1, fill=False,
                                  edgecolor='#00cc44', linewidth=2)
            ax2.add_patch(rect)

plt.colorbar(im, ax=ax2, label='Avg Sharpe')

fig.suptitle('T68 Hedge Overlay Hyperopt — pct ∈ {50..95%} × lb ∈ {63..504}', fontsize=13)
plt.tight_layout()
out_path = os.path.join(OUTDIR, 'comparison_chart.png')
plt.savefig(out_path, dpi=150, bbox_inches='tight')
print(f'Saved: {out_path}')
plt.close()
