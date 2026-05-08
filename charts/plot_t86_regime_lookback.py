#!/usr/bin/env python3
"""
T86 REGIME_LOOKBACK Hyperopt Comparison Chart

Generates: /home/ubuntu/.openclaw/workspace-krypto/charts/comparison_chart.png

Data source: snapshots/t86_regime_lookback_equity.csv
  Columns: bar, date, lb_5, lb_8, lb_10  (only top few winner equity columns exported)

Also shows: live_bot_exact_equity.csv (baseline LB=41 / current production)

Winners from T86 sweep:
  LB=8  → equity 3.3607 (winner)
  LB=5  → equity 3.3565 (runner-up 1)
  LB=10 → equity 3.3529 (runner-up 2)
  LB=40 → equity 2.9299 (nearest to production LB=41)

Baseline: live_bot_exact_equity.csv (LB=41 production = 2.77x)
"""

import pandas as pd
import numpy as np
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import matplotlib.dates as mdates
from matplotlib.ticker import PercentFormatter
import os

OUT = "/home/ubuntu/.openclaw/workspace-krypto/charts/comparison_chart.png"
WORKSPACE = "/home/ubuntu/.openclaw/workspace-krypto/krypto"

os.makedirs(os.path.dirname(OUT), exist_ok=True)

# ── 1. Load T86 equity time-series ─────────────────────────────────────────
eq_t86 = pd.read_csv(f"{WORKSPACE}/snapshots/t86_regime_lookback_equity.csv")
eq_t86['date'] = pd.to_datetime(eq_t86['date'])

# The T86 equity CSV only has top winners (lb_5, lb_8, lb_10) + baseline in columns
# Check what's actually available
print("T86 equity columns:", eq_t86.columns.tolist())

# ── 2. Load live_bot exact equity (production baseline LB=41) ────────────────
eq_live = pd.read_csv(f"{WORKSPACE}/snapshots/live_bot_exact_equity.csv")
eq_live['date'] = pd.to_datetime(eq_live['date'])

# ── 3. Load sweep summary for metrics ────────────────────────────────────────
sw = pd.read_csv(f"{WORKSPACE}/snapshots/t86_regime_lookback_sweep.csv")
sw_u = sw.drop_duplicates('lb').sort_values('lb')
metrics = {
    5: sw_u[sw_u['lb']==5].iloc[0],
    8: sw_u[sw_u['lb']==8].iloc[0],
    10: sw_u[sw_u['lb']==10].iloc[0],
}

live_final = eq_live['equity'].iloc[-1]

# ── 4. Build the comparison chart ───────────────────────────────────────────
fig, (ax1, ax2) = plt.subplots(2, 1, figsize=(14, 10), gridspec_kw={'height_ratios': [3, 1]})
fig.suptitle("T86 REGIME_LOOKBACK Hyperopt — Exact-Live Comparison\n"
             "Production Turtle-only config vs. T86 Winners (Base5, 300-bar warmup)",
             fontsize=13, fontweight='bold', y=0.98)

COLORS = {
    'baseline': '#555555',
    'winner':   '#00BCD4',
    'runner1':  '#FF9800',
    'runner2':  '#9C27B0',
}

# Subplot 1: equity curves (log scale)
ax1.set_title("Equity Curve (log scale)", fontsize=11)
ax1.set_ylabel("Portfolio Equity", fontsize=10)
ax1.set_xlabel("")
ax1.grid(True, alpha=0.3, linestyle='--')
ax1.set_yscale('log')
ax1.yaxis.set_major_formatter(plt.FuncFormatter(lambda v, _: f'{v:.2f}x'))

# Plot live_bot exact equity (LB=41 production baseline)
ax1.plot(eq_live['date'], eq_live['equity'],
         color=COLORS['baseline'], linewidth=2.0, alpha=0.85,
         label=f"LB=41 Production Baseline (equity={live_final:.2f}x)")

# Plot T86 winners from equity CSV
t86_cols = [c for c in eq_t86.columns if c.startswith('lb_') and c != 'bar']
label_map = {'lb_8': ('LB=8 Winner', COLORS['winner']),
             'lb_5': ('LB=5 Runner-up', COLORS['runner1']),
             'lb_10': ('LB=10 Runner-up', COLORS['runner2'])}

for col in sorted(t86_cols):
    if col in label_map:
        label, color = label_map[col]
        final_eq = eq_t86[col].iloc[-1]
        ax1.plot(eq_t86['date'], eq_t86[col],
                 color=color, linewidth=2.0, alpha=0.85,
                 label=f"{label} (equity={final_eq:.2f}x)")

ax1.legend(loc='upper left', fontsize=9, framealpha=0.9)
ax1.set_xlim(eq_live['date'].min(), eq_live['date'].max())

# Subplot 2: drawdown (linear scale) for LB=41 baseline
def compute_dd(equity_series):
    peak = equity_series.cummax()
    dd = (peak - equity_series) / peak * 100
    return dd

ax2.set_title("Drawdown — Production Baseline LB=41 (linear scale)", fontsize=11)
ax2.set_ylabel("Drawdown %", fontsize=10)
ax2.set_xlabel("Date", fontsize=10)
ax2.grid(True, alpha=0.3, linestyle='--')
ax2.fill_between(eq_live['date'], 0, compute_dd(eq_live['equity']),
                  color=COLORS['baseline'], alpha=0.35, label='LB=41 Drawdown')
ax2.set_xlim(eq_live['date'].min(), eq_live['date'].max())

# ── 5. Metrics annotation box ─────────────────────────────────────────────────
metrics_text = (
    "T86 REGIME_LOOKBACK Sweep Results (exact-live, 248 values tested)\n"
    "───────────────────────────────────────────────────────────────\n"
    f"  LB=8  Winner  → equity 3.3607x  Sharpe 1.220  DD 23.7%  trades 275\n"
    f"  LB=5  Runner1 → equity 3.3565x  Sharpe 1.219  DD 23.7%  trades 270\n"
    f"  LB=10 Runner2 → equity 3.3529x  Sharpe 1.218  DD 23.7%  trades 276\n"
    f"  LB=40 ~baseline → equity 2.9299x  Sharpe 1.061  DD 22.3%  trades 300\n"
    f"  LB=41 Production (current) → equity ~2.76x  Sharpe 1.02\n"
    "───────────────────────────────────────────────────────────────\n"
    f"  ⚠ NOTE: LB=41 NOT tested in sweep. LB=40 is nearest (2.93x).\n"
    f"  T86 sweep run: 2026-05-08 (not yet exact-live verified for LB=8)"
)
fig.text(0.99, 0.01, metrics_text, transform=fig.transFigure,
         fontsize=7.5, va='bottom', ha='right',
         bbox=dict(boxstyle='round,pad=0.4', facecolor='lightyellow', alpha=0.8))

plt.tight_layout(rect=[0, 0.08, 1, 0.97])
plt.savefig(OUT, dpi=150, bbox_inches='tight', facecolor='white')
plt.close()
print(f"Chart saved: {OUT}")
print(f"  Live baseline final equity: {live_final:.4f}x")
print(f"  T86 winner LB=8 final equity: {eq_t86['lb_8'].iloc[-1]:.4f}x")
print(f"  Improvement: {(eq_t86['lb_8'].iloc[-1] - live_final) / live_final * 100:.1f}%")
