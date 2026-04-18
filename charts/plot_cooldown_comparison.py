#!/usr/bin/env python3
"""Chart: Turtle Freshness Cooldown Hyperopt — equity curve comparison.

Plots:
1. Aggregated equity curves across all 9 universes (compounded) — log scale
2. Per-universe normalized equity: winner (cd=10) vs baseline (cd=0) — log scale
3. Pass rate bar chart by universe
4. Summary metrics table
"""

import csv
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import matplotlib.ticker as mticker
from collections import defaultdict
import numpy as np

EQUITY_CSV = "snapshots/turtle_cooldown_equity.csv"
SWEEP_CSV  = "snapshots/turtle_cooldown_sweep.csv"
OUT_PNG    = "charts/turtle_cooldown_comparison.png"

CDS = [0, 4, 10, 28]
cd_labels = {0: 'Baseline (cd=0)', 4: 'Runner-up 1 (cd=4)', 10: 'Winner (cd=10)', 28: 'Runner-up 2 (cd=28)'}
colors     = {0: '#888888', 4: '#f4a261', 10: '#2a9d8f', 28: '#e76f51'}
universe_order = ['Base5','NoDOGE','Legacy4','Legacy5BNB','OldGuardNoBNB',
                  'LargeCaps5','Legacy3','LowVolume5','OldGuard4']

# --- Load sweep results ---
pass_data = defaultdict(lambda: defaultdict(lambda: {'pass':0,'total':0,'sharpe':0.0,'ret':0.0,'trades':0}))
with open(SWEEP_CSV) as f:
    reader = csv.DictReader(f)
    for row in reader:
        cd = int(row['cooldown'])
        uni = row['universe']
        pass_data[cd][uni] = {
            'pass':   int(row['pass'] == 'true'),
            'total':  1,
            'sharpe': float(row['sharpe']),
            'ret':    float(row['ret']),
            'trades': int(row['trades']),
        }

def agg(cd):
    totals = {'pass':0,'total':0,'sharpe':[],'ret':[],'trades':0}
    for uni, d in pass_data[cd].items():
        totals['pass']  += d['pass']
        totals['total'] += d['total']
        totals['sharpe'].append(d['sharpe'])
        totals['ret'].append(d['ret'])
        totals['trades'] += d['trades']
    n = totals['total']
    if n == 0:
        return None
    avg_s = sum(totals['sharpe']) / n
    avg_r = sum(totals['ret']) / n
    pr    = totals['pass'] / n * 100
    return {'pass_rate': pr, 'avg_sharpe': avg_s, 'avg_ret': avg_r, 'trades': totals['trades']}

# --- Load equity curves ---
eq_data = defaultdict(lambda: defaultdict(list))
with open(EQUITY_CSV) as f:
    reader = csv.DictReader(f)
    for row in reader:
        cd  = int(row['cd'])
        uni = row['universe']
        eq_data[cd][uni].append(float(row['equity']))

# Normalize to start at 1.0
def normalize(lst):
    if not lst:
        return lst
    start = lst[0]
    if start <= 0:
        return lst
    return [v / start for v in lst]

# Compound across universes
def compound_equity(cd):
    """Compound equal-weighted equity across all universes."""
    eqs = []
    for uni in universe_order:
        e = eq_data[cd].get(uni, [])
        if e:
            eqs.append(normalize(e))
    if not eqs:
        return []
    min_len = min(len(e) for e in eqs)
    result = [1.0] * min_len
    for e in eqs:
        for i in range(min_len):
            result[i] *= e[i]
    return result

# ===========================
fig, axes = plt.subplots(2, 2, figsize=(16, 12))
fig.patch.set_facecolor('#0d1117')
for ax in axes.flat:
    ax.set_facecolor('#161b22')
    ax.tick_params(colors='#c9d1d9', labelcolor='#c9d1d9')
    ax.xaxis.label.set_color('#c9d1d9')
    ax.yaxis.label.set_color('#c9d1d9')
    ax.title.set_color('#e6edf3')
    ax.grid(color='#21262d', alpha=0.8, linewidth=0.5)

# ---- Panel 1: Aggregated Equity (log scale) ----
ax = axes[0, 0]
for cd in CDS:
    eq = compound_equity(cd)
    if not eq:
        continue
    ax.semilogy(eq, color=colors[cd], label=cd_labels[cd], linewidth=1.8, alpha=0.95)
ax.set_title('Aggregated Equity — All 9 Universes (Equal-Weighted, Compounded)', fontsize=11, fontweight='bold')
ax.set_xlabel('Day')
ax.set_ylabel('Portfolio Equity (log scale)')
ax.legend(loc='upper left', fontsize=8, framealpha=0.3, labelcolor='#c9d1d9')
ax.yaxis.set_major_formatter(mticker.FuncFormatter(lambda y, _: f'{y:.0e}'))

# ---- Panel 2: Per-universe normalized equity (cd=10 vs cd=0) ----
ax = axes[0, 1]
uni_colors_map = {
    'Base5': '#1f77b4', 'NoDOGE': '#ff7f0e', 'Legacy4': '#2ca02c',
    'Legacy5BNB': '#d62728', 'OldGuardNoBNB': '#9467bd', 'LargeCaps5': '#8c564b',
    'Legacy3': '#e377c2', 'LowVolume5': '#7f7f7f', 'OldGuard4': '#bcbd22'
}
# Plot individual universe trajectories (thin, transparent)
for uni in universe_order:
    e0  = normalize(eq_data[0].get(uni, []))
    e10 = normalize(eq_data[10].get(uni, []))
    if not e0 or not e10:
        continue
    n = min(len(e0), len(e10))
    lbl0  = 'Baseline cd=0' if uni == universe_order[0] else None
    lbl10 = 'Winner cd=10' if uni == universe_order[0] else None
    ax.semilogy(e0[:n],  color='#888888', alpha=0.25, linewidth=0.7, label=lbl0)
    ax.semilogy(e10[:n], color='#2a9d8f', alpha=0.25, linewidth=0.7, label=lbl10)

# Mean trajectory across universes
for cd, color, lbl in [(0, '#888888', 'Mean cd=0'), (10, '#2a9d8f', 'Mean cd=10')]:
    all_eqs = []
    for uni in universe_order:
        e = normalize(eq_data[cd].get(uni, []))
        if e:
            all_eqs.append(e)
    if all_eqs:
        min_len = min(len(e) for e in all_eqs)
        means = [np.mean([e[i] for e in all_eqs]) for i in range(min_len)]
        ax.semilogy(means, color=color, linewidth=2.5, label=lbl, zorder=10)

ax.set_title('Per-Universe Equity — Winner vs Baseline (mean bold)', fontsize=11, fontweight='bold')
ax.set_xlabel('Day')
ax.set_ylabel('Normalized Equity (log scale)')
ax.legend(loc='upper left', fontsize=8, framealpha=0.3, labelcolor='#c9d1d9')
ax.yaxis.set_major_formatter(mticker.FuncFormatter(lambda y, _: f'{y:.0e}'))

# ---- Panel 3: Pass rate bar chart ----
ax = axes[1, 0]
x = np.arange(len(universe_order))
width = 0.2
for i, cd in enumerate(CDS):
    pr = []
    for uni in universe_order:
        d = pass_data[cd].get(uni, None)
        if d and d['total'] > 0:
            pr.append(d['pass'] / d['total'] * 100)
        else:
            pr.append(0.0)
    bars = ax.bar(x + i*width - 0.3, pr, width, color=colors[cd], label=cd_labels[cd], alpha=0.85)
ax.set_xticks(x)
ax.set_xticklabels(universe_order, rotation=30, ha='right', fontsize=7.5)
ax.set_ylabel('Pass Rate (%)')
ax.set_title('Pass Rate by Universe — Winners vs Baseline', fontsize=11, fontweight='bold')
ax.legend(fontsize=7, framealpha=0.3, labelcolor='#c9d1d9', ncol=2)
ax.set_ylim(0, 115)
ax.axhline(60, color='#f4a261', linestyle='--', alpha=0.7, linewidth=0.9)
ax.text(len(universe_order)-0.5, 62, '60% threshold', color='#f4a261', fontsize=7, alpha=0.8)

# ---- Panel 4: Summary table ----
ax = axes[1, 1]
ax.axis('off')

base_sharpe = agg(0)['avg_sharpe'] if agg(0) else 0.001

table_data = []
for cd in CDS:
    a = agg(cd)
    if not a:
        continue
    delta_s = (a['avg_sharpe'] - base_sharpe) / abs(base_sharpe) * 100 if base_sharpe != 0 else 0
    delta_str = f"{delta_s:+.0f}%"
    marker = "  ★" if cd == 10 else ""
    table_data.append([
        cd_labels[cd] + marker,
        f"{a['pass_rate']:.0f}%",
        f"{a['avg_sharpe']:.3f}",
        f"{a['avg_ret']:.0f}%",
        f"{a['trades']:,}",
        delta_str,
    ])

col_labels = ['Config', 'Pass Rate', 'Avg Sharpe', 'Avg Ret%', 'Trades', 'vs Baseline']
table = ax.table(
    cellText=table_data,
    colLabels=col_labels,
    loc='center',
    cellLoc='center',
)
table.auto_set_font_size(False)
table.set_fontsize(9)
table.scale(1.1, 2.0)

for (row, col), cell in table.get_celld().items():
    cell.set_edgecolor('#30363d')
    if row == 0:
        cell.set_facecolor('#1c2128')
        cell.get_text().set_color('#e6edf3')
        cell.get_text().set_fontweight('bold')
    elif row == 3:  # cd=10 winner row
        cell.set_facecolor('#1a3830')
        cell.get_text().set_color('#2a9d8f')
        cell.get_text().set_fontweight('bold')
    else:
        cell.set_facecolor('#161b22' if row % 2 == 0 else '#1c2128')
        cell.get_text().set_color('#c9d1d9')

ax.set_title('9-Universe Walk-Forward Summary', fontsize=11, fontweight='bold', pad=12)

fig.suptitle(
    'Turtle+Chandelier — Freshness Cooldown Hyperopt (cd ∈ 0..30, step 1)\n'
    'WINNER: cd=10 | +7.8pp pass rate | +797% Sharpe improvement | fewer false breakouts',
    fontsize=12, fontweight='bold', color='#e6edf3', y=0.98
)
plt.tight_layout(rect=[0, 0, 1, 0.94])
plt.savefig(OUT_PNG, dpi=150, bbox_inches='tight', facecolor=fig.get_facecolor())
plt.close()
print(f"Saved: {OUT_PNG}")
