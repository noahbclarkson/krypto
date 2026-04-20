#!/usr/bin/env python3
"""
CHAND_PERIOD Fine Sweep Comparison Chart
========================================
Generates charts/chand_period_fine_comparison.png

Left panel:  Sharpe vs CHAND_PERIOD (step 1, critical region [5-30])
Middle panel: Pass Rate vs CHAND_PERIOD
Right panel: Log-scale equity curves for Baseline (prior CP=11), Runner-up (CP=12), Winner (CP=13)
"""
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import matplotlib.ticker as mticker
import numpy as np
import pandas as pd

# ── Load sweep summary ─────────────────────────────────────────────────────────
sweep = pd.read_csv('snapshots/chand_period_fine_sweep.csv')
sweep = sweep.sort_values('chand_period')

# ── Load equity curves for top 3 ─────────────────────────────────────────────
def load_equity(path):
    df = pd.read_csv(path)
    # Bar vs equity
    bars = df['bar'].values
    equity = df['equity'].values
    return bars, equity

eq11_b, eq11_v = load_equity('snapshots/chand_period_cp11_equity.csv')
eq12_b, eq12_v = load_equity('snapshots/chand_period_cp12_equity.csv')
eq13_b, eq13_v = load_equity('snapshots/chand_period_cp13_equity.csv')

# ── Figure setup ─────────────────────────────────────────────────────────────
fig, axes = plt.subplots(1, 3, figsize=(18, 6), gridspec_kw={'width_ratios': [1, 1, 1.5]})
fig.patch.set_facecolor('#0d1117')
for ax in axes:
    ax.set_facecolor('#161b22')
    ax.tick_params(colors='#c9d1d9', labelsize=9)
    ax.xaxis.label.set_color('#c9d1d9')
    ax.yaxis.label.set_color('#c9d1d9')
    ax.title.set_color('#e6edf3')
    ax.spines['top'].set_visible(False)
    ax.spines['right'].set_visible(False)
    ax.spines['left'].set_color('#30363d')
    ax.spines['bottom'].set_color('#30363d')
    ax.grid(True, alpha=0.15, color='#30363d', linestyle='--')

# ── Panel A: Sharpe vs CHAND_PERIOD ─────────────────────────────────────────
ax = axes[0]
cp = sweep['chand_period'].values
sharpe = sweep['avg_sharpe'].values
colors = ['#58a6ff' if p != 13 else '#f78166' for p in cp]
bars_a = ax.bar(cp, sharpe, color=colors, width=0.7, edgecolor='none', zorder=3)
# Highlight winner
ax.bar([13], [sweep[sweep['chand_period']==13]['avg_sharpe'].values[0]],
       color='#f78166', width=0.7, edgecolor='#ffa657', linewidth=2, zorder=4, label='Winner CP=13')
# Prior winner annotation
ax.axvline(11, color='#7ee787', linewidth=1.5, linestyle='--', alpha=0.8, label='Prior CP=11')
ax.set_xlabel('CHAND_PERIOD', fontsize=11)
ax.set_ylabel('Avg Sharpe Ratio', fontsize=11)
ax.set_title('Sharpe vs CHAND_PERIOD\n(step=1, Base5, 6 windows)', fontsize=12, pad=8)
ax.xaxis.set_major_locator(mticker.MultipleLocator(2))
ax.set_xlim(4, 31)
ax.legend(fontsize=8, framealpha=0.3)

# Annotate winner
winner_sharpe = sweep[sweep['chand_period']==13]['avg_sharpe'].values[0]
ax.annotate(f'Sharpe {winner_sharpe:.3f}', xy=(13, winner_sharpe),
            xytext=(13, winner_sharpe + 0.15), ha='center', fontsize=9,
            color='#ffa657', fontweight='bold')

# Phase-transition annotation
ax.axvspan(5, 10, alpha=0.08, color='red', label='Sub-optimal zone')
ax.annotate('Sub-optimal\n(< CP=10)', xy=(7.5, 6.2), ha='center', fontsize=7.5,
            color='#f85149', style='italic')

# ── Panel B: Pass Rate vs CHAND_PERIOD ───────────────────────────────────────
ax = axes[1]
pr = sweep['pass_rate_pct'].values
colors_b = ['#58a6ff' if p != 13 else '#f78166' for p in cp]
ax.bar(cp, pr, color=colors_b, width=0.7, edgecolor='none', zorder=3)
ax.axhline(100, color='#7ee787', linewidth=1.5, linestyle='--', alpha=0.6)
ax.set_xlabel('CHAND_PERIOD', fontsize=11)
ax.set_ylabel('Pass Rate (%)', fontsize=11)
ax.set_title('Pass Rate vs CHAND_PERIOD\n(100% = all 6 windows pass)', fontsize=12, pad=8)
ax.xaxis.set_major_locator(mticker.MultipleLocator(2))
ax.set_xlim(4, 31)
ax.set_ylim(0, 115)
ax.yaxis.set_major_formatter(mticker.PercentFormatter())

# Mark sub-optimal zone
for p, pr_val in zip(cp, pr):
    if pr_val < 100:
        ax.annotate(f'{pr_val:.0f}%', xy=(p, pr_val), xytext=(p, pr_val + 3),
                    ha='center', fontsize=8, color='#f85149')

# ── Panel C: Log-scale Equity Curves ─────────────────────────────────────────
ax = axes[2]
ax.semilogy(eq11_b, eq11_v, color='#58a6ff', linewidth=2.0, label='CP=11 (prior winner)', zorder=3, alpha=0.9)
ax.semilogy(eq12_b, eq12_v, color='#a371f7', linewidth=2.0, label='CP=12 (runner-up)', zorder=3, alpha=0.9)
ax.semilogy(eq13_b, eq13_v, color='#ffa657', linewidth=2.5, label='CP=13 (WINNER)', zorder=4)
ax.set_xlabel('Test Bar', fontsize=11)
ax.set_ylabel('Portfolio Equity (log scale)', fontsize=11)
ax.set_title('Equity Curve — Top 3 Candidates\n(Base5 mean across 6 windows)', fontsize=12, pad=8)
ax.legend(fontsize=9, framealpha=0.3, loc='upper left')
ax.grid(True, alpha=0.15, color='#30363d', which='both')

# Add final equity annotation
for (bars, equity, label, color) in [(eq11_b, eq11_v, 'CP=11', '#58a6ff'),
                                        (eq12_b, eq12_v, 'CP=12', '#a371f7'),
                                        (eq13_b, eq13_v, 'CP=13', '#ffa657')]:
    final = equity[-1]
    ax.annotate(f'{label}: {final:.3f}x', xy=(bars[-1], final),
                xytext=(bars[-1] - 30, final * 1.15), fontsize=8,
                color=color, ha='right')

# ── Overall title and caption ─────────────────────────────────────────────────
fig.suptitle('CHAND_PERIOD Fine Sweep (step=1, critical region [5–30]) — Turtle+Chandelier\n'
             'Fixed: CM=2.25, EP=21, ATR=24, ATR_M=2.0, HM=45, CAP=3',
             fontsize=13, color='#e6edf3', y=1.02)

# Caption with key metrics
fig.text(0.5, -0.04,
         'CP=13 wins with Sharpe 7.012 (+0.3% vs CP=11=6.993, within noise). '
         'CP ≤ 9: pass rate collapses to 66.7%. Phase transition at CP≈10. '
         'Prior production: CP=11 (confirmed robust). CP=13 is marginal alternative.',
         ha='center', fontsize=9, color='#8b949e', style='italic',
         wrap=True)

plt.tight_layout(rect=[0, 0.02, 1, 1])
plt.savefig('charts/chand_period_fine_comparison.png', dpi=150, bbox_inches='tight',
            facecolor='#0d1117', edgecolor='none')
print("Saved: charts/chand_period_fine_comparison.png")
plt.close()
