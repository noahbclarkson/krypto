#!/usr/bin/env python3
"""
SIZE_MULT Hyperopt — Comparison Chart
Generates comparison_chart.png from size_mult sweep data.
Plots equity curves (Base5) and aggregate metrics.
"""
import csv, os
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import numpy as np

CHART_DIR = os.path.dirname(os.path.abspath(__file__))
OUT_PNG = os.path.join(CHART_DIR, 'comparison_chart.png')
SNAP_DIR = os.path.join(os.path.dirname(CHART_DIR), 'snapshots')

# ── Load equity time-series ─────────────────────────────────────────────────
eq_data = {}
with open(os.path.join(SNAP_DIR, 'size_mult_equity_base5.csv')) as f:
    reader = csv.DictReader(f)
    cols = [c for c in reader.fieldnames if c.startswith('eq_')]
    for c in cols:
        eq_data[c] = []
    bars = []
    for row in reader:
        bars.append(int(row['bar']))
        for c in cols:
            eq_data[c].append(float(row[c]))

# ── Load summary ────────────────────────────────────────────────────────────
summary = []
with open(os.path.join(SNAP_DIR, 'size_mult_summary.csv')) as f:
    for r in csv.DictReader(f):
        summary.append({
            'mult': float(r['size_mult']),
            'pass': int(r['pass_count']),
            'total': int(r['total_windows']),
            'pass_pct': float(r['pass_pct']),
            'sharpe': float(r['avg_sharpe']),
            'ret': float(r['avg_return_pct']),
            'dd': float(r['avg_dd_pct']),
            'trades': int(r['total_trades']),
        })

# Key variants to highlight
BASELINE_M = 0.70
WINNER_M = 1.00   # highest pass rate
SHARPE_M = 0.40   # highest Sharpe among 54-pass group
RUNNERUP_M = 0.50 # middle ground

# ── Figure: 2x2 ─────────────────────────────────────────────────────────────
fig, axes = plt.subplots(2, 2, figsize=(16, 12))
fig.patch.set_facecolor('#0d1117')
for ax in axes.flat:
    ax.set_facecolor('#161b22')

fig.suptitle(
    'SIZE_MULT Hyperopt — High-Vol Overlay Position Sizing\n'
    'M ∈ [0.00..1.00 step 0.05] × 9 universes × 7 WF windows = 1,323 runs',
    color='white', fontsize=13, y=0.98)

# ── Top-left: Equity curves (log scale) ─────────────────────────────────────
ax = axes[0, 0]
# Plot all variants as light grey
for c in cols:
    m = float(c.replace('eq_', ''))
    if m not in [BASELINE_M, WINNER_M, SHARPE_M, RUNNERUP_M]:
        ax.plot(bars, eq_data[c], color='#30363d', lw=0.5, alpha=0.4)

# Highlight key variants
highlight = [
    (f'eq_{BASELINE_M:.2f}', '#ff7b72', f'M={BASELINE_M:.2f} (baseline)', 2.0),
    (f'eq_{WINNER_M:.2f}', '#3fb950', f'M={WINNER_M:.2f} (winner: no overlay)', 2.5),
    (f'eq_{SHARPE_M:.2f}', '#58a6ff', f'M={SHARPE_M:.2f} (best Sharpe)', 2.0),
    (f'eq_{RUNNERUP_M:.2f}', '#d2a8ff', f'M={RUNNERUP_M:.2f} (runner-up)', 1.8),
]
for col_name, color, label, lw in highlight:
    if col_name in eq_data:
        ax.plot(bars, eq_data[col_name], color=color, lw=lw, label=label)

ax.set_yscale('log')
ax.set_xlabel('Bar (daily)', color='white', fontsize=10)
ax.set_ylabel('Equity (log scale)', color='white', fontsize=10)
ax.set_title('Base5 Equity Curves — SIZE_MULT Variants', color='white', fontsize=11, pad=8)
ax.tick_params(colors='white')
ax.grid(True, alpha=0.12, color='white')
ax.legend(loc='upper left', facecolor='#1c2128', labelcolor='white', fontsize=8)

# ── Top-right: Pass rate + Sharpe vs SIZE_MULT ──────────────────────────────
ax2 = axes[0, 1]
ax2b = ax2.twinx()
ms = [r['mult'] for r in summary]
prs = [r['pass_pct'] for r in summary]
shs = [r['sharpe'] for r in summary]

ln1 = ax2.plot(ms, prs, color='#58a6ff', lw=2.0, marker='o', ms=4, label='Pass Rate')
ln2 = ax2b.plot(ms, shs, color='#ff7b72', lw=2.0, marker='s', ms=4, label='Avg Sharpe')
ax2.axvline(BASELINE_M, color='#ff7b72', lw=1.5, ls='--', alpha=0.7, label=f'Baseline M={BASELINE_M}')
ax2.axhline(70, color='#f0883e', lw=1.0, ls=':', alpha=0.6)

ax2.set_xlabel('SIZE_MULT', color='white', fontsize=10)
ax2.set_ylabel('Pass Rate (%)', color='#58a6ff', fontsize=10)
ax2b.set_ylabel('Avg Sharpe', color='#ff7b72', fontsize=10)
ax2.tick_params(colors='white'); ax2b.tick_params(colors='white')
ax2.yaxis.label.set_color('#58a6ff'); ax2b.yaxis.label.set_color('#ff7b72')
ax2.set_title('Pass Rate and Sharpe vs SIZE_MULT', color='white', fontsize=11, pad=8)
ax2.grid(True, alpha=0.12, color='white')
lns = ln1 + ln2
ax2.legend(lns, [l.get_label() for l in lns], loc='lower right',
          facecolor='#1c2128', labelcolor='white', fontsize=8)

# ── Bottom-left: Return + DD vs SIZE_MULT ──────────────────────────────────
ax3 = axes[1, 0]
rets = [r['ret'] for r in summary]
dds = [r['dd'] for r in summary]
ax3.plot(ms, rets, color='#3fb950', lw=2.0, marker='o', ms=4, label='Avg Return %')
ax3.plot(ms, dds, color='#f0883e', lw=2.0, marker='s', ms=4, label='Avg Max DD %')
ax3.axvline(BASELINE_M, color='#ff7b72', lw=1.5, ls='--', alpha=0.7, label=f'Baseline M={BASELINE_M}')
ax3.set_xlabel('SIZE_MULT', color='white', fontsize=10)
ax3.set_ylabel('Percent (%)', color='white', fontsize=10)
ax3.tick_params(colors='white')
ax3.set_title('Return and Max Drawdown vs SIZE_MULT', color='white', fontsize=11, pad=8)
ax3.grid(True, alpha=0.12, color='white')
ax3.legend(loc='upper left', facecolor='#1c2128', labelcolor='white', fontsize=8)

# ── Bottom-right: Sharpe/DD ratio (risk-adjusted efficiency) ───────────────
ax4 = axes[1, 1]
ratios = [r['sharpe'] / max(r['dd'], 0.01) for r in summary]
bar_colors = ['#3fb950' if abs(r['mult'] - WINNER_M) < 0.01 else
              '#ff7b72' if abs(r['mult'] - BASELINE_M) < 0.01 else
              '#58a6ff' if abs(r['mult'] - SHARPE_M) < 0.01 else
              '#484f58' for r in summary]
names = [f'{r["mult"]:.2f}' for r in summary]
ax4.bar(names, ratios, color=bar_colors, edgecolor='white', lw=0.3, width=0.7)
ax4.set_xlabel('SIZE_MULT', color='white', fontsize=10)
ax4.set_ylabel('Sharpe / Max DD', color='white', fontsize=10)
ax4.tick_params(colors='white', labelsize=7)
ax4.set_title('Risk-Adjusted Efficiency (Sharpe/DD ratio)', color='white', fontsize=11, pad=8)
ax4.grid(True, axis='y', alpha=0.12, color='white')

# ── Caption ──────────────────────────────────────────────────────────────────
base = next(r for r in summary if abs(r['mult'] - BASELINE_M) < 0.01)
win  = next(r for r in summary if abs(r['mult'] - WINNER_M) < 0.01)
best_s = next(r for r in summary if abs(r['mult'] - SHARPE_M) < 0.01)

caption = (
    f"WINNER M=1.00 (no overlay): {win['pass']}/{win['total']} pass ({win['pass_pct']:.1f}%), "
    f"Sharpe {win['sharpe']:.3f}, Ret {win['ret']:.1f}%, DD {win['dd']:.1f}% | "
    f"BASELINE M=0.70: {base['pass']}/{base['total']} pass ({base['pass_pct']:.1f}%), "
    f"Sharpe {base['sharpe']:.3f}, Ret {base['ret']:.1f}%, DD {base['dd']:.1f}% | "
    f"BEST SHARPE M=0.40: {best_s['pass']}/{best_s['total']} pass ({best_s['pass_pct']:.1f}%), "
    f"Sharpe {best_s['sharpe']:.3f}, Ret {best_s['ret']:.1f}%, DD {best_s['dd']:.1f}%"
)
fig.text(0.5, 0.005, caption, ha='center', va='bottom', fontsize=8,
         color='#8b949e', style='italic', wrap=True)

plt.tight_layout(rect=[0, 0.04, 1, 0.96])
plt.savefig(OUT_PNG, dpi=180, bbox_inches='tight', facecolor=fig.get_facecolor())
plt.close()
print(f"Saved: {OUT_PNG}")
print(f"\n{caption}")
