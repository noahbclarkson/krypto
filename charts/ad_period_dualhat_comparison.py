#!/usr/bin/env python3
"""
A/D Dual-Hat AD_PERIOD Hyperopt Comparison Chart
Charts equity curves for:
  - Winner: AD_PERIOD=1 (71.9% pass, Sharpe 3.353)
  - Runner-up #1: AD_PERIOD=8 (56.6% pass, Sharpe 3.872)
  - Runner-up #2: AD_PERIOD=40 (53.6% pass, Sharpe 2.804)
  - Baseline: AD_PERIOD=5 (52% pass, old default)
"""
import pandas as pd
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import matplotlib.ticker as mticker
import numpy as np

CSV = "snapshots/ad_period_dualhat_equity.csv"
OUT  = "charts/ad_period_dualhat_comparison.png"

# Load equity data
df = pd.read_csv(CSV)
print(f"Loaded {len(df):,} rows, columns: {list(df.columns)}")
print(f"AD_PERIODs in data: {sorted(df['ad_period'].unique())}")
print(f"Universes in data: {sorted(df['universe'].unique())}")

# ── Figure setup ────────────────────────────────────────────────────────────
fig, axes = plt.subplots(2, 1, figsize=(14, 10), facecolor='#0d1117')
fig.subplots_adjust(hspace=0.35)

# Style
TEXT_COLOR = '#c9d1d9'
GRID_COLOR = '#21262d'
AXIS_BG    = '#161b22'
for ax in axes:
    ax.set_facecolor(AXIS_BG)
    ax.tick_params(colors=TEXT_COLOR, labelsize=9)
    ax.xaxis.label.set_color(TEXT_COLOR)
    ax.yaxis.label.set_color(TEXT_COLOR)
    ax.title.set_color(TEXT_COLOR)
    ax.spines['top'].set_color(GRID_COLOR)
    ax.spines['bottom'].set_color(GRID_COLOR)
    ax.spines['left'].set_color(GRID_COLOR)
    ax.spines['right'].set_color(GRID_COLOR)
    ax.grid(True, color=GRID_COLOR, linewidth=0.5, alpha=0.7)

# ── Colors & labels ─────────────────────────────────────────────────────────
COLORS = {
    1:  '#58a6ff',   # Winner — bright blue
    8:  '#3fb950',   # Runner-up 1 — green
    40: '#f78166',   # Runner-up 2 — orange
    5:  '#8b949e',   # Baseline — grey
}
AD_LABELS = {
    1:  'AD_P=1  [WINNER]  (71.9% pass, Sharpe 3.35)',
    8:  'AD_P=8  [Runner-1] (56.6% pass, Sharpe 3.87)',
    40: 'AD_P=40 [Runner-2] (53.6% pass, Sharpe 2.80)',
    5:  'AD_P=5  [Baseline] (52.0% pass, old default)',
}

UNIVERSE_GROUPS = [
    ('Base5',     ['Base5']),
    ('GlobalAvg', None),   # None = all universes
]
UNIVERSE_NAMES = ['Base5 (Production)', 'Global Average (9 Universes)']

for axi, (group_name, universes) in enumerate(UNIVERSE_GROUPS):
    ax = axes[axi]
    ax.set_title(f'A/D Dual-Hat — Equity Curves: {UNIVERSE_NAMES[axi]}', fontsize=13, fontweight='bold', pad=10)

    if universes is None:
        # Global: average across ALL universes per step
        grouped = df.groupby(['ad_period', 'step'])['equity'].mean().reset_index()
    else:
        # Per-universe: filter, then average across windows
        sub = df[df['universe'].isin(universes)]
        grouped = sub.groupby(['ad_period', 'step'])['equity'].mean().reset_index()

    for ad_p in [1, 8, 40, 5]:
        curve = grouped[grouped['ad_period'] == ad_p].sort_values('step')
        if curve.empty:
            print(f"  WARNING: No data for AD_PERIOD={ad_p} in group={group_name}")
            continue
        steps = curve['step'].values
        equity = curve['equity'].values

        # Use log scale — equity compounds multiplicatively
        ax.semilogy(steps, equity,
                    color=COLORS[ad_p],
                    linewidth=1.8 if ad_p == 1 else 1.3,
                    alpha=0.95 if ad_p == 1 else 0.85,
                    label=AD_LABELS[ad_p])

        # Annotate final value
        final_eq = equity[-1]
        ax.annotate(f'{final_eq:.1f}x',
                    xy=(steps[-1], final_eq),
                    xytext=(5, 0), textcoords='offset points',
                    color=COLORS[ad_p], fontsize=8,
                    va='center')

    ax.axhline(y=1.0, color='#484f58', linewidth=0.8, linestyle='--', alpha=0.6)
    ax.set_xlabel('Walk-Forward Step (bar index)', fontsize=10)
    ax.set_ylabel('Portfolio Equity (× initial)', fontsize=10)
    ax.legend(loc='upper left', fontsize=8.5, framealpha=0.3,
              facecolor=AXIS_BG, edgecolor=GRID_COLOR, labelcolor=TEXT_COLOR)
    ax.xaxis.set_major_formatter(mticker.FuncFormatter(lambda x, _: f'{int(x):d}'))
    ax.yaxis.set_major_formatter(mticker.FuncFormatter(lambda x, _: f'{x:.0f}x' if x >= 1 else f'{x:.2f}'))

# ── Summary metrics table ──────────────────────────────────────────────────
# Read sweep summary
sweep = pd.read_csv("snapshots/ad_period_dualhat_sweep.csv")
print("\nSweep CSV columns:", list(sweep.columns))

# Aggregate per AD_PERIOD across universes
if 'avg_sharpe' in sweep.columns:
    metrics = sweep.groupby('ad_period').agg(
        pass_rate=('n_pass', lambda x: x.sum()),
        total_n=('n_total', lambda x: x.sum()),
        avg_ret=('avg_ret_pct', 'mean'),
        avg_sharpe=('avg_sharpe', 'mean'),
        avg_dd=('avg_dd_pct', 'mean'),
        total_trades=('total_trades', 'sum'),
    ).reset_index()
    metrics['pass_pct'] = metrics['pass_rate'] / metrics['total_n'] * 100

# Bottom panel: pass rate bar chart
ax3 = fig.add_axes([0.12, 0.01, 0.78, 0.14])
ax3.set_facecolor(AXIS_BG)
ax3.spines['top'].set_color(GRID_COLOR)
ax3.spines['bottom'].set_color(GRID_COLOR)
ax3.spines['left'].set_color(GRID_COLOR)
ax3.spines['right'].set_color(GRID_COLOR)
ax3.tick_params(colors=TEXT_COLOR, labelsize=8)
ax3.set_title('9-Universe Pass Rate by AD_PERIOD', fontsize=10, fontweight='bold', pad=6)
ax3.set_ylabel('Pass Rate (%)', fontsize=9)
ax3.set_xlabel('AD_PERIOD', fontsize=9)
ax3.grid(True, color=GRID_COLOR, linewidth=0.5, alpha=0.5)

# Coarse sweep results (Base5 only for bar chart)
coarse_ad = [1,2,3,4,5,6,7,8,9,10,12,15,18,20,25,30,35,40,45,50]
coarse_pass = {
    1:9, 2:8, 3:8, 4:6, 5:5, 6:6, 7:8, 8:7, 9:5, 10:5,
    12:8, 15:7, 18:9, 20:6, 25:7, 30:5, 35:6, 40:5, 45:8, 50:6
}
coarse_windows = {
    1:10, 2:10, 3:10, 4:10, 5:7, 6:10, 7:10, 8:10, 9:8, 10:7,
    12:10, 15:10, 18:10, 20:10, 25:10, 30:7, 35:8, 40:7, 45:10, 50:7
}

bar_colors = ['#58a6ff' if p in [1,8,40,5] else '#30363d' for p in coarse_ad]
bar_pass = [coarse_pass.get(p, 0)/coarse_windows.get(p,1)*100 for p in coarse_ad]

bars = ax3.bar(coarse_ad, bar_pass, color=bar_colors, width=1.8, edgecolor='none', alpha=0.9)
ax3.axhline(y=60, color='#f85149', linewidth=1.2, linestyle='--', alpha=0.8, label='60% threshold')
ax3.set_xticks(coarse_ad)
ax3.set_xticklabels([str(p) for p in coarse_ad], fontsize=7, rotation=45)
ax3.set_ylim(0, 110)
ax3.yaxis.set_major_formatter(mticker.FuncFormatter(lambda x, _: f'{x:.0f}%'))
ax3.legend(fontsize=8, framealpha=0.3, facecolor=AXIS_BG, edgecolor=GRID_COLOR, labelcolor=TEXT_COLOR)

# Annotate winners
for p, pct in zip([1, 8, 40, 5], [coarse_pass[1]/coarse_windows[1]*100,
                                     coarse_pass[8]/coarse_windows[8]*100,
                                     coarse_pass[40]/coarse_windows[40]*100,
                                     coarse_pass[5]/coarse_windows[5]*100]):
    ax3.annotate(f'AD={p}',
                 xy=(p, pct + 2), ha='center', color=COLORS[p],
                 fontsize=7, fontweight='bold')

fig.savefig(OUT, dpi=150, bbox_inches='tight', facecolor='#0d1117', edgecolor='none')
print(f"\nSaved: {OUT}")
plt.close(fig)
