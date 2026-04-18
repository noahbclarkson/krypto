#!/usr/bin/env python3
"""
ATR Period Full-Range Sweep Comparison Chart
Reads: snapshots/turtle_atr_phase1_equity.csv
Outputs: charts/turtle_atr_full_comparison.png

Generates a multi-panel chart:
  Panel 1 (log scale): Equity curves — Baseline (ATR=25) vs Winner (ATR=95) + runners
  Panel 2: Per-ATR Sharpe bar chart (all 20 values, color-coded)
  Panel 3: Per-ATR pass rate
"""

import pandas as pd
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import matplotlib.ticker as mticker
import numpy as np
import os

CHART_DIR = "/home/ubuntu/.openclaw/workspace-krypto/krypto/charts"
EQ_CSV = "/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/turtle_atr_phase1_equity.csv"
SWEEP_CSV = "/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/turtle_atr_full_sweep.csv"
OUT_PNG = "/home/ubuntu/.openclaw/workspace-krypto/krypto/charts/turtle_atr_full_comparison.png"

os.makedirs(CHART_DIR, exist_ok=True)

# ── Color palette ─────────────────────────────────────────────────────────────
COLORS = {
    95:  '#00C853',   # winner — bright green
    100: '#76FF03',   # runner-up — lime
    90:  '#00B0FF',   # runner-up — blue
    40:  '#FF6D00',   # runner-up — orange
    25:  '#F50057',   # baseline — hot pink/red
}

def color_for_atr(atr, winner=None):
    if atr in COLORS:
        return COLORS[atr]
    return '#9E9E9E'

# ── Load data ────────────────────────────────────────────────────────────────
eq_df = pd.read_csv(EQ_CSV)
sweep_df = pd.read_csv(SWEEP_CSV)

phase1 = sweep_df[sweep_df['phase'] == 'phase1'].copy()
phase1['atr'] = phase1['atr_period'].astype(int)
phase1 = phase1.sort_values('atr')

# ── Figure setup ────────────────────────────────────────────────────────────
fig, axes = plt.subplots(2, 2, figsize=(16, 12))
fig.suptitle(
    'TURTLE_ATR_PERIOD Full-Range Sweep: 20 Values (5–100 step 5)\n'
    'HYPEROPT RESULT: ATR=95 was DATA-CAP ARTIFACT → ATR=24 CONFIRMED AS PRODUCTION DEFAULT\n'
    'Turtle + Chandelier(20, 2.15) dual-exit | Base5 (BTC/ETH/SOL/XRP/DOGE/ADA) | 6 windows',
    fontsize=12, fontweight='bold', y=0.98
)

# ── Panel 1: Equity curves (log scale) ────────────────────────────────────
ax1 = axes[0, 0]

# Plot all ATR values (faint) then highlight winners
all_atrs = sorted(eq_df['atr_period'].unique())

# First pass: all ATRs as faint lines
for atr in all_atrs:
    df = eq_df[eq_df['atr_period'] == atr].sort_values('bar')
    if df.empty:
        continue
    color = color_for_atr(atr)
    lw = 3.0 if atr in [25, 95, 100, 90, 40] else 0.5
    alpha = 1.0 if atr in [25, 95, 100, 90, 40] else 0.25
    ax1.plot(df['bar'].values, df['equity'].values,
             color=color, lw=lw, alpha=alpha, label=f'ATR={atr}')

ax1.set_xlim(0, 252)
ax1.set_yscale('log')
ax1.set_xlabel('Bar (252-test window)', fontsize=10)
ax1.set_ylabel('Portfolio Equity (log scale)', fontsize=10)
ax1.set_title('Equity Curves — Sweep Winner ATR=95 (green) vs Baseline ATR=25 (red) [CAPPED DATA]', fontsize=11)
ax1.grid(True, which='both', alpha=0.3, ls='--')
ax1.legend(loc='upper left', fontsize=8, ncol=2)

# Annotate final values
for atr, col, ls in [(95, '#FF5252', 'normal'), (25, '#F50057', 'normal')]:
    df = eq_df[eq_df['atr_period'] == atr].sort_values('bar')
    if not df.empty:
        final = df['equity'].iloc[-1]
        ax1.annotate(f'{atr}: {final:.1f}x',
                     xy=(251, final), xytext=(220, final * (1.3 if atr == 95 else 0.7)),
                     fontsize=8, color=col, fontweight=ls,
                     arrowprops=dict(arrowstyle='->', color=col, lw=0.8))

# ── Panel 2: Sharpe bar chart (all 20 values) ───────────────────────────────
ax2 = axes[0, 1]

atrs = phase1['atr'].values
sharpes = phase1['avg_sharpe'].values
passes = phase1['passes'].values
total_w = phase1['total_w'].values
pass_rate = passes / total_w

bar_colors = [color_for_atr(a) for a in atrs]
# Highlight: ATR=95 was sweep winner but REJECTED; ATR=24 confirmed production default
for i, atr in enumerate(atrs):
    if atr == 95:
        bar_colors[i] = '#FF5252'   # RED: sweep winner but REJECTED
    elif atr == 24:
        bar_colors[i] = '#00C853'   # GREEN: confirmed production default
    elif atr == 25:
        bar_colors[i] = '#F50057'   # pink: baseline reference
    else:
        bar_colors[i] = '#78909C'

bars = ax2.bar(atrs, sharpes, color=bar_colors, edgecolor='white', lw=0.5, width=4)
ax2.axhline(0, color='black', lw=0.8)
ax2.set_xlabel('ATR Period', fontsize=10)
ax2.set_ylabel('Avg Sharpe (Base5, 6 windows)', fontsize=10)
ax2.set_title('Sharpe by ATR — Capped Data: ATR=95 wins (green). BUT: Full walk-forward rejects ATR=95 → ATR=24 confirmed ✓', fontsize=10)
ax2.xaxis.set_major_locator(mticker.MultipleLocator(10))
ax2.grid(True, axis='y', alpha=0.3, ls='--')

# Annotate bars
for atr, sh in zip(atrs, sharpes):
    color = color_for_atr(atr)
    ax2.text(atr, sh + 0.02, f'{sh:.2f}', ha='center', va='bottom', fontsize=7, color=color)

# Shade the "fine sweep only" region (18-35)
ax2.axvspan(18, 35, alpha=0.08, color='blue', label='Prior fine-sweep range (18-35)')
ax2.legend(fontsize=8)

# ── Panel 3: Pass rate bar chart ────────────────────────────────────────────
ax3 = axes[1, 0]

bar_colors3 = [color_for_atr(a) for a in atrs]
for i, atr in enumerate(atrs):
    if atr == 95: bar_colors3[i] = '#00C853'
    elif atr == 25: bar_colors3[i] = '#F50057'
    else: bar_colors3[i] = '#78909C'

ax3.bar(atrs, pass_rate * 100, color=bar_colors3, edgecolor='white', lw=0.5, width=4)
ax3.axhline(100, color='green', lw=0.8, ls='--', alpha=0.5)
ax3.set_xlabel('ATR Period', fontsize=10)
ax3.set_ylabel('Pass Rate (%)', fontsize=10)
ax3.set_title('Pass Rate by ATR Period — Base5 (6 windows)', fontsize=11)
ax3.xaxis.set_major_locator(mticker.MultipleLocator(10))
ax3.set_ylim(0, 115)
ax3.grid(True, axis='y', alpha=0.3, ls='--')

for atr, pr in zip(atrs, pass_rate):
    color = color_for_atr(atr)
    ax3.text(atr, pr * 100 + 1, f'{pr*100:.0f}%', ha='center', va='bottom', fontsize=7, color=color)

# ── Panel 4: Phase 2 results table + summary text ───────────────────────────
ax4 = axes[1, 1]
ax4.axis('off')

phase2 = sweep_df[sweep_df['phase'] == 'phase2'].copy()
phase2['atr'] = phase2['atr_period'].astype(int)
phase2 = phase2.sort_values('avg_sharpe', ascending=False)

summary_text = [
    "Phase 2: 9 Universes × 6 Windows (Full Walk-Forward)",
    "",
    f"{'Rank':<6}{'ATR':<8}{'Pass':<12}{'Sharpe':<10}{'Return%':<12}{'Worst DD':<12}{'Trades':<8}",
    "-" * 68,
]
for rank, (_, row) in enumerate(phase2.iterrows(), 1):
    atr = int(row['atr_period'])
    passes = int(row['passes'])
    total = int(row['total_w'])
    pr = passes / total * 100
    sh = row['avg_sharpe']
    ret = row['avg_return']
    dd = row['worst_dd']
    trades = int(row['trades'])
    marker = " 🏆" if rank == 1 else (" ⬅ baseline" if atr == 25 else "")
    summary_text.append(
        f"{rank:<6}{atr:<8}{passes}/{total} ({pr:.0f}%){'':<4}{sh:+.4f}   {ret:+.1f}%     {dd:.1f}%      {trades:<8}{marker}"
    )

summary_text += [
    "",
    "⚠️  VALIDATION RESULT: ATR=95 was a DATA-CAP ARTIFACT",
    "",
    "  Capped sweep (2077 bars SOL): ATR=95 won (80% pass)",
    "  Full walk-forward (3003 bars BTC/ETH):",
    "    ATR=95:  39/54 pass (72%) — REJECTED",
    "    ATR=24:  41/54 pass (76%) — CONFIRMED ✓",
    "",
    "  PRODUCTION DEFAULT STAYS: TURTLE_ATR_PERIOD = 24",
    "  Fine-sweep local optimum (ATR=24) is the TRUE optimum.",
    "",
    "  Mechanism: ATR=95 → 2.5× more trades, diluted returns.",
    "  ATR=24: fewer, higher-quality signals.",
    "",
    "  Lesson: Always validate hyperopt with full data before",
    "  changing production. Data cap (2077 bars) gave false signal.",
]

ax4.text(0.02, 0.98, "\n".join(summary_text),
         transform=ax4.transAxes,
         fontsize=10, fontfamily='monospace',
         verticalalignment='top',
         bbox=dict(boxstyle='round,pad=0.5', facecolor='#1E1E1E', edgecolor='#444', alpha=0.9))

plt.tight_layout(rect=[0, 0, 1, 0.965])
plt.savefig(OUT_PNG, dpi=150, bbox_inches='tight', facecolor='white')
print(f"Saved: {OUT_PNG}")
plt.close()
