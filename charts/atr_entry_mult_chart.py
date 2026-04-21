#!/usr/bin/env python3
import csv, math
from collections import defaultdict
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import matplotlib.ticker as mticker

EQUITY_CSV = "/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/atr_entry_mult_full_equity.csv"
OUTPUT_PNG = "/home/ubuntu/.openclaw/workspace-krypto/krypto/charts/atr_entry_mult_comparison.png"

COMPARE_VALUES = [0.00, 0.75, 0.90, 1.00]
COMPARE_LABELS = {
    0.00: "Baseline (EM=0.00)",
    0.75: "Runner-up (EM=0.75)",
    0.90: "WINNER (EM=0.90)",
    1.00: "EM=1.00",
}
COLORS = {
    0.00: "#888888",
    0.75: "#56b4e9",
    0.90: "#e74c3c",
    1.00: "#2ecc71",
}

by_mult = defaultdict(list)
with open(EQUITY_CSV, 'r') as f:
    reader = csv.DictReader(f)
    raw = defaultdict(list)
    for row in reader:
        mult = float(row['atr_entry_mult'])
        key = (row['universe'], int(row['window']))
        raw[(mult, key)].append((int(row['step']), float(row['equity'])))

    for mult in set(k[0] for k in raw.keys()):
        curves = []
        for (m, key), bars in raw.items():
            if m == mult:
                bars_sorted = sorted(bars, key=lambda x: x[0])
                eq = [b[1] for b in bars_sorted]
                curves.append(eq)
        if curves:
            max_len = max(len(c) for c in curves)
            truncated = [c[:max_len] for c in curves]
            avg_eq = []
            for i in range(max_len):
                vals = [c[i] for c in truncated if i < len(c)]
                avg_eq.append(sum(vals) / len(vals))
            by_mult[mult] = avg_eq

def compute_dd(eq):
    peak = eq[0]
    dd = []
    for v in eq:
        peak = max(peak, v)
        dd.append((peak - v) / peak)
    return dd

fig, (ax1, ax2) = plt.subplots(2, 1, figsize=(14, 10), gridspec_kw={'height_ratios': [3, 1]})
fig.patch.set_facecolor('#0e0e0e')
for ax in (ax1, ax2):
    ax.set_facecolor('#1a1a1a')
    ax.tick_params(colors='#cccccc', labelsize=10)
    for spine in ax.spines.values():
        spine.set_color('#333333')

for mult in COMPARE_VALUES:
    if mult not in by_mult:
        continue
    eq = by_mult[mult]
    x = list(range(len(eq)))
    label = COMPARE_LABELS.get(mult, f"EM={mult:.2f}")
    color = COLORS.get(mult, '#ffffff')
    lw = 2.5 if mult == 0.90 else 1.5
    ls = '-' if mult == 0.90 else '--'
    ax1.plot(x, eq, label=label, color=color, linewidth=lw, linestyle=ls)

ax1.set_ylabel('Portfolio Equity (log scale)', color='#cccccc', fontsize=11)
ax1.set_title(
    'ATR_ENTRY_MULT Full Walk-Forward Comparison\n'
    '(9 universes × 7 windows = 63 window-runs)',
    color='#ffffff', fontsize=13, pad=10
)
ax1.set_yscale('log')
ax1.grid(True, alpha=0.15, color='#666666')
ax1.yaxis.set_major_formatter(mticker.FuncFormatter(lambda v, _: f'{v:.0f}x'))
ax1.legend(loc='upper left', fontsize=9, framealpha=0.3, labelcolor='#dddddd')
ax1.set_facecolor('#1a1a1a')
ax1.tick_params(colors='#cccccc')
for spine in ax1.spines.values():
    spine.set_color('#333333')

for mult in COMPARE_VALUES:
    if mult not in by_mult:
        continue
    eq = by_mult[mult]
    dd = compute_dd(eq)
    x = list(range(len(dd)))
    label = COMPARE_LABELS.get(mult, f"EM={mult:.2f}")
    color = COLORS.get(mult, '#ffffff')
    lw = 2.5 if mult == 0.90 else 1.5
    ls = '-' if mult == 0.90 else '--'
    ax2.plot(x, [-d * 100 for d in dd], label=label, color=color, linewidth=lw, linestyle=ls)

ax2.set_xlabel('Step (bar index)', color='#cccccc', fontsize=11)
ax2.set_ylabel('Drawdown %', color='#cccccc', fontsize=11)
ax2.set_ylim(-100, 5)
ax2.grid(True, alpha=0.15, color='#666666')
ax2.yaxis.set_major_formatter(mticker.FuncFormatter(lambda v, _: f'{v:.0f}%'))
ax2.set_facecolor('#1a1a1a')
ax2.tick_params(colors='#cccccc')
for spine in ax2.spines.values():
    spine.set_color('#333333')

summary = (
    "EM=0.90 WINNER: +29.7% Sharpe vs baseline | 100% pass (63/63) | DD=44.3% vs baseline 72.1%\n"
    "Baseline EM=0.00: Sharpe 1.95 | 96.8% pass | DD=72.1% | 4,691 trades\n"
    "EM=1.00 (prior 6-window winner): Sharpe 1.58 | -18.7% vs baseline on full 63-window validation"
)
fig.text(0.5, 0.01, summary, ha='center', fontsize=8.5, color='#aaaaaa', style='italic')
plt.tight_layout(rect=[0, 0.08, 1, 1])
plt.savefig(OUTPUT_PNG, dpi=150, bbox_inches='tight', facecolor=fig.get_facecolor())
print(f"Saved: {OUTPUT_PNG}")
