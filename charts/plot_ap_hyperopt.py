#!/usr/bin/env python3
"""
AP (REGIME_ATR_PERIOD) hyperopt comparison chart.
Updated: 2026-05-04
Key finding: AP=17 wins OOS on Sharpe/return, AP=63 wins on pass rate.
Held-out confirms: AP=17 4/4 (Sharpe 7.715, equity 1.9481x) vs AP=63 4/4 (Sharpe 5.721, equity 1.3976x).
AP=17 is promoted to production default.
"""
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import matplotlib.ticker as mticker
import pandas as pd
import numpy as np
import os

SNAPSHOT_DIR = "/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/ap_hyperopt"
CHART_DIR = "/home/ubuntu/.openclaw/workspace-krypto/krypto/charts"

# Values to show: baseline (AP=12), runner-ups (7, 37), OOS winner (63), promoted winner (17)
AP_VALUES = [7, 12, 17, 37, 63]

COLORS = {
    7:  '#2196F3',
    12: '#4CAF50',
    17: '#FF5722',  # Winner (promoted)
    37: '#9C27B0',
    63: '#F44336',
}
LINESTYLES = {17: '-', 63: '--', 7: ':', 12: '-', 37: '--'}
LINEWIDTHS = {17: 2.5, 63: 2.0, 12: 2.0, 7: 1.5, 37: 1.5}

LABELS = {
    7:  'AP=7 (88.9% pass, Sharpe 4.39)',
    12: 'AP=12 baseline (87.3% pass, Sharpe 4.91)',
    17: 'AP=17 WINNER (87.3% pass, Sharpe 6.37) ← PROMOTED',
    37: 'AP=37 (88.9% pass, Sharpe 5.84)',
    63: 'AP=63 OOS-pass-winner (88.9% pass, Sharpe 6.11) — held-out: inferior to AP=17',
}

def load_base5_aggregate(ap):
    path = os.path.join(SNAPSHOT_DIR, f"base5_agg_ap{ap}.csv")
    return pd.read_csv(path) if os.path.exists(path) else None

def load_base5_per_window(ap):
    path = os.path.join(SNAPSHOT_DIR, f"Base5_ap{ap}_per_window.csv")
    return pd.read_csv(path) if os.path.exists(path) else None

sweep = pd.read_csv(os.path.join(SNAPSHOT_DIR, "../ap_hyperopt_sweep.csv"))
sweep_relevant = sweep[sweep['ap'].isin(AP_VALUES)].sort_values('ap')

# Held-out results (from ap_held_out_comprehensive_summary.csv)
HELD_OUT = {
    7:  {'pass': 3, 'total': 4, 'equity': 1.5235, 'sharpe': 3.067, 'dd': 28.7},
    12: {'pass': 2, 'total': 4, 'equity': 1.3645, 'sharpe': 3.762, 'dd': 28.0},
    17: {'pass': 4, 'total': 4, 'equity': 1.9481, 'sharpe': 7.715, 'dd': 21.3},
    37: {'pass': 3, 'total': 4, 'equity': 1.4067, 'sharpe': 4.425, 'dd': 27.7},
    63: {'pass': 4, 'total': 4, 'equity': 1.3976, 'sharpe': 5.721, 'dd': 26.6},
}

fig, axes = plt.subplots(2, 2, figsize=(16, 12))
fig.suptitle(
    'REGIME_ATR_PERIOD (AP) Hyperopt — Winner: AP=17\n'
    'OOS: AP=63 won by pass rate; Held-out confirms AP=17 superior on Sharpe/Return/DD\n'
    'AP=17: 4/4 held-out, Sharpe 7.715, equity 1.9481x | AP=63: 4/4 held-out, Sharpe 5.721, equity 1.3976x',
    fontsize=13, fontweight='bold', y=0.98
)

# ==============================================================================
# Panel 1: Base5 Aggregate Equity — log scale line chart
# ==============================================================================
ax1 = axes[0, 0]
for ap in AP_VALUES:
    df = load_base5_aggregate(ap)
    if df is None:
        continue
    ls = LINESTYLES.get(ap, '-')
    lw = LINEWIDTHS.get(ap, 2.0)
    ax1.plot(df['window'], df['agg_equity'], color=COLORS[ap], linestyle=ls,
             linewidth=lw, label=LABELS[ap], marker='o' if ap in [17, 63] else None,
             markersize=5)

ax1.set_yscale('log')
ax1.yaxis.set_major_formatter(mticker.FuncFormatter(lambda x, _: f'{x:.0f}x'))
ax1.set_xlabel('Walk-Forward Window', fontsize=11)
ax1.set_ylabel('Aggregate Equity (log scale)', fontsize=11)
ax1.set_title('Base5 Aggregate Equity by AP Value', fontsize=12)
ax1.legend(fontsize=8, loc='upper left')
ax1.grid(True, alpha=0.3)

# Annotate final equity
for ap in [12, 17, 63]:
    df = load_base5_aggregate(ap)
    if df is not None and len(df) > 0:
        final = df['agg_equity'].values[-1]
        offset = 1.5 if ap == 17 else 0.6
        ax1.annotate(f'AP={ap}: {final:.0f}x', xy=(len(df)-1, final),
                    xytext=(len(df)-3, final * offset),
                    fontsize=8, color=COLORS[ap],
                    arrowprops=dict(arrowstyle='->', color=COLORS[ap], alpha=0.5) if ap == 17 else None)

# ==============================================================================
# Panel 2: Per-Window Sharpe — line chart
# ==============================================================================
ax2 = axes[0, 1]
for ap in AP_VALUES:
    df = load_base5_per_window(ap)
    if df is None:
        continue
    ls = LINESTYLES.get(ap, '-')
    lw = LINEWIDTHS.get(ap, 2.0)
    ax2.plot(df['window'], df['sharpe'], color=COLORS[ap], linestyle=ls,
             linewidth=lw, label=f'AP={ap}', marker='o' if ap in [17, 63] else None, markersize=4)

ax2.axhline(y=0, color='black', linewidth=0.5)
ax2.set_xlabel('Walk-Forward Window', fontsize=11)
ax2.set_ylabel('Sharpe Ratio', fontsize=11)
ax2.set_title('Per-Window Sharpe by AP Value', fontsize=12)
ax2.legend(fontsize=9, ncol=2)
ax2.grid(True, alpha=0.3)

# ==============================================================================
# Panel 3: OOS Sweep — Pass Rate by AP
# ==============================================================================
ax3 = axes[1, 0]
x = np.arange(len(sweep_relevant))
bar_colors = [COLORS[ap] for ap in sweep_relevant['ap']]
pass_pcts = sweep_relevant['pass'] / sweep_relevant['total'] * 100
bars = ax3.bar(x, pass_pcts, color=bar_colors, alpha=0.85, edgecolor='black', linewidth=0.5)
for bar, pct in zip(bars, pass_pcts):
    ax3.text(bar.get_x() + bar.get_width()/2, bar.get_height() + 0.2,
             f'{pct:.1f}%', ha='center', va='bottom', fontsize=9, fontweight='bold')

ax3.set_xticks(x)
ax3.set_xticklabels([f'AP={ap}' for ap in sweep_relevant['ap']])
ax3.set_ylabel('Pass Rate (%)', fontsize=11)
ax3.set_title('OOS Sweep: Pass Rate by AP Value', fontsize=12)
ax3.set_ylim(80, 95)
ax3.axhline(y=87.3, color='#4CAF50', linestyle=':', alpha=0.6, label='AP=12 baseline (87.3%)')
ax3.axhline(y=88.9, color='#F44336', linestyle=':', alpha=0.6, label='88.9% threshold (AP=63 winner)')
ax3.legend(fontsize=8)
ax3.grid(True, alpha=0.3, axis='y')

# Highlight winner bar
for bar, ap in zip(bars, sweep_relevant['ap']):
    if ap == 17:
        for rect in [bar]:
            rect.set_edgecolor('#FF5722')
            rect.set_linewidth(2.5)

# ==============================================================================
# Panel 4: Held-Out Comparison — Side-by-side bars
# ==============================================================================
ax4 = axes[1, 1]
aps = [7, 12, 17, 37, 63]
n = len(aps)
x = np.arange(n)
w = 0.22

pass_bars = [HELD_OUT[ap]['pass'] / HELD_OUT[ap]['total'] * 100 for ap in aps]
sharpe_vals = [HELD_OUT[ap]['sharpe'] for ap in aps]
ret_vals = [(HELD_OUT[ap]['equity'] - 1) * 100 for ap in aps]  # % return
dd_vals = [-HELD_OUT[ap]['dd'] for ap in aps]

ax4.bar(x - w, pass_bars, w, label='Pass%', color='#2196F3', alpha=0.85)
ax4.bar(x,     [s/2 for s in sharpe_vals], w, label='Sharpe/2', color='#FF5722', alpha=0.85)
ax4.bar(x + w, ret_vals, w, label='Return%', color='#4CAF50', alpha=0.85)

ax4.set_xticks(x)
ax4.set_xticklabels([f'AP={ap}' for ap in aps])
ax4.set_ylabel('Value', fontsize=11)
ax4.set_title('Held-Out Validation (Pre-2021 Data)\nAP=17: 4/4 pass, Sharpe 7.715, equity 1.9481x — BEST', fontsize=12)
ax4.legend(fontsize=9)
ax4.grid(True, alpha=0.3, axis='y')
ax4.axhline(y=0, color='black', linewidth=0.5)

# Annotate winner
for i, ap in enumerate(aps):
    if ap == 17:
        ax4.annotate('← PROMOTED', xy=(i + w, ret_vals[i]),
                    xytext=(i + 1.5, ret_vals[i] + 10),
                    fontsize=9, color='#FF5722', fontweight='bold',
                    arrowprops=dict(arrowstyle='->', color='#FF5722'))

plt.tight_layout(rect=[0, 0, 1, 0.95])
out_path = os.path.join(CHART_DIR, 'comparison_chart.png')
plt.savefig(out_path, dpi=150, bbox_inches='tight')
plt.close()
print(f"Saved: {out_path}")

# Print summary for report
print("\n=== AP Hyperopt Results ===")
print("OOS Sweep (9 universes × 7 windows = 63 windows per AP):")
sorted_sweep = sweep_relevant.sort_values('sharpe', ascending=False)
for _, row in sorted_sweep.iterrows():
    pass_pct = row['pass'] / row['total'] * 100
    print(f"  AP={int(row['ap'])}: pass={int(row['pass'])}/63 ({pass_pct:.1f}%), "
          f"Sharpe={row['sharpe']:.3f}, Ret={row['ret']:.1f}%, DD={row['dd']:.1f}%")

print("\nHeld-Out Validation (Pre-2021 data):")
for ap in sorted(aps):
    h = HELD_OUT[ap]
    print(f"  AP={ap}: {h['pass']}/{h['total']} pass ({h['pass']/h['total']*100:.0f}%), "
          f"equity={h['equity']:.4f}x, Sharpe={h['sharpe']:.3f}, DD={h['dd']:.1f}%")

print("\nDecision: AP=17 → production default (AP=63 wins OOS by pass rate, "
      "but AP=17 dominates held-out on Sharpe/Return/DD)")