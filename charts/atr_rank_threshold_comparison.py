#!/usr/bin/env python3
"""
ATR_RANK_THRESHOLD Hyperopt — Final Chart
======================================
Generates comparison_chart.png from snapshots/atr_rank_t_summary.csv
with equity curve style visualization.
"""
import csv, math, os

import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import matplotlib.ticker as mticker

OUT_PNG = os.path.join(os.path.dirname(__file__), 'comparison_chart.png')
MAX_SHARPE = 50.0  # clip mega-outliers from 1-2 trade windows

# ── load summary ──────────────────────────────────────────────────────────────
rows = []
with open('snapshots/atr_rank_t_summary.csv') as f:
    for r in csv.DictReader(f):
        t   = float(r['threshold'])
        pc  = int(r['pass_count'])
        tt  = int(r['total_trades'])
        ret = float(r['avg_return_pct'])
        sh  = float(r['avg_sharpe'])
        dd  = float(r['avg_dd_pct'])
        if tt == 0 or pc == 0:
            continue
        if sh > MAX_SHARPE:  sh = MAX_SHARPE
        if sh < -MAX_SHARPE: sh = -MAX_SHARPE
        rows.append(dict(threshold=t, pass_count=pc, total_trades=tt,
                         avg_return_pct=ret, avg_sharpe=sh, avg_dd_pct=dd))

TOTAL_WINDOWS = 63  # 9 universes × 7 windows

ts      = [r['threshold']          for r in rows]
passes  = [r['pass_count']         for r in rows]
sharps  = [r['avg_sharpe']         for r in rows]
rets    = [r['avg_return_pct']      for r in rows]
dds     = [r['avg_dd_pct']         for r in rows]
pr_pct  = [p / TOTAL_WINDOWS * 100 for p in passes]

# ── winner selection (robustness-first: pass rate > Sharpe > return) ───────
by_pass   = sorted(rows, key=lambda r: (r['pass_count'], r['avg_sharpe'], r['avg_return_pct']), reverse=True)
by_sharpe = sorted(rows, key=lambda r: r['avg_sharpe'], reverse=True)
winner_row = by_pass[0]
base_row   = next(r for r in rows if r['threshold'] == 0.0)
WINNER_T  = int(winner_row['threshold'])
BASE_T    = 0
RUNNER_T1 = int(by_sharpe[1]['threshold']) if len(by_sharpe) > 1 else None
RUNNER_T2 = int(by_sharpe[2]['threshold']) if len(by_sharpe) > 2 else None

print(f"BASELINE T={BASE_T}:  pass={base_row['pass_count']}/{TOTAL_WINDOWS} "
      f"({base_row['pass_count']/TOTAL_WINDOWS*100:.1f}%), Sharpe={base_row['avg_sharpe']:.3f}")
print(f"WINNER   T={WINNER_T}:  pass={winner_row['pass_count']}/{TOTAL_WINDOWS} "
      f"({winner_row['pass_count']/TOTAL_WINDOWS*100:.1f}%), Sharpe={winner_row['avg_sharpe']:.3f}")

# ── figure ─────────────────────────────────────────────────────────────────────
fig, axes = plt.subplots(2, 2, figsize=(16, 12))
fig.patch.set_facecolor('#0d1117')
for ax in axes.flat:
    ax.set_facecolor('#161b22')
fig.suptitle(
    f'ATR_RANK_THRESHOLD Hyperparameter Optimization\n'
    f'T ∈ [0..100 step 1] × 9 universes × 7 windows = 6,363 runs | Winner: T={WINNER_T}',
    color='white', fontsize=13, y=0.98)

# ── Top-left: pass rate vs threshold ───────────────────────────────────────────
ax = axes[0, 0]
ax.fill_between(ts, pr_pct, alpha=0.15, color='#58a6ff')
ax.plot(ts, pr_pct, color='#58a6ff', lw=2.0, label='Pass Rate %')
ax.axhline(70, color='#f0883e', lw=1.2, ls='--', alpha=0.8, label='70% guardrail')
ax.axhline(winner_row['pass_count']/TOTAL_WINDOWS*100,
           color='#3fb950', lw=1.5, ls=':', alpha=0.9, label=f"Win T={WINNER_T}")
ax.set_xlabel('ATR Rank Threshold (T)', color='white', fontsize=11)
ax.set_ylabel('Pass Rate (%)', color='#58a6ff', fontsize=11)
ax.tick_params(colors='white'); ax.yaxis.label.set_color('#58a6ff')
ax.set_xlim(0, 100); ax.set_ylim(30, 100)
ax.set_title('Pass Rate vs Threshold', color='white', fontsize=11, pad=8)
ax.grid(True, alpha=0.12, color='white')
ax.legend(loc='upper right', facecolor='#1c2128', labelcolor='white', fontsize=9)

# ── Top-right: Sharpe vs threshold ───────────────────────────────────────────
ax2 = axes[0, 1]
ax2.plot(ts, sharps, color='#ff7b72', lw=2.0, label='Avg Sharpe')
ax2.axhline(base_row['avg_sharpe'], color='#ff7b72', lw=1.0, ls='--', alpha=0.5)
for rank, r in enumerate(by_sharpe[:3]):
    col = ['#3fb950', '#58a6ff', '#d2a8ff'][rank]
    lw  = [2.5, 2.0, 1.8][rank]
    ax2.axvline(r['threshold'], color=col, lw=lw, ls=':', alpha=0.85)
    ax2.annotate(f"T={int(r['threshold'])}\nS={r['avg_sharpe']:.2f}",
                xy=(r['threshold'], r['avg_sharpe']),
                xytext=(r['threshold']+4, r['avg_sharpe']+0.3),
                fontsize=8, color=col,
                arrowprops=dict(arrowstyle='->', color=col, lw=0.8))
ax2.set_xlabel('ATR Rank Threshold (T)', color='white', fontsize=11)
ax2.set_ylabel('Avg Sharpe', color='#ff7b72', fontsize=11)
ax2.tick_params(colors='white'); ax2.yaxis.label.set_color('#ff7b72')
ax2.set_xlim(0, 100)
ax2.set_title(f'Sharpe vs Threshold (clipped ±{MAX_SHARPE})', color='white', fontsize=11, pad=8)
ax2.grid(True, alpha=0.12, color='white')

# ── Bottom-left: top-10 bar chart by Sharpe ─────────────────────────────────
ax3 = axes[1, 0]
top10 = by_sharpe[:10]
names = [f"T={int(r['threshold'])}" for r in top10]
sps   = [r['avg_sharpe'] for r in top10]
prs   = [r['pass_count'] / TOTAL_WINDOWS * 100 for r in top10]
cols  = ['#3fb950' if r['threshold'] == WINNER_T else
         '#d2a8ff' if r['threshold'] == RUNNER_T1 else
         '#79c0ff' if r['threshold'] == RUNNER_T2 else
         '#484f58' for r in top10]
bars  = ax3.bar(names, sps, color=cols, edgecolor='white', lw=0.5, width=0.6)
for bar, p in zip(bars, prs):
    ax3.text(bar.get_x()+bar.get_width()/2, bar.get_height()+0.05,
             f'{p:.0f}%', ha='center', va='bottom', fontsize=8, color='white')
ax3.set_ylabel('Avg Sharpe', color='white', fontsize=11)
ax3.set_xlabel('Threshold',   color='white', fontsize=11)
ax3.set_title('Top 10 Thresholds by Sharpe (% pass annotated)', color='white', fontsize=11, pad=8)
ax3.tick_params(colors='white')
ax3.yaxis.label.set_color('white')
ax3.grid(True, axis='y', alpha=0.12, color='white')
ax3.set_ylim(0, max(sps)*1.25)

# ── Bottom-right: return + DD scatter ─────────────────────────────────────
ax4 = axes[1, 1]
for r in rows:
    alpha = 0.9 if r['threshold'] in [BASE_T, WINNER_T] else 0.3
    lw    = 2.0  if r['threshold'] in [BASE_T, WINNER_T] else 0.8
    col   = '#3fb950' if r['threshold'] == WINNER_T else \
            '#58a6ff' if r['threshold'] == BASE_T else '#8b949e'
    ax4.scatter(r['avg_return_pct'], r['avg_dd_pct'], c=col, s=20, alpha=alpha, lw=lw)
ax4.set_xlabel('Avg Return (%)', color='white', fontsize=11)
ax4.set_ylabel('Avg Drawdown (%)', color='white', fontsize=11)
ax4.tick_params(colors='white')
ax4.set_title('Return vs Drawdown (green=win, blue=base, grey=other)', color='white', fontsize=11, pad=8)
ax4.grid(True, alpha=0.12, color='white')

# Annotate winner and baseline
for r in [winner_row, base_row]:
    col = '#3fb950' if r['threshold'] == WINNER_T else '#58a6ff'
    ax4.annotate(f"T={int(r['threshold'])}",
                xy=(r['avg_return_pct'], r['avg_dd_pct']),
                xytext=(r['avg_return_pct']+5, r['avg_dd_pct']+2),
                fontsize=9, color=col,
                arrowprops=dict(arrowstyle='->', color=col, lw=0.8))

# ── caption ──────────────────────────────────────────────────────────────────
gap_pass  = winner_row['pass_count'] - base_row['pass_count']
gap_sh    = winner_row['avg_sharpe'] - base_row['avg_sharpe']
caption = (
    f"T={WINNER_T} WINNER | +{gap_pass} pass ({base_row['pass_count']}→{winner_row['pass_count']}), "
    f"Sharpe {base_row['avg_sharpe']:.2f}→{winner_row['avg_sharpe']:.2f} ({gap_sh:+.2f}), "
    f"Return {base_row['avg_return_pct']:.1f}%→{winner_row['avg_return_pct']:.1f}%, "
    f"DD {base_row['avg_dd_pct']:.1f}%→{winner_row['avg_dd_pct']:.1f}%. "
    f"Sweep: 101 thresholds × 9 universes × 7 windows. "
    f"Production default ATR_RANK_THRESHOLD = {WINNER_T}.0 (from src/live/config.rs)."
)
fig.text(0.5, 0.01, caption, ha='center', va='bottom', fontsize=9,
         color='#8b949e', style='italic')

plt.tight_layout(rect=[0, 0.04, 1, 0.97])
plt.savefig(OUT_PNG, dpi=180, bbox_inches='tight', facecolor=fig.get_facecolor())
plt.close()
print(f"\nSaved: {OUT_PNG}")
print(f"\nCaption:\n{caption}")
