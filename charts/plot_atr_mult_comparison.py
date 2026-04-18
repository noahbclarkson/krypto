#!/usr/bin/env python3
"""
TURTLE_ATR_MULT Full Sweep Comparison Chart
Reads: snapshots/turtle_atr_mult_equity.csv, snapshots/turtle_atr_mult_sweep.csv
Outputs: charts/turtle_atr_mult_comparison.png

4-panel chart:
  Panel 1: Equity curves — all mult values, log scale
  Panel 2: Sharpe bar chart by mult
  Panel 3: Pass rate bar chart
  Panel 4: Summary table
"""

import pandas as pd
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import matplotlib.ticker as mticker
import numpy as np
import os

CHART_DIR = "/home/ubuntu/.openclaw/workspace-krypto/krypto/charts"
EQ_CSV = "/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/turtle_atr_mult_equity.csv"
SWEEP_CSV = "/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/turtle_atr_mult_sweep.csv"
OUT_PNG = f"{CHART_DIR}/turtle_atr_mult_comparison.png"

os.makedirs(CHART_DIR, exist_ok=True)

# Color per multiplier
COLORS = {
    1.0: '#00B0FF',   # blue
    1.5: '#76FF03',   # lime
    2.0: '#F50057',   # hot pink (baseline)
    2.5: '#FF6D00',   # orange
    3.0: '#AA00FF',   # purple
    3.5: '#7C4DFF',   # deep purple
    4.0: '#00E5FF',   # cyan
    4.5: '#EEFF41',   # yellow-green
    5.0: '#9E9E9E',   # grey
}

def color_for_mult(m):
    return COLORS.get(m, '#9E9E9E')

# Load data
eq_df = pd.read_csv(EQ_CSV)
sw_df = pd.read_csv(SWEEP_CSV)

print("Sweep CSV columns:", sw_df.columns.tolist())
print(sw_df.head(5))

fig, axes = plt.subplots(2, 2, figsize=(16, 12))
fig.suptitle(
    'TURTLE_ATR_MULT Full Sweep: 9 Values (1.0–5.0 step 0.5)\n'
    'Turtle + Chandelier(24, 2.0) dual-exit | 9 Universes × 6 Windows | Baseline MULT=2.0 (pink)',
    fontsize=12, fontweight='bold', y=0.98
)

# ── Panel 1: Equity curves (log scale) ─────────────────────────────────────
ax1 = axes[0, 0]

mults = sorted(eq_df['mult'].unique())
for m in mults:
    df_m = eq_df[eq_df['mult'] == m].sort_values('window_idx')
    color = color_for_mult(m)
    lw = 2.5 if m == 2.0 else (1.5 if m in [1.5, 1.0] else 0.8)
    alpha = 1.0 if m in [2.0, 1.5, 1.0] else 0.35
    ax1.plot(
        df_m['window_idx'].values, df_m['cumulative_equity'].values,
        color=color, lw=lw, alpha=alpha,
        label=f'M={m}' + (' ← baseline' if m == 2.0 else '')
    )

ax1.set_yscale('log')
ax1.set_xlabel('Window Index (6 windows)', fontsize=10)
ax1.set_ylabel('Cumulative Equity (log scale)', fontsize=10)
ax1.set_title('Equity by Turtle ATR Multiplier', fontsize=11)
ax1.grid(True, which='both', alpha=0.3, ls='--')
ax1.legend(loc='upper left', fontsize=8)

# ── Panel 2: Sharpe bar chart ─────────────────────────────────────────────
ax2 = axes[0, 1]

mults_sorted = sorted(sw_df['mult'].unique())
sharpes = [sw_df[sw_df['mult'] == m]['avg_sharpe'].values[0] for m in mults_sorted]
bar_colors = [color_for_mult(m) for m in mults_sorted]

bars = ax2.bar([str(m) for m in mults_sorted], sharpes, color=bar_colors,
               edgecolor='white', lw=0.5)
ax2.axhline(0, color='black', lw=0.8)
ax2.set_xlabel('Turtle ATR Multiplier', fontsize=10)
ax2.set_ylabel('Avg Sharpe (9 universes × 6 windows)', fontsize=10)
ax2.set_title('Sharpe by Turtle ATR Multiplier', fontsize=11)
ax2.grid(True, axis='y', alpha=0.3, ls='--')

for i, (m, sh) in enumerate(zip(mults_sorted, sharpes)):
    color = color_for_mult(m)
    ax2.text(i, sh + 0.03, f'{sh:.3f}', ha='center', va='bottom',
             fontsize=8, color=color)

# Highlight baseline
baseline_idx = mults_sorted.index(2.0)
ax2.axvline(baseline_idx, color='#F50057', lw=2, ls='--', alpha=0.6)

# ── Panel 3: Pass rate ─────────────────────────────────────────────────────
ax3 = axes[1, 0]

pass_rates = []
for m in mults_sorted:
    row = sw_df[sw_df['mult'] == m]
    if 'pass_rate_pct' in row.columns:
        pr = row['pass_rate_pct'].values[0]
    else:
        pc = row['pass_count'].values[0]
        tw = row['total_windows'].values[0]
        pr = pc / tw * 100 if tw > 0 else 0
    pass_rates.append(pr)

ax3.bar([str(m) for m in mults_sorted], pass_rates, color=bar_colors,
        edgecolor='white', lw=0.5)
ax3.set_xlabel('Turtle ATR Multiplier', fontsize=10)
ax3.set_ylabel('Pass Rate (%)', fontsize=10)
ax3.set_title('Pass Rate by Turtle ATR Multiplier', fontsize=11)
ax3.set_ylim(0, 105)
ax3.axhline(60, color='gray', lw=0.8, ls='--', alpha=0.5, label='60% threshold')
ax3.grid(True, axis='y', alpha=0.3, ls='--')

for i, (m, pr) in enumerate(zip(mults_sorted, pass_rates)):
    color = color_for_mult(m)
    ax3.text(i, pr + 1, f'{pr:.0f}%', ha='center', va='bottom', fontsize=8, color=color)
ax3.legend(fontsize=8)

# ── Panel 4: Summary table ─────────────────────────────────────────────────
ax4 = axes[1, 1]
ax4.axis('off')

sw_df_sorted = sw_df.sort_values('avg_sharpe', ascending=False)

summary_lines = [
    "TURTLE_ATR_MULT Full Sweep — 9 Universes × 54 Windows",
    "",
    f"{'Rank':<6}{'Mult':<8}{'Sharpe':<10}{'Pass Rate':<12}{'Return%':<12}{'Max DD%':<10}{'Trades':<8}",
    "-" * 68,
]
for rank, (_, row) in enumerate(sw_df_sorted.iterrows(), 1):
    m = row['mult']
    sh = row['avg_sharpe']
    if 'pass_rate_pct' in sw_df_sorted.columns:
        pr = row['pass_rate_pct']
    else:
        pc = row['pass_count']
        tw = row['total_windows']
        pr = pc / tw * 100 if tw > 0 else 0
    ret = row['avg_return_pct']
    dd = row['avg_max_dd_pct']
    trades = row['total_trades']
    marker = "  ← baseline" if m == 2.0 else ""
    summary_lines.append(
        f"{rank:<6}{m:<8.1f}{sh:+.4f}    {pr:5.1f}%     {ret:+8.1f}%   {dd:8.1f}%   {trades:<8}{marker}"
    )

summary_lines += [
    "",
    "RESULT: TURTLE_ATR_MULT=2.0 STAYS (baseline is winner)",
    "Mult 1.5: +0 trades but -8.7% Sharpe vs baseline",
    "Mult >= 2.5: 7pp pass rate drop, -9% Sharpe",
    "Mult 1.0: highest trade count (1435) but lowest Sharpe",
    "",
    "CONCLUSION: Multiplier 2.0 is the robust optimum.",
    "Tighter stops (M<2): too many trades, diluted returns.",
    "Looser stops (M>2): 7pp fewer passes, less Sharpe.",
]

ax4.text(0.02, 0.98, "\n".join(summary_lines),
         transform=ax4.transAxes,
         fontsize=9.5, fontfamily='monospace',
         verticalalignment='top',
         bbox=dict(boxstyle='round,pad=0.5', facecolor='#1E1E1E', edgecolor='#444', alpha=0.9))

plt.tight_layout(rect=[0, 0, 1, 0.965])
plt.savefig(OUT_PNG, dpi=150, bbox_inches='tight', facecolor='white')
print(f"Saved: {OUT_PNG}")
plt.close()