#!/usr/bin/env python3
"""
Plot A/D Chandelier Hyperopt — comparison of P × M grid for A/D Dual-Hat strategy.
Reads: snapshots/ad_chandelier_sweep.csv
Output: charts/ad_chandelier_hyperopt.png (equity comparison chart per the task brief)

Task brief rule: "Plot the Baseline, the Winner, and top Runner-ups as distinct
colored lines on the same graph."
Baseline = legacy defaults (P=45, M=2.5)
Winner   = best by global avg Sharpe (P=15, M=2.0, already in production)
Runner-ups = P=15,M=2.0 alternative close configs

Chart MUST be a line graph plotting equity over time — NOT text/stats.
Dynamic Y-axis scaling (log scale), legend, axis labels, title, grid.
"""

import pandas as pd
import numpy as np
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import matplotlib.ticker as mticker
from pathlib import Path
import warnings
warnings.filterwarnings('ignore')
import os

try:
    BASE = Path(__file__).parent.parent
except NameError:
    BASE = Path(os.environ.get('KRYPTO_BASE', '/home/ubuntu/.openclaw/workspace-krypto/krypto'))

OUT = BASE / "charts" / "ad_chandelier_hyperopt.png"

# ── 1. Load sweep results ──────────────────────────────────────────────────────
df = pd.read_csv(BASE / "snapshots/ad_chandelier_sweep.csv")
EQUITY_CSV = BASE / "snapshots" / "ad_chandelier_equity.csv"

# ── 2. Identify configs ────────────────────────────────────────────────────────
BASELINE_P, BASELINE_M = 45, 2.5

# Global average per (P, M)
grp = df.groupby(['chand_period','chand_mult']).agg(
    avg_sharpe=('sharpe','mean'),
    pass_pct=('pass','mean'),
    total_trades=('trades','sum'),
    windows=('window','count')
).reset_index()

# Winner: best Sharpe globally (P=15, M=2.0 — already production)
winner = grp.sort_values('avg_sharpe', ascending=False).iloc[0]
WINNER_P, WINNER_M = int(winner['chand_period']), float(winner['chand_mult'])

print(f"BASELINE: P={BASELINE_P}, M={BASELINE_M}")
print(f"WINNER:   P={WINNER_P}, M={WINNER_M} (sh={winner['avg_sharpe']:.4f}, pass={winner['pass_pct']:.1%})")

# Runner-ups: same P=15, top 3 M values around winner
p15 = grp[grp['chand_period'] == WINNER_P].sort_values('avg_sharpe', ascending=False)
runners = p15.iloc[1:4]  # next 3 best M values
print(f"RUNNER-UPS (P={WINNER_P} vary M):")
print(runners[['chand_mult','avg_sharpe','pass_pct']].to_string(index=False))

CONFIGS = [
    {'p': BASELINE_P, 'm': BASELINE_M, 'label': f'Baseline P={BASELINE_P},M={BASELINE_M}', 'style': '--', 'color': '#888888', 'lw': 1.5},
    {'p': WINNER_P,   'm': WINNER_M,   'label': f'Winner P={WINNER_P},M={WINNER_M} (sh={winner["avg_sharpe"]:.3f})',  'style': '-',  'color': '#2196F3', 'lw': 2.5},
]

for _, r in runners.iterrows():
    CONFIGS.append({
        'p': WINNER_P, 'm': float(r['chand_mult']),
        'label': f'Runner-up P={WINNER_P},M={r["chand_mult"]} (sh={r["avg_sharpe"]:.3f})',
        'style': '-', 'color': None, 'lw': 1.5
    })

# Assign distinct colors to runners
RUNNER_COLORS = ['#FF5722', '#4CAF50', '#9C27B0']
for i, cfg in enumerate(CONFIGS):
    if cfg['color'] is None:
        cfg['color'] = RUNNER_COLORS[i % len(RUNNER_COLORS)]

# ── 3. Load equity CSV ─────────────────────────────────────────────────────────
eq_df = pd.read_csv(EQUITY_CSV)
print(f"\nEquity CSV: {len(eq_df)} rows, universes={eq_df['universe'].unique()}")
print(f"Columns: {eq_df.columns.tolist()}")

# ── 4. Build per-window geometric-mean equity curve ───────────────────────────
def geometric_mean_equity(sub_df):
    """Aggregate equity across windows using geometric mean per bar step."""
    windows = sorted(sub_df['window'].unique())
    steps = sorted(sub_df['step'].unique())

    # Renumber steps sequentially (0, 1, 2, ...) for consistent x-axis
    step_map = {s: i for i, s in enumerate(steps)}
    sub_df = sub_df.copy()
    sub_df['bar'] = sub_df['step'].map(step_map)
    max_bar = len(steps) - 1

    geomean_by_bar = []
    for bar_idx in range(max_bar + 1):
        vals = []
        for w in windows:
            w_df = sub_df[(sub_df['window'] == w) & (sub_df['bar'] == bar_idx)]
            if not w_df.empty:
                v = w_df['equity'].values[0]
                if v > 0:
                    vals.append(v)
        if vals:
            geomean_by_bar.append(np.exp(np.mean(np.log(np.clip(np.array(vals, dtype=np.float64), 1e-10, 1e10)))))
        else:
            geomean_by_bar.append(np.nan)
    return np.arange(len(geomean_by_bar)), np.array(geomean_by_bar)

# ── 5. Main chart ─────────────────────────────────────────────────────────────
fig, axes = plt.subplots(2, 2, figsize=(16, 12))
fig.suptitle(
    f"A/D Chandelier Hyperopt: P × M Grid (91 configs) × 9 Universes\n"
    f"BASELINE: P=45,M=2.5 (legacy) | WINNER: P={WINNER_P},M={WINNER_M} | RUNNER-UPS: P={WINNER_P} top-M",
    fontsize=13, fontweight='bold'
)

# Panel 1: Equity curves (log scale) — baseline vs winner vs runners
ax1 = axes[0, 0]
for cfg in CONFIGS:
    mask = (eq_df['chand_period'] == cfg['p']) & (eq_df['chand_mult'] == cfg['m'])
    sub = eq_df[mask]
    if sub.empty:
        print(f"  WARNING: no equity data for P={cfg['p']}, M={cfg['m']}")
        continue
    bars, geq = geometric_mean_equity(sub)
    ax1.plot(bars, geq,
            linestyle=cfg['style'], color=cfg['color'],
            linewidth=cfg['lw'], label=cfg['label'])

ax1.set_yscale('log')
ax1.set_xlabel("Bar (test window)")
ax1.set_ylabel("Normalized Equity (log scale)")
ax1.set_title("Equity Curve: Geometric Mean Across All Windows (9 Universes)")
ax1.grid(True, alpha=0.3, which='both')
ax1.legend(fontsize=8)
ax1.set_ylim(bottom=0.1)

# Panel 2: Sharpe heatmap of full grid (P × M)
ax2 = axes[0, 1]
pivot_sharpe = grp.pivot(index='chand_mult', columns='chand_period', values='avg_sharpe')
# sort periods
pivot_sharpe = pivot_sharpe.reindex(sorted(pivot_sharpe.columns), axis=1)
im = ax2.imshow(pivot_sharpe.values, aspect='auto', cmap='RdYlGn', origin='lower')
plt.colorbar(im, ax=ax2, label='Avg Sharpe')
ax2.set_xlabel('CHAND_PERIOD')
ax2.set_ylabel('CHAND_MULT')
ax2.set_title('Sharpe Heatmap: CHAND_PERIOD × CHAND_MULT')
# Tick labels
periods = [int(c) for c in pivot_sharpe.columns]
mults   = [float(r) for r in pivot_sharpe.index]
ax2.set_xticks(np.arange(len(periods)))
ax2.set_xticklabels(periods, fontsize=8)
ax2.set_yticks(np.arange(len(mults)))
ax2.set_yticklabels([f'{m:.1f}' for m in mults], fontsize=8)
# Annotate winner cell
wp_col = list(periods).index(WINNER_P)
wm_row = list(mults).index(WINNER_M)
ax2.scatter(wp_col, wm_row, s=200, marker='*', color='white', zorder=10, label=f'Winner P={WINNER_P},M={WINNER_M}')
ax2.legend(fontsize=8)

# Panel 3: Bar chart — baseline vs winner vs runners (global Sharpe + pass rate)
ax3 = axes[1, 0]
labels_s = [f"{cfg['label'].split(' (')[0]}" for cfg in CONFIGS]
sharpes_s = []
pass_s    = []
for cfg in CONFIGS:
    row = grp[(grp['chand_period'] == cfg['p']) & (grp['chand_mult'] == cfg['m'])]
    if not row.empty:
        sharpes_s.append(float(row['avg_sharpe'].iloc[0]))
        pass_s.append(float(row['pass_pct'].iloc[0]) * 100)
    else:
        sharpes_s.append(0.0)
        pass_s.append(0.0)

x = np.arange(len(labels_s))
colors_b = [cfg['color'] for cfg in CONFIGS]
bars_s = ax3.bar(x, sharpes_s, color=colors_b, alpha=0.85, width=0.6)
ax3.set_xticks(x)
ax3.set_xticklabels([l.replace('Baseline ', 'B: ').replace('Winner ', 'W: ').replace('Runner-up ', 'R: ')
                     for l in labels_s], rotation=15, ha='right', fontsize=8)
ax3.set_ylabel('Avg OOS Sharpe')
ax3.set_title('Sharpe Comparison: Baseline vs Winner vs Runner-ups')
ax3.grid(True, alpha=0.3, axis='y')
for bar, sh in zip(bars_s, sharpes_s):
    ax3.text(bar.get_x() + bar.get_width()/2, bar.get_height() + 0.05,
            f'{sh:.3f}', ha='center', va='bottom', fontsize=8)

# Panel 4: Per-universe Sharpe for winner vs baseline
ax4 = axes[1, 1]
universe_sharpe = df.groupby(['universe','chand_period','chand_mult'])['sharpe'].mean().reset_index()

uni_order = sorted(df['universe'].unique())
width = 0.35
x_pos = np.arange(len(uni_order))

base_sharpes = []
for u in uni_order:
    r = universe_sharpe[(universe_sharpe['universe']==u) & (universe_sharpe['chand_period']==BASELINE_P) & (universe_sharpe['chand_mult']==BASELINE_M)]
    base_sharpes.append(float(r['sharpe'].iloc[0]) if not r.empty else 0.0)

win_sharpes = []
for u in uni_order:
    r = universe_sharpe[(universe_sharpe['universe']==u) & (universe_sharpe['chand_period']==WINNER_P) & (universe_sharpe['chand_mult']==WINNER_M)]
    win_sharpes.append(float(r['sharpe'].iloc[0]) if not r.empty else 0.0)

ax4.bar(x_pos - width/2, base_sharpes, width, color='#888888', alpha=0.7, label=f'Baseline P={BASELINE_P},M={BASELINE_M}')
ax4.bar(x_pos + width/2, win_sharpes,  width, color='#2196F3', alpha=0.7, label=f'Winner P={WINNER_P},M={WINNER_M}')
ax4.set_xticks(x_pos)
ax4.set_xticklabels(uni_order, rotation=30, ha='right', fontsize=8)
ax4.set_ylabel('Avg OOS Sharpe per Universe')
ax4.set_title('Per-Universe Sharpe: Baseline vs Winner')
ax4.grid(True, alpha=0.3, axis='y')
ax4.legend(fontsize=8)

plt.tight_layout(rect=[0, 0, 1, 0.95])
plt.savefig(OUT, dpi=150, bbox_inches='tight', facecolor='white')
print(f"\nSaved: {OUT}")
plt.close()
