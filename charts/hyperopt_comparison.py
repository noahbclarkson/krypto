#!/usr/bin/env python3
"""
Equity comparison chart for Turtle+Chandelier hyperopt results.
Plots time-series equity curves for winner + runner-ups + baseline.
Y-axis auto-scales (no forced 0) — curves are visually distinct.
"""
import pandas as pd
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import matplotlib.ticker as mticker

OUT_DIR = '/home/ubuntu/.openclaw/workspace-krypto/krypto/charts/'
SNAP_DIR = '/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/'

# ─────────────────────────────────────────────────────────────────────────────
# Panel A: VOL_LOOKBACK hyperopt equity curves
# Data: vol_lookback_prod_equity.csv — 5 configs: vl_8, vl_9, vl_2, vl_7, vl_78
# Summary: vol_lookback_prod_summary.csv
# ─────────────────────────────────────────────────────────────────────────────
vl_eq = pd.read_csv(f'{SNAP_DIR}vol_lookback_prod_equity.csv')
vl_sm = pd.read_csv(f'{SNAP_DIR}vol_lookback_prod_summary.csv')

# Winner: VL=8 (robustness winner, 40/54 pass, 9/9 positive)
# Runner-ups: VL=9 (42/54 tie, close), VL=2 (39/54, highest Sharpe 3.18)
# Baseline: VL=78 (old default, 34/54 pass, only 221x final equity)

VL_COLS = {
    'VL=8  WINNER (1279x, 74.1% pass)':    ('vl_8',  '#00e5ff', 2.0),
    'VL=9  runner-up (1859x, 74.1% pass)': ('vl_9',  '#ff9800', 1.5),
    'VL=2  highest Sharpe (1224x, 72.2%)': ('vl_2',  '#76ff03', 1.5),
    'VL=78 baseline (221x, 63.0% pass)':    ('vl_78', '#f44336', 1.5),
}

# ─────────────────────────────────────────────────────────────────────────────
# Panel B: POSITION_CAP hyperopt equity curves
# Data: position_cap_all_equity.csv — cap_1..cap_10 (cap_4+ go NaN after ~step 4379)
# Summary: position_cap_sweep_summary.csv
# Winner: CAP=3 (72.2% pass, 9/9 positive, avg Sharpe 4.58)
# Baseline: CAP=1 (70.4% pass, 8/9 positive, avg Sharpe 2.27)
# Runner-up: CAP=2 (68.5% pass, 8/9 positive, avg Sharpe 4.81)
# ─────────────────────────────────────────────────────────────────────────────
cap_eq = pd.read_csv(f'{SNAP_DIR}position_cap_all_equity.csv')

# cap_3 has valid data up to step 5269; cap_1, cap_2 go further
# Use all rows where cap_3 is non-NaN for a fair comparison
cap_eq = cap_eq[cap_eq['cap_3'].notna()].copy()

CAP_COLS = {
    'CAP=3  WINNER (72.2% pass, 9/9 +ve)':      ('cap_3', '#00e5ff', 2.0),
    'CAP=2  runner-up (68.5% pass, 4.81 Sharpe)': ('cap_2', '#ff9800', 1.5),
    'CAP=1  baseline (70.4% pass, 2.27 Sharpe)':  ('cap_1', '#f44336', 1.5),
}

# ─────────────────────────────────────────────────────────────────────────────
# Build figure
# ─────────────────────────────────────────────────────────────────────────────
fig, axes = plt.subplots(2, 1, figsize=(15, 11), gridspec_kw={'height_ratios': [3, 2]})
fig.patch.set_facecolor('#0d1117')
for ax in axes:
    ax.set_facecolor('#161b22')

# ── Panel A: VOL_LOOKBACK ────────────────────────────────────────────────────
ax = axes[0]
for label, (col, color, lw) in VL_COLS.items():
    y = vl_eq[col].values
    x = vl_eq['bar'].values
    ax.plot(x, y, label=label, color=color, linewidth=lw, alpha=0.92)

ax.set_yscale('log')
ax.yaxis.set_major_formatter(mticker.FuncFormatter(lambda v, _: f'{v:,.0f}x'))
ax.set_title(
    'VOL_LOOKBACK Hyperopt — Equity Curves  |  Turtle+Chandelier, 9 universes × 6 windows (2018–2026)',
    color='white', fontsize=12, pad=10
)
ax.set_ylabel('Portfolio Equity (log scale)', color='#ccd6f6', fontsize=11)
ax.tick_params(colors='#8899a6', labelsize=9)
for sp in ['top','bottom','left','right']:
    ax.spines[sp].set_color('#30363d')
ax.grid(True, alpha=0.12, color='#30363d')
ax.legend(loc='upper left', fontsize=8.5, framealpha=0.2, labelcolor='white')

# Annotate final bar
final = vl_eq.iloc[-1]
annotations = [
    ('vl_8',  'WINNER\n1279x',  '#00e5ff'),
    ('vl_9',  '1859x',         '#ff9800'),
    ('vl_2',  '1224x',         '#76ff03'),
    ('vl_78', '221x baseline', '#f44336'),
]
for col, lbl, color in annotations:
    ax.annotate(lbl, xy=(final['bar'], final[col]),
                xytext=(6, 0), textcoords='offset points',
                color=color, fontsize=8, va='center')

# ── Panel B: POSITION_CAP ────────────────────────────────────────────────────
ax = axes[1]
for label, (col, color, lw) in CAP_COLS.items():
    y = cap_eq[col].values
    x = cap_eq['step'].values
    ax.plot(x, y, label=label, color=color, linewidth=lw, alpha=0.92)

ax.set_yscale('log')
ax.yaxis.set_major_formatter(mticker.FuncFormatter(lambda v, _: f'{v:,.0f}x'))
ax.set_title(
    'POSITION_CAP Hyperopt — Equity Curves  |  Turtle-only, 9 universes × 6 windows (walk-forward aggregated)',
    color='white', fontsize=12, pad=10
)
ax.set_xlabel('Bar index (time)', color='#ccd6f6', fontsize=11)
ax.set_ylabel('Portfolio Equity (log scale)', color='#ccd6f6', fontsize=11)
ax.tick_params(colors='#8899a6', labelsize=9)
for sp in ['top','bottom','left','right']:
    ax.spines[sp].set_color('#30363d')
ax.grid(True, alpha=0.12, color='#30363d')
ax.legend(loc='upper left', fontsize=8.5, framealpha=0.2, labelcolor='white')

# Annotate final bar
final_cap = cap_eq.iloc[-1]
cap_annotations = [
    ('cap_3', 'WINNER\n4e12x',   '#00e5ff'),
    ('cap_2', '4.9e9x',         '#ff9800'),
    ('cap_1', '1.0e5x baseline', '#f44336'),
]
for col, lbl, color in cap_annotations:
    ax.annotate(lbl, xy=(final_cap['step'], final_cap[col]),
                xytext=(6, 0), textcoords='offset points',
                color=color, fontsize=8, va='center')

plt.tight_layout(pad=2.0)
out = f'{OUT_DIR}comparison_chart.png'
plt.savefig(out, dpi=150, bbox_inches='tight', facecolor=fig.get_facecolor())
print(f'Saved: {out}')
plt.close()