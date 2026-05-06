#!/usr/bin/env python3
"""Plot T70 FRESHNESS_COOLDOWN equity curves.

Reads absolute-path CSVs exported by examples/t70_freshness_cooldown_extensive.rs
and writes /home/ubuntu/.openclaw/workspace-krypto/charts/comparison_chart.png.
"""
from pathlib import Path
import pandas as pd
import matplotlib.pyplot as plt
from matplotlib.ticker import FuncFormatter

ROOT = Path('/home/ubuntu/.openclaw/workspace-krypto')
KRYPTO = ROOT / 'krypto'
SUMMARY = KRYPTO / 'snapshots' / 't70_freshness_cooldown_summary.csv'
EQUITY = KRYPTO / 'snapshots' / 't70_freshness_cooldown_equity.csv'
OUT = ROOT / 'charts' / 'comparison_chart.png'

summary = pd.read_csv(SUMMARY)
equity = pd.read_csv(EQUITY)

# Robustness-first: pass rate, then Sharpe, then lower DD.
baseline = 0
ranked = summary.sort_values(
    ['pass_rate_pct', 'avg_sharpe', 'avg_return_pct', 'avg_max_dd_pct'],
    ascending=[False, False, False, True],
)
selected = [baseline]
for cd in ranked['cooldown'].tolist():
    if cd not in selected:
        selected.append(int(cd))
    if len(selected) >= 4:
        break

labels = {}
for cd in selected:
    row = summary.loc[summary['cooldown'] == cd].iloc[0]
    if cd == baseline:
        prefix = 'Baseline'
    elif cd == selected[1]:
        prefix = 'Winner'
    else:
        prefix = 'Runner-up'
    labels[cd] = (
        f"{prefix} CD={cd} | pass {int(row.pass_count)}/{int(row.total_windows)} "
        f"({row.pass_rate_pct:.1f}%), Sharpe {row.avg_sharpe:.2f}"
    )

plt.style.use('seaborn-v0_8-whitegrid')
fig, ax = plt.subplots(figsize=(14, 8), dpi=160)
colors = ['#555555', '#1f77b4', '#2ca02c', '#ff7f0e']

plot_vals = []
for color, cd in zip(colors, selected):
    sub = equity[equity['cooldown'] == cd].copy().sort_values('step')
    if sub.empty:
        continue
    ax.plot(sub['step'], sub['equity'], label=labels[cd], linewidth=2.2, color=color)
    plot_vals.extend(sub['equity'].dropna().tolist())

# Dynamic Y-axis scaling: zoom to data range with padding; never force zero.
if plot_vals:
    ymin, ymax = min(plot_vals), max(plot_vals)
    pad = max((ymax - ymin) * 0.08, ymax * 0.02, 0.02)
    ax.set_ylim(max(0.001, ymin - pad), ymax + pad)

ax.set_title('T70 FRESHNESS_COOLDOWN Extensive Sweep — Base5 Full-History Equity Curves', fontsize=15, pad=14)
ax.set_xlabel('Daily step after warmup', fontsize=12)
ax.set_ylabel('Portfolio equity (× initial capital)', fontsize=12)
ax.yaxis.set_major_formatter(FuncFormatter(lambda y, _: f'{y:.2f}x'))
ax.grid(True, which='major', alpha=0.35)
ax.legend(loc='best', fontsize=9, frameon=True)

caption = (
    'Sweep: cooldown 0..100 daily bars, step 1. '
    'Ranking uses 9-universe walk-forward pass rate first, then Sharpe; curves show Base5 full-history economic equity.'
)
fig.text(0.5, 0.01, caption, ha='center', fontsize=9, color='#444444')
fig.tight_layout(rect=[0, 0.035, 1, 1])
OUT.parent.mkdir(parents=True, exist_ok=True)
fig.savefig(OUT, bbox_inches='tight')
print(f'Wrote {OUT}')
print('Selected cooldowns:', selected)
