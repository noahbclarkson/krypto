#!/usr/bin/env python3
"""
HEDGE_SIZE_MULT Hyperopt Comparison Chart
=========================================
Generates: /home/ubuntu/.openclaw/workspace-krypto/charts/comparison_chart.png

Data: Base5 × 7 windows (7 rows of final window-equity)
      sm_0_30, sm_0_40, sm_0_50, sm_0_55, sm_0_60 (5 M variants in equity file)
      Full sweep summary: hedge_size_mult_summary.csv (13 M values)

Shows: Baseline (0.40 current), Winner (0.55 plateau), Runner-ups (0.30, 0.50)
"""
import csv, os

CHART_DIR = '/home/ubuntu/.openclaw/workspace-krypto/charts'
SNAP_DIR  = '/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots'
OUT_PNG   = os.path.join(CHART_DIR, 'comparison_chart.png')

import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt

# ── Load aggregate summary (13 M values) ─────────────────────────────────────
summary = []
with open(os.path.join(SNAP_DIR, 'hedge_size_mult_summary.csv')) as f:
    for r in csv.DictReader(f):
        summary.append({
            'mult':    float(r['hedge_size_mult']),
            'pass':    int(r['passes']),
            'total':   int(r['total']),
            'pass_pct':float(r['pass_rate']),
            'sharpe':  float(r['avg_sharpe']),
            'ret':     float(r['avg_return']),
            'dd':      float(r['avg_dd']),
            'trades':  int(r['total_trades']),
        })

# ── Load equity (Base5, 7 windows × 5 M variants) ──────────────────────────
eq_rows = []
eq_sm_cols = []
with open(os.path.join(SNAP_DIR, 'hedge_size_mult_equity.csv')) as f:
    reader = csv.DictReader(f)
    eq_sm_cols = [h for h in reader.fieldnames if h.startswith('sm_')]
    for row in reader:
        eq_rows.append({h: float(row[h]) for h in eq_sm_cols})

windows = list(range(len(eq_rows)))   # 0..6

# ── Map M → equity cumulative curve ─────────────────────────────────────────
def build_cumulative(sm_cols, eq_rows):
    result = {}
    for col in sm_cols:
        m = float(col.replace('sm_', '').replace('_', '.'))
        running = 1.0
        curve = []
        for row in eq_rows:
            running *= row[col]
            curve.append(running)
        result[m] = curve
    return result

cumulative = build_cumulative(eq_sm_cols, eq_rows)
all_ms_in_equity = sorted(cumulative.keys())

# ── Identify key variants ────────────────────────────────────────────────────
BASELINE_M  = 0.40   # current default
WINNER_M    = 0.55   # plateau winner (93.65% pass, 92.15% return)
RUNNERUP_1  = 0.30   # best Sharpe (7.6507)
RUNNERUP_2  = 0.50   # mid-range

ms_sorted = sorted(r['mult'] for r in summary)
best_sharpe_row = max(summary, key=lambda r: r['sharpe'])
best_pass_row   = max(summary, key=lambda r: r['pass_pct'])

print("Summary:")
for r in sorted(summary, key=lambda x: x['mult']):
    print(f"  M={r['mult']:.2f}: {r['pass']}/{r['total']} pass ({r['pass_pct']:.1f}%), "
          f"Sharpe {r['sharpe']:.4f}, +{r['ret']:.1f}%, DD {r['dd']:.1f}%")

# ── Figure ──────────────────────────────────────────────────────────────────
fig, axes = plt.subplots(2, 2, figsize=(18, 13))
fig.patch.set_facecolor('#0d1117')
for ax in axes.flat:
    ax.set_facecolor('#161b22')
fig.suptitle(
    'HEDGE_SIZE_MULT Hyperopt — Position Sizing in High-BTC-Vol Regimes\n'
    'Base5 × 7 windows (252 bar OOS each) | 13 M values | Cumulative window equity',
    color='white', fontsize=13, y=0.98)

# ── Top-left: Cumulative equity (log scale) ─────────────────────────────────
ax = axes[0, 0]
highlight = [
    (BASELINE_M, '#ff7b72', f'Baseline M={BASELINE_M} (current)', 2.5),
    (WINNER_M,   '#3fb950', f'Winner M={WINNER_M} (plateau)', 2.5),
    (0.30,       '#58a6ff', f'Runner-up M=0.30 (best Sharpe)', 1.8),
    (0.50,       '#d2a8ff', f'Runner-up M=0.50 (mid-range)', 1.8),
]
for m, color, label, lw in highlight:
    if m in cumulative:
        ax.plot(windows, cumulative[m], color=color, lw=lw, label=label, marker='o', ms=6)
    else:
        print(f"WARNING: M={m} not in cumulative curves (available: {all_ms_in_equity})")

ax.set_yscale('log')
ax.set_xlabel('Walk-Forward Window', color='white', fontsize=11)
ax.set_ylabel('Cumulative Equity (log)', color='white', fontsize=11)
ax.set_title('Base5 Cumulative Equity — M Variants', color='white', fontsize=12, pad=10)
ax.tick_params(colors='white')
ax.grid(True, alpha=0.12, color='white')
ax.legend(loc='upper left', facecolor='#1c2128', labelcolor='white', fontsize=9)

# ── Top-right: Pass Rate + Sharpe vs M ──────────────────────────────────────
ax2 = axes[0, 1]
ax2b = ax2.twinx()
prs = [next(r for r in summary if r['mult'] == m)['pass_pct'] for m in ms_sorted]
shs = [next(r for r in summary if r['mult'] == m)['sharpe'] for m in ms_sorted]

ln1 = ax2.plot(ms_sorted, prs, color='#58a6ff', lw=2.0, marker='o', ms=5, label='Pass Rate %')
ln2 = ax2b.plot(ms_sorted, shs, color='#ff7b72', lw=2.0, marker='s', ms=5, label='Avg Sharpe')

ax2.axvline(BASELINE_M, color='#ff7b72', lw=1.5, ls='--', alpha=0.8, label=f'Baseline M={BASELINE_M}')
ax2.axvline(WINNER_M,   color='#3fb950', lw=1.5, ls='--', alpha=0.8, label=f'Winner M={WINNER_M}')
ax2.axhline(70, color='#f0883e', lw=1.0, ls=':', alpha=0.6, label='70% threshold')

ax2.set_xlabel('HEDGE_SIZE_MULT', color='white', fontsize=11)
ax2.set_ylabel('Pass Rate (%)', color='#58a6ff', fontsize=11)
ax2b.set_ylabel('Avg Sharpe', color='#ff7b72', fontsize=11)
ax2.tick_params(colors='white'); ax2b.tick_params(colors='white')
ax2.yaxis.label.set_color('#58a6ff'); ax2b.yaxis.label.set_color('#ff7b72')
ax2.set_title('Pass Rate and Sharpe vs HEDGE_SIZE_MULT', color='white', fontsize=12, pad=10)
ax2.grid(True, alpha=0.12, color='white')
lns = ln1 + ln2
ax2.legend(lns, [l.get_label() for l in lns], loc='lower right',
           facecolor='#1c2128', labelcolor='white', fontsize=9)

# ── Bottom-left: Return + DD vs M ───────────────────────────────────────────
ax3 = axes[1, 0]
rets = [next(r for r in summary if r['mult'] == m)['ret'] for m in ms_sorted]
dds  = [next(r for r in summary if r['mult'] == m)['dd']  for m in ms_sorted]
ax3.plot(ms_sorted, rets, color='#3fb950', lw=2.0, marker='o', ms=5, label='Avg Return %')
ax3.plot(ms_sorted, dds,  color='#f0883e', lw=2.0, marker='s', ms=5, label='Avg Max DD %')
ax3.axvline(BASELINE_M, color='#ff7b72', lw=1.5, ls='--', alpha=0.8, label=f'Baseline M={BASELINE_M}')
ax3.axvline(WINNER_M,   color='#3fb950', lw=1.5, ls='--', alpha=0.8, label=f'Winner M={WINNER_M}')
ax3.set_xlabel('HEDGE_SIZE_MULT', color='white', fontsize=11)
ax3.set_ylabel('Percent (%)', color='white', fontsize=11)
ax3.tick_params(colors='white')
ax3.set_title('Return and Max Drawdown vs HEDGE_SIZE_MULT', color='white', fontsize=12, pad=10)
ax3.grid(True, alpha=0.12, color='white')
ax3.legend(loc='upper left', facecolor='#1c2128', labelcolor='white', fontsize=9)

# ── Bottom-right: Bar chart — Sharpe per M ─────────────────────────────────
ax4 = axes[1, 1]
bar_colors = []
for m in ms_sorted:
    if abs(m - WINNER_M) < 0.01:
        bar_colors.append('#3fb950')
    elif abs(m - BASELINE_M) < 0.01:
        bar_colors.append('#ff7b72')
    elif abs(m - 0.30) < 0.01 or abs(m - 0.50) < 0.01:
        bar_colors.append('#58a6ff')
    else:
        bar_colors.append('#30363d')

ax4.bar([f'{m:.2f}' for m in ms_sorted],
        [next(r for r in summary if r['mult'] == m)['sharpe'] for m in ms_sorted],
        color=bar_colors, edgecolor='white', lw=0.3, width=0.7)
ax4.set_xlabel('HEDGE_SIZE_MULT', color='white', fontsize=11)
ax4.set_ylabel('Avg Sharpe', color='white', fontsize=11)
ax4.tick_params(colors='white', labelsize=8, axis='x', labelrotation=45)
ax4.set_title('Avg Sharpe by HEDGE_SIZE_MULT', color='white', fontsize=12, pad=10)
ax4.grid(True, axis='y', alpha=0.12, color='white')

# ── Caption ──────────────────────────────────────────────────────────────────
base = next(r for r in summary if r['mult'] == BASELINE_M)
win  = next(r for r in summary if r['mult'] == WINNER_M)
ru30 = next(r for r in summary if r['mult'] == 0.30)
ru50 = next(r for r in summary if r['mult'] == 0.50)

caption = (
    f"BASELINE M={BASELINE_M}: {base['pass']}/{base['total']} pass ({base['pass_pct']:.1f}%), "
    f"Sharpe {base['sharpe']:.4f}, +{base['ret']:.1f}%, DD {base['dd']:.1f}% | "
    f"WINNER M={WINNER_M}: {win['pass']}/{win['total']} pass ({win['pass_pct']:.1f}%), "
    f"Sharpe {win['sharpe']:.4f}, +{win['ret']:.1f}%, DD {win['dd']:.1f}% | "
    f"Runner-up M=0.30: {ru30['pass']}/{ru30['total']} pass ({ru30['pass_pct']:.1f}%), "
    f"Sharpe {ru30['sharpe']:.4f} | Runner-up M=0.50: {ru50['pass']}/{ru50['total']} pass "
    f"({ru50['pass_pct']:.1f}%), Sharpe {ru50['sharpe']:.4f} | "
    f"Best Sharpe: M={best_sharpe_row['mult']:.2f} ({best_sharpe_row['sharpe']:.4f})"
)
fig.text(0.5, 0.005, caption, ha='center', va='bottom', fontsize=9,
         color='#8b949e', style='italic')

plt.tight_layout(rect=[0, 0.05, 1, 0.96])
plt.savefig(OUT_PNG, dpi=180, bbox_inches='tight', facecolor=fig.get_facecolor())
plt.close()
print(f"\nSaved: {OUT_PNG}")
print(caption)