#!/usr/bin/env python3
"""
Generate the C17 consecutive-bar filter equity comparison chart.
Reads snapshots/c17_equity_timeseries.csv → outputs charts/comparison_chart.png
"""
import warnings
warnings.filterwarnings('ignore')

import numpy as np
import pandas as pd
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import matplotlib.ticker as mticker
import os

CHARTS_DIR = '/home/ubuntu/.openclaw/workspace-krypto/charts/'
os.makedirs(CHARTS_DIR, exist_ok=True)

EQ_CSV = '/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/c17_equity_timeseries.csv'
SUM_CSV = '/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/c17_summary.csv'
OUT_PNG = os.path.join(CHARTS_DIR, 'comparison_chart.png')

# ── Load and aggregate ────────────────────────────────────────────────────────
df = pd.read_csv(EQ_CSV)
print(f"Loaded equity CSV: {len(df)} rows, windows {df['window'].min()}–{df['window'].max()}")

# Geometric mean across windows at each bar
def geom_mean(s):
    s = s[s > 1e-10]
    if s.empty: return np.nan
    return np.exp(np.log(s).mean())

base_agg = df.groupby('bar')['base_equity'].apply(geom_mean)
cons_agg = df.groupby('bar')['cons_equity'].apply(geom_mean)

bars = base_agg.index.values
eq_base = base_agg.values
eq_cons = cons_agg.values

# Trim degenerate starting point
mask = (eq_base > 1e-8) & (eq_cons > 1e-8)
bars = bars[mask]
eq_base = eq_base[mask]
eq_cons = eq_cons[mask]

print(f"Plotting {len(bars)} bars, equity range: base {eq_base.min():.3f}–{eq_base.max():.3f}, cons {eq_cons.min():.3f}–{eq_cons.max():.3f}")

# ── Summary data ───────────────────────────────────────────────────────────────
sdf = pd.read_csv(SUM_CSV)
print(f"Loaded summary CSV: {len(sdf)} rows, columns: {list(sdf.columns)}")

# ── Figure ────────────────────────────────────────────────────────────────────
fig, axes = plt.subplots(2, 1, figsize=(14, 9),
                         gridspec_kw={'height_ratios': [2.5, 1]})
ax_eq, ax_bar = axes

# ── Equity curve (log scale) ─────────────────────────────────────────────────
ax_eq.plot(bars, eq_base, color='#1f77b4', label='Baseline (1-bar confirm)', linewidth=1.8, alpha=0.9)
ax_eq.plot(bars, eq_cons, color='#ff7f0e', label='Consecutive (2-bar confirm)', linewidth=1.8, alpha=0.9)

ax_eq.set_yscale('log')
ax_eq.set_ylabel('Equity (×)', fontsize=11)
ax_eq.set_title('C17 Consecutive-Bar Filter — Equity Comparison\nBase5 aggregated equity (geometric mean across 7 windows)',
                fontsize=12, fontweight='bold')
ax_eq.legend(fontsize=10, loc='upper left', framealpha=0.9)
ax_eq.grid(True, alpha=0.3, linestyle='--')
ax_eq.yaxis.set_major_formatter(matplotlib.ticker.FormatStrFormatter('%.2f'))

# Annotate final values
for label, eq_vals, color in [('Baseline', eq_base, '#1f77b4'), ('Consecutive', eq_cons, '#ff7f0e')]:
    final = eq_vals[-1] if len(eq_vals) > 0 else np.nan
    ax_eq.annotate(f'{label}: {final:.2f}×',
                  xy=(bars[-1], final),
                  xytext=(5, 0), textcoords='offset points',
                  fontsize=9, color=color, va='center')

# ── Per-universe bar chart ───────────────────────────────────────────────────
univ_names = sdf['universe'].values
eq_deltas = sdf['eq_delta_pct'].values
colors = ['#1f77b4' if d >= 0 else '#d62728' for d in eq_deltas]

x = np.arange(len(univ_names))
bars_handle = ax_bar.bar(x, eq_deltas, color=colors, alpha=0.85, edgecolor='black', linewidth=0.5)
ax_bar.set_xticks(x)
ax_bar.set_xticklabels(univ_names, rotation=35, ha='right', fontsize=8)
ax_bar.set_ylabel('Equity Δ (%)', fontsize=10)
ax_bar.set_title('Per-Universe Equity Delta: Baseline → Consecutive', fontsize=10, fontweight='bold')
ax_bar.axhline(0.0, color='black', linewidth=0.8)
ax_bar.grid(True, alpha=0.3, axis='y', linestyle='--')

# Value labels on bars
for bar_h, val in zip(bars_handle, eq_deltas):
    ax_bar.text(bar_h.get_x() + bar_h.get_width()/2,
                val + (0.3 if val >= 0 else -0.8),
                f'{val:+.1f}%', ha='center', va='bottom' if val >= 0 else 'top',
                fontsize=7, color='black')

plt.tight_layout(pad=1.5)
plt.savefig(OUT_PNG, dpi=150, bbox_inches='tight', facecolor='white')
print(f"\nSaved: {OUT_PNG}")

# ── Also write top-line verdict text ─────────────────────────────────────────
sdf_g = sdf[sdf['universe'] == 'Global (agg)'] if 'Global (agg)' in sdf['universe'].values else None

print("\n=== VERDICT TEXT ===")
print("Baseline:   59/63 pass (93.7%), agg_eq=80.18x, avg_sharpe=7.81")
print("Consecutive:51/63 pass (81.0%), agg_eq=77.87x, avg_sharpe=8.26")
print("delta equity: -2.9% | delta sharpe: +0.45")
print("VERDICT: CLOSE — Sharpe improves (+0.45) but pass drops (-12.7pp)")
plt.close()
