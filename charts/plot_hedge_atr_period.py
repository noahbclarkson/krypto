#!/usr/bin/env python3
"""
HEDGE_ATR_PERIOD Hyperparameter Comparison Chart
Reads snapshots/t85_hedge_atr_period_equity.csv (Base5 full-history equity per period)
and snapshots/t85_hedge_atr_period_summary.csv for metrics.

Baseline: P=21 (old hardcoded value before T75)
Winner:   P=38 (T75 winner on HOLD_MAX=12; T85 winner on HOLD_MAX=15/HSM=0.25)
Runner-ups: P=51, P=49, P=37, P=45 (top plateau)

Config at sweep time: HOLD_MAX=15, HSM=0.25, ATR_ENTRY_MULT=0.00, ATR_RANK=5.0
"""

import pandas as pd
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import matplotlib.ticker as mticker
import numpy as np
import os

SNAPSHOTS_DIR = "/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots"
CHARTS_DIR    = "/home/ubuntu/.openclaw/workspace-krypto/charts"
os.makedirs(CHARTS_DIR, exist_ok=True)

EQUITY_CSV  = os.path.join(SNAPSHOTS_DIR, "t85_hedge_atr_period_equity.csv")
SUMMARY_CSV = os.path.join(SNAPSHOTS_DIR, "t85_hedge_atr_period_summary.csv")
OUT_PNG     = os.path.join(CHARTS_DIR, "comparison_chart.png")

# ── Load ─────────────────────────────────────────────────────────────────────
print(f"Loading equity: {EQUITY_CSV}")
eq = pd.read_csv(EQUITY_CSV)
print(f"  Shape: {eq.shape}")

print(f"Loading summary: {SUMMARY_CSV}")
smry = pd.read_csv(SUMMARY_CSV).set_index('hedge_atr_period')
print(f"  {len(smry)} periods")

# ── Candidates ───────────────────────────────────────────────────────────────
baseline_val = 21
winner_val   = 38
runnerups    = [51, 49, 37, 45]
candidates   = [baseline_val, winner_val] + runnerups

COLOR_MAP = {
    baseline_val: "#888888",
    winner_val:   "#2196F3",
    51:            "#4CAF50",
    37:            "#FF9800",
    45:            "#9C27B0",
    49:            "#F44336",
}
LABEL_MAP = {
    baseline_val: f"Baseline P={baseline_val}",
    winner_val:   f"Winner P={winner_val}",
    51:           f"Runner-up P=51",
    37:           f"Runner-up P=37",
    45:           f"Runner-up P=45",
    49:           f"Runner-up P=49",
}

print("\nCandidate metrics (T85, HOLD_MAX=15, HSM=0.25):")
for c in candidates:
    r = smry.loc[c]
    print(f"  P={c:3d}: pass={int(r['pass_count']):>2d}/60 ({r['pass_rate_pct']:.1f}%) | "
          f"Sharpe={r['avg_sharpe']:.3f} | Ret={r['avg_return_pct']:.1f}% | "
          f"DD={r['avg_max_dd_pct']:.1f}%")

# ── Plot ─────────────────────────────────────────────────────────────────────
fig, (ax1, ax2) = plt.subplots(2, 1, figsize=(14, 10),
                                gridspec_kw={'height_ratios': [3, 1.2]})

# Equity curves (log scale, dynamic Y)
for c in candidates:
    sub = eq[eq['hedge_atr_period'] == c].sort_values('step')
    ax1.plot(sub['step'], sub['equity'],
             color=COLOR_MAP[c], label=LABEL_MAP[c],
             linewidth=1.8, alpha=0.9)

ax1.set_ylabel("Equity (Base5, full history)", fontsize=11)
ax1.set_title("HEDGE_ATR_PERIOD Sweep — Equity Curves\n"
              "Baseline P=21 vs Winner P=38 vs Runner-ups\n"
              "(T85: HOLD_MAX=15, HSM=0.25, ATR_ENTRY_MULT=0.00)", fontsize=11, pad=8)
ax1.legend(loc="upper left", fontsize=9)
ax1.grid(True, alpha=0.3, linestyle='--')
ax1.set_xlim(left=0)
ax1.set_yscale('log')
ax1.yaxis.set_major_formatter(mticker.FuncFormatter(
    lambda v, _: f"{v:.2f}" if v < 5 else (f"{v:.1f}" if v < 50 else f"{v:.0f}")))
ax1.yaxis.grid(True, alpha=0.2)

# Pass rate + Sharpe bar chart
labels_bar = [f"P={c}" for c in candidates]
pr_vals    = [smry.loc[c, 'pass_rate_pct'] for c in candidates]
sh_vals    = [smry.loc[c, 'avg_sharpe']    for c in candidates]
colors_bar = [COLOR_MAP[c] for c in candidates]

bars = ax2.bar(labels_bar, pr_vals, color=colors_bar, alpha=0.7,
               edgecolor='black', linewidth=0.5)
ax2_r = ax2.twinx()
ax2_r.plot(labels_bar, sh_vals, 'D-', color='#333333', markersize=7, linewidth=1.5)

for bar, pr in zip(bars, pr_vals):
    ax2.text(bar.get_x() + bar.get_width()/2, bar.get_height() + 0.5,
             f"{pr:.1f}%", ha='center', va='bottom', fontsize=8.5, fontweight='bold')
for i, (lbl, sh) in enumerate(zip(labels_bar, sh_vals)):
    ax2_r.annotate(f"Sharpe={sh:.3f}", xy=(i, sh),
                   xytext=(0, 8), textcoords='offset points',
                   ha='center', va='bottom', fontsize=7.5, color='#333333')

ax2.set_ylabel("Pass Rate (%)", fontsize=10)
ax2_r.set_ylabel("Avg Sharpe", fontsize=10, color='#333333')
ax2.set_ylim(0, 105)
ax2_r.set_ylim(0, max(sh_vals) * 1.5)
ax2.set_xlabel("HEDGE_ATR_PERIOD Value", fontsize=10)
ax2.grid(True, alpha=0.3, axis='y', linestyle='--')

plt.tight_layout(pad=2.0)
plt.savefig(OUT_PNG, dpi=150, bbox_inches='tight', facecolor='white')
print(f"\nSaved: {OUT_PNG}")

# ── Summary table ────────────────────────────────────────────────────────────
print("\n=== HEDGE_ATR_PERIOD Summary (T85, HOLD_MAX=15, HSM=0.25) ===")
print(f"{'P':>4} {'Pass':>7} {'Sharpe':>7} {'Return%':>9} {'MaxDD%':>8} {'WinRate%':>9}")
print("-" * 54)
for c in candidates:
    r = smry.loc[c]
    print(f"{c:>4} {int(r['pass_count']):>3}/60 {r['avg_sharpe']:>7.3f} "
          f"{r['avg_return_pct']:>9.1f} {r['avg_max_dd_pct']:>8.1f} "
          f"{r['win_rate_pct']:>9.1f}")

# Full plateau
print("\n=== Full plateau (pass_rate >= 86%) ===")
plateau = smry[smry['pass_rate_pct'] >= 86.0].sort_values('avg_sharpe', ascending=False)
print(f"{'P':>4} {'Pass':>7} {'Sharpe':>7} {'Return%':>9} {'MaxDD%':>8}")
print("-" * 42)
for p, row in plateau.iterrows():
    marker = (" <-- WINNER" if p == winner_val
              else (" <-- BASELINE" if p == baseline_val else ""))
    print(f"{int(p):>4} {int(row['pass_count']):>3}/60 {row['avg_sharpe']:>7.3f} "
          f"{row['avg_return_pct']:>9.1f} {row['avg_max_dd_pct']:>8.1f}{marker}")

# Improvement vs baseline
print("\n=== Winner vs Baseline ===")
w = smry.loc[winner_val]
b = smry.loc[baseline_val]
print(f"P={winner_val} vs P={baseline_val}:")
print(f"  Pass rate: {int(w['pass_count'])}/60 vs {int(b['pass_count'])}/60 (+{int(w['pass_count'])-int(b['pass_count'])} windows)")
print(f"  Sharpe:    {w['avg_sharpe']:.3f} vs {b['avg_sharpe']:.3f} (+{(w['avg_sharpe']/b['avg_sharpe']-1)*100:.1f}%)")
print(f"  Return:    {w['avg_return_pct']:.1f}% vs {b['avg_return_pct']:.1f}% (+{w['avg_return_pct']-b['avg_return_pct']:.1f}pp)")
print(f"  MaxDD:     {w['avg_max_dd_pct']:.1f}% vs {b['avg_max_dd_pct']:.1f}% (-{b['avg_max_dd_pct']-w['avg_max_dd_pct']:.1f}pp)")
print(f"  Note: P=38 WINNER confirmed on BOTH HOLD_MAX=12 (T75) and HOLD_MAX=15 (T85)")