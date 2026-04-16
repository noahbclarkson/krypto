import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import matplotlib.ticker as mticker
import pandas as pd
import numpy as np
import os

CHART_DIR = 'charts'
os.makedirs(CHART_DIR, exist_ok=True)

PERIODS = [50, 60, 55, 45]
COLORS  = ['#888888', '#1f77b4', '#ff7f0e', '#2ca02c']
LS = [':', '-', '--', '--']
LW = [1.5, 3.0, 1.5, 1.5]

# ─── Equity Comparison ───────────────────────────────────────────────────────
eq_file = 'snapshots/dynamic_trend_equity_comparison.csv'
if os.path.exists(eq_file):
    df_eq = pd.read_csv(eq_file, index_col=0)
    fig, (ax1, ax2) = plt.subplots(2, 1, figsize=(16, 10), gridspec_kw={'height_ratios': [3, 1]})
    t1 = 'Winner: ef=60  |  Baseline: ef=50'
    fig.suptitle('DynamicTrend EMA Fast Period - Equity Comparison\n' + t1 + '\n        fontsize=13, fontweight=bold')
    fig.subplots_adjust(top=0.88)

    for col in df_eq.columns:
        ef = int(col.replace('ef_', ''))
        vals = df_eq[col].dropna().values.astype(float)
        if len(vals) == 0 or vals[-1] < 0.001:
            continue
        idx = PERIODS.index(ef) if ef in PERIODS else -1
        color = COLORS[idx] if idx >= 0 else '#888888'
        ls = LS[idx] if idx >= 0 else '-'
        lw = LW[idx] if idx >= 0 else 1.5
        label = 'ef=' + str(ef)
        ax1.plot(np.arange(len(vals)), vals, label=label, color=color, linestyle=ls, linewidth=lw)

    ax1.set_yscale('log')
    all_vals = pd.concat([df_eq[c].dropna() for c in df_eq.columns]).values
    y_min = max(all_vals.min() * 0.8, 0.01)
    y_max = all_vals.max() * 1.2
    ax1.set_ylim(y_min, y_max)
    ax1.yaxis.set_major_formatter(mticker.FuncFormatter(lambda x, _: '{:.2f}'.format(x)))
    ax1.set_ylabel('Equity (log scale)', fontsize=11)
    ax1.legend(loc='upper left', fontsize=9, framealpha=0.9)
    ax1.grid(True, alpha=0.3, which='both')
    ax1.set_title('DynamicTrend - Top EMA Fast Periods Equity Curves', fontsize=10)

    for col in df_eq.columns:
        ef = int(col.replace('ef_', ''))
        vals = df_eq[col].dropna().values.astype(float)
        if len(vals) == 0:
            continue
        peak = np.maximum.accumulate(vals)
        dd = (vals - peak) / peak * 100.0
        idx = PERIODS.index(ef) if ef in PERIODS else -1
        color = COLORS[idx] if idx >= 0 else '#888888'
        ls = LS[idx] if idx >= 0 else '-'
        lw = LW[idx] if idx >= 0 else 1.5
        ax2.plot(np.arange(len(dd)), dd, color=color, linestyle=ls, linewidth=lw)

    ax2.set_ylabel('Drawdown pct', fontsize=11)
    ax2.set_xlabel('Trading Day', fontsize=11)
    ax2.grid(True, alpha=0.3)
    ax2.set_ylim(bottom=-100)

    plt.tight_layout()
    out = os.path.join(CHART_DIR, 'dynamic_trend_comparison.png')
    fig.savefig(out, dpi=150, bbox_inches='tight')
    plt.close(fig)
    print('Saved:', out)
else:
    print('WARNING: equity CSV not found', file=sys.stderr)

# ─── Sweep Overview ───────────────────────────────────────────────────────────
smry_file = 'snapshots/dynamic_trend_ema_fast_global.csv'
if os.path.exists(smry_file):
    smry = pd.read_csv(smry_file).sort_values('ema_fast')
    fig2, axes = plt.subplots(1, 3, figsize=(22, 7))
    t2 = 'Winner: ef=60  |  Baseline: ef=50'
    fig2.suptitle('DynamicTrend EMA Fast Sweep - Global | ' + t2 + '\n        fontsize=13, fontweight=bold')
    fig2.subplots_adjust(top=0.92)

    ax = axes[0]
    ax.plot(smry['ema_fast'], smry['avg_sharpe'], 'o-', color='#1f77b4', linewidth=1.5, markersize=3)
    ax.axvline(x=60, color='#ff7f0e', linestyle='--', linewidth=2, label='Winner ef=60')
    ax.axvline(x=50, color='gray', linestyle=':', linewidth=2, label='Baseline ef=50')
    ax.set_xlabel('EMA Fast Period', fontsize=11)
    ax.set_ylabel('Avg Sharpe (multi-universe)', fontsize=11)
    ax.set_title('Avg Sharpe vs EMA Fast', fontsize=12)
    ax.legend(fontsize=9)
    ax.grid(True, alpha=0.3)

    ax = axes[1]
    ax.plot(smry['ema_fast'], smry['pass_rate'] * 100, 's-', color='#d62728', linewidth=1.5, markersize=3)
    ax.axvline(x=60, color='#ff7f0e', linestyle='--', linewidth=2, label='Winner ef=60')
    ax.axvline(x=50, color='gray', linestyle=':', linewidth=2, label='Baseline ef=50')
    ax.set_xlabel('EMA Fast Period', fontsize=11)
    ax.set_ylabel('Walk-Fwd Pass Rate pct', fontsize=11)
    ax.set_title('Pass Rate vs EMA Fast', fontsize=12)
    ax.legend(fontsize=9)
    ax.grid(True, alpha=0.3)

    ax = axes[2]
    ax.plot(smry['ema_fast'], smry['avg_dd_pct'], '^-', color='#8c564b', linewidth=1.5, markersize=3)
    ax.axvline(x=60, color='#ff7f0e', linestyle='--', linewidth=2, label='Winner ef=60')
    ax.axvline(x=50, color='gray', linestyle=':', linewidth=2, label='Baseline ef=50')
    ax.set_xlabel('EMA Fast Period', fontsize=11)
    ax.set_ylabel('Worst Drawdown pct', fontsize=11)
    ax.set_title('Max DD vs EMA Fast', fontsize=12)
    ax.legend(fontsize=9)
    ax.grid(True, alpha=0.3)

    plt.tight_layout()
    out2 = os.path.join(CHART_DIR, 'dynamic_trend_sweep_overview.png')
    fig2.savefig(out2, dpi=150, bbox_inches='tight')
    plt.close(fig2)
    print('Saved:', out2)

print('Done.')
