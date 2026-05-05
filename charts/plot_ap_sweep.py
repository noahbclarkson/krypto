#!/usr/bin/env python3
"""
AP REGIME_PERIOD Sweep — Equity Curve Comparison Chart
Reads: snapshots/ap_turtle_sweep_eq_{017,063,064,023}.csv
Output: charts/comparison_chart.png
"""
import csv
import os
import numpy as np
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import matplotlib.ticker as mticker

BASE = '/home/ubuntu/.openclaw/workspace-krypto/krypto'
OUT_DIR = os.path.join(BASE, 'charts')
os.makedirs(OUT_DIR, exist_ok=True)

def load_equity(path):
    """Load all equity curves for a given AP CSV. Returns list of (universe, window, steps, eqs)."""
    curves = {}
    with open(path) as f:
        reader = csv.DictReader(f)
        for row in reader:
            key = (row['universe'], row['window'])
            if key not in curves:
                curves[key] = []
            curves[key].append(float(row['equity']))
    # Sort by (universe, window) for consistent ordering
    return sorted(curves.items(), key=lambda x: (x[0][0], int(x[0][1])))

def aggregate_universe_equity(curves_by_uw):
    """
    Aggregate equity across windows for each universe.
    For each universe, compute a single equity time-series by averaging
    equity across all windows at each time-step.
    Returns: {universe: [avg_eq_per_step]}
    """
    by_universe = {}
    for (uni, win), eqs in curves_by_uw:
        if uni not in by_universe:
            by_universe[uni] = []
        by_universe[uni].append(eqs)

    result = {}
    for uni, list_of_curves in by_universe.items():
        max_len = max(len(c) for c in list_of_curves)
        # Pad shorter curves with last value
        padded = []
        for c in list_of_curves:
            if len(c) < max_len:
                c = c + [c[-1]] * (max_len - len(c))
            padded.append(c)
        avg = np.mean(padded, axis=0)
        result[uni] = avg.tolist()
    return result

def normalize_eq(eq_series):
    """Normalize equity so first value = 1.0"""
    if not eq_series or eq_series[0] == 0:
        return eq_series
    return [v / eq_series[0] for v in eq_series]

AP_FILES = {
    'Baseline\nAP=17':  os.path.join(BASE, 'snapshots/ap_turtle_sweep_eq_017.csv'),
    'Winner\nAP=63':    os.path.join(BASE, 'snapshots/ap_turtle_sweep_eq_063.csv'),
    'Runner-1\nAP=64': os.path.join(BASE, 'snapshots/ap_turtle_sweep_eq_064.csv'),
    'Runner-2\nAP=23': os.path.join(BASE, 'snapshots/ap_turtle_sweep_eq_023.csv'),
}

# Load and process
uni_equity = {}  # {label: {universe: normalized_avg_eq}}
all_labels = []
for label, path in AP_FILES.items():
    all_labels.append(label)
    curves_by_uw = load_equity(path)
    uni_avgs = aggregate_universe_equity(curves_by_uw)
    # Normalize each universe's avg equity
    normed = {u: normalize_eq(eq) for u, eq in uni_avgs.items()}
    uni_equity[label] = normed

# Build portfolio-level equity: mean across all universes per label
# Each label: {universe: [eq_series]} -> [portfolio_avg_series]
label_portfolio_eq = {}
for label, u_dict in uni_equity.items():
    all_unis = list(u_dict.keys())
    max_len = max(len(u_dict[u]) for u in all_unis)
    # Align all universes (pad with last value)
    aligned = []
    for u in all_unis:
        s = u_dict[u]
        if len(s) < max_len:
            s = s + [s[-1]] * (max_len - len(s))
        aligned.append(s)
    portfolio = np.mean(aligned, axis=0)
    label_portfolio_eq[label] = portfolio

# ─── PLOT ───────────────────────────────────────────────────────────────
fig, axes = plt.subplots(2, 1, figsize=(14, 10), sharex=False)

COLORS = ['#2196F3', '#FF5722', '#4CAF50', '#9C27B0']
linestyles = ['-', '--', '-.', ':']

# ── Top: Equity Curve (log scale) ───────────────────────────────────────
ax1 = axes[0]
for i, (label, eq_series) in enumerate(label_portfolio_eq.items()):
    steps = range(len(eq_series))
    ax1.plot(steps, eq_series,
             color=COLORS[i % len(COLORS)],
             linestyle=linestyles[i % len(linestyles)],
             linewidth=1.8,
             label=label,
             alpha=0.9)

ax1.set_yscale('log')
ax1.yaxis.set_major_formatter(mticker.FuncFormatter(lambda x, _: f'{x:.1f}'))
ax1.set_ylabel('Portfolio Equity (log scale)', fontsize=11)
ax1.set_title('REGIME_ATR_PERIOD Sweep — Equity Curves\n(9 universes × 7 walk-forward windows, mean)', fontsize=13, fontweight='bold')
ax1.legend(loc='upper left', fontsize=10, framealpha=0.9)
ax1.grid(True, alpha=0.3, which='both')
ax1.set_xlabel('Time-step (bar index across all windows)', fontsize=10)

# ── Bottom: Sharpe bar chart from summary ────────────────────────────────
ax2 = axes[1]
summary_path = os.path.join(BASE, 'snapshots/ap_turtle_sweep_summary.csv')
ap_sharpe = {}
with open(summary_path) as f:
    reader = csv.DictReader(f)
    for row in reader:
        ap = int(row['ap'])
        sr = float(row['avg_sharpe'])
        ap_sharpe[ap] = sr

top_aps = sorted(ap_sharpe.keys(), key=lambda a: ap_sharpe[a], reverse=True)[:20]
colors_bar = []
for ap in top_aps:
    if ap == 17:
        colors_bar.append('#2196F3')
    elif ap == 63:
        colors_bar.append('#FF5722')
    elif ap in (64, 23):
        colors_bar.append('#4CAF50')
    else:
        colors_bar.append('#90A4AE')

bars = ax2.bar([str(a) for a in top_aps], [ap_sharpe[a] for a in top_aps], color=colors_bar, alpha=0.85, edgecolor='white')
ax2.axhline(y=0, color='black', linewidth=0.5)
ax2.set_xlabel('REGIME_ATR_PERIOD (AP)', fontsize=10)
ax2.set_ylabel('Avg Walk-Forward Sharpe', fontsize=10)
ax2.set_title('Top 20 AP Values by Avg Sharpe', fontsize=13, fontweight='bold')
ax2.grid(True, alpha=0.3, axis='y')

# Annotate top 3
for i, (ap, sr) in enumerate(zip(top_aps[:3], [ap_sharpe[a] for a in top_aps[:3]])):
    ax2.annotate(f'{sr:.2f}', xy=(i, sr), xytext=(0, 5),
                 textcoords='offset points', ha='center', fontsize=8, fontweight='bold')

plt.tight_layout()
out_path = os.path.join(OUT_DIR, 'comparison_chart.png')
plt.savefig(out_path, dpi=150, bbox_inches='tight', facecolor='white')
print(f'Saved: {out_path}')

# Also save universe-level equity breakdown
fig2, axes2 = plt.subplots(3, 3, figsize=(16, 12), sharex=False, sharey=False)
fig2.suptitle('Equity by Universe — AP=17 (blue) vs AP=63 (orange) vs AP=64 (green)', fontsize=13, fontweight='bold')
axes_flat = axes2.flatten()

all_universes = sorted(list(all_labels[0] and list(uni_equity[all_labels[0]].keys()) or []))
for idx, uni in enumerate(all_universes[:9]):
    ax = axes_flat[idx]
    for li, label in enumerate([all_labels[0], all_labels[1], all_labels[2]]):
        if uni in uni_equity[label]:
            eq = uni_equity[label][uni]
            ax.plot(range(len(eq)), eq,
                    color=COLORS[li % len(COLORS)],
                    linestyle=linestyles[li % len(linestyles)],
                    linewidth=1.5,
                    label=label.replace('\n', ' '),
                    alpha=0.85)
    ax.set_yscale('log')
    ax.set_title(uni, fontsize=9, fontweight='bold')
    ax.grid(True, alpha=0.3, which='both')
    ax.legend(loc='upper left', fontsize=7)

plt.tight_layout()
out_path2 = os.path.join(OUT_DIR, 'comparison_by_universe.png')
plt.savefig(out_path2, dpi=120, bbox_inches='tight', facecolor='white')
print(f'Saved: {out_path2}')
