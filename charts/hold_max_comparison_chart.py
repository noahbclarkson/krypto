#!/usr/bin/env python3
"""
HOLD_MAX Equity Comparison Chart
Reads: snapshots/hold_max_prod.csv (per-window results)
       snapshots/hold_max_prod_equity.csv (equity time-series)
Chart: Equity curves for key HM candidates + metrics bar chart
"""
import pandas as pd
import matplotlib.pyplot as plt
import matplotlib.gridspec as gridspec
import numpy as np
import os

OUTDIR = '/home/ubuntu/.openclaw/workspace-krypto/krypto/charts'
os.makedirs(OUTDIR, exist_ok=True)

EQ_CSV  = '/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/hold_max_prod_equity.csv'
SUM_CSV = '/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/hold_max_prod_summary.csv'

# ── Load ────────────────────────────────────────────────────────────────────────
eq_df  = pd.read_csv(EQ_CSV)
sum_df = pd.read_csv(SUM_CSV)

print("=== HOLD_MAX Summary (9 universes × 54 windows) ===")
print(sum_df.to_string(index=False))

# ── Candidate HM values to plot ───────────────────────────────────────────────
HM_CANDIDATES = [5, 8, 10, 12, 15, 45, 80]
UNIVERSE      = 'Base5'
WINDOW        = 0  # W0 = mega-bull (2019–2020)

# ── Equity curves ─────────────────────────────────────────────────────────────
curves = {}
for hm in HM_CANDIDATES:
    sub = eq_df[(eq_df['hm'] == hm) & (eq_df['universe'] == UNIVERSE) & (eq_df['window'] == WINDOW)]
    if sub.empty:
        sub = eq_df[(eq_df['hm'] == hm) & (eq_df['universe'] == UNIVERSE) & (eq_df['window'] == 1)]
    if not sub.empty:
        sub = sub.sort_values('step')
        curves[hm] = sub['equity'].values
        print(f"  HM={hm}: {len(curves[hm])} bars, final={curves[hm][-1]:.3f}x")

# ── Metrics ────────────────────────────────────────────────────────────────────
def ann_sharpe(rets):
    if len(rets) < 2: return 0.0
    mn = np.mean(rets); sd = np.std(rets, ddof=0)
    return mn * np.sqrt(365) / sd if sd > 0 else 0.0

metrics = {}
for hm, eq in curves.items():
    rets = np.diff(eq) / eq[:-1]
    rets = rets[~np.isnan(rets) & ~np.isinf(rets)]
    peak = np.maximum.accumulate(eq)
    dd   = (eq / peak - 1) * 100
    metrics[hm] = dict(
        sharpe = ann_sharpe(rets),
        max_dd = dd.min(),
        final  = eq[-1],
    )
    print(f"  HM={hm:>3}: Sharpe={metrics[hm]['sharpe']:>6.2f}, MaxDD={metrics[hm]['max_dd']:>7.2f}%, Final={metrics[hm]['final']:>8.3f}x")

# ── Summary metrics from full 9-universe aggregate ──────────────────────────────
sum_df['sharpe_num'] = sum_df['avg_sharpe']
print("\n=== Global 9-universe summary ===")
for _, row in sum_df.iterrows():
    print(f"  HM={int(row['hm']):>3}: pass={int(row['global_pass'])}/{int(row['global_total'])} ({row['pass_rate']:.1f}%), Sharpe={row['avg_sharpe']:.4f}")

# ── Chart ────────────────────────────────────────────────────────────────────────
fig = plt.figure(figsize=(16, 10))
gs  = gridspec.GridSpec(2, 2, figure=fig, hspace=0.35, wspace=0.30)

ax1 = fig.add_subplot(gs[0, :])   # equity curves (full width top)
ax2 = fig.add_subplot(gs[1, 0])   # Sharpe bar chart
ax3 = fig.add_subplot(gs[1, 1])   # pass rate bar chart

COLORS = ['#E53935','#FB8C00','#FDD835','#43A047','#1E88E5','#8E24AA','#00ACC1']
color_map = {hm: COLORS[i % len(COLORS)] for i, hm in enumerate(sorted(curves.keys()))}

# ── Panel 1: Equity curves (log scale) ────────────────────────────────────────
for hm in sorted(curves.keys()):
    eq = curves[hm]
    m  = metrics[hm]
    label = f"HM={hm:>3}  (Sharpe={m['sharpe']:.2f}, DD={m['max_dd']:.1f}%, {m['final']:.1f}x)"
    ax1.plot(eq, label=label, color=color_map[hm], linewidth=2.0)

ax1.set_yscale('log')
ax1.set_ylabel('Equity (log scale)', fontsize=12)
ax1.set_title(f'HOLD_MAX Equity Comparison — {UNIVERSE} Window {WINDOW}\n'
             f'(CHAND_P=11, CHAND_M=2.25, EP=24, ATR_P=24, ATR_M=2.0)', fontsize=13)
ax1.legend(loc='upper left', fontsize=9, framealpha=0.9)
ax1.grid(True, which='both', ls='--', alpha=0.35)
ax1.set_xlabel('Bar', fontsize=11)

# ── Panel 2: Sharpe bar chart ─────────────────────────────────────────────────
sorted_hms = sorted(sum_df['hm'].astype(int).values)
sharpes    = [sum_df[sum_df['hm']==float(hm)]['avg_sharpe'].values[0] for hm in sorted_hms]
bars2 = ax2.bar([str(h) for h in sorted_hms], sharpes,
                color=[color_map.get(h, '#999') for h in sorted_hms], alpha=0.85)
ax2.set_ylabel('Avg Sharpe (9 universes)', fontsize=11)
ax2.set_xlabel('HOLD_MAX', fontsize=11)
ax2.set_title('Sharpe by HOLD_MAX', fontsize=12)
ax2.grid(True, axis='y', ls='--', alpha=0.35)
# Highlight winner
best_hm = int(sum_df.loc[sum_df['avg_sharpe'].idxmax(), 'hm'])
for bar, hm in zip(bars2, sorted_hms):
    if hm == best_hm:
        bar.set_edgecolor('gold'); bar.set_linewidth(2.5)
        bar.set_label(f'Winner: HM={hm}')

# ── Panel 3: Pass rate bar chart ───────────────────────────────────────────────
pass_rates = [sum_df[sum_df['hm']==float(hm)]['pass_rate'].values[0] for hm in sorted_hms]
bars3 = ax3.bar([str(h) for h in sorted_hms], pass_rates,
                color=[color_map.get(h, '#999') for h in sorted_hms], alpha=0.85)
ax3.set_ylabel('Pass Rate %', fontsize=11)
ax3.set_xlabel('HOLD_MAX', fontsize=11)
ax3.set_title('Pass Rate by HOLD_MAX', fontsize=12)
ax3.set_ylim(0, 105)
ax3.grid(True, axis='y', ls='--', alpha=0.35)
for bar, hm in zip(bars3, sorted_hms):
    if hm == best_hm:
        bar.set_edgecolor('gold'); bar.set_linewidth(2.5)

# ── Caption ────────────────────────────────────────────────────────────────────
winner_row = sum_df[sum_df['hm'] == float(best_hm)].iloc[0]
caption = (f"HOLD_MAX Hyperopt — 9 universes × 54 windows | Winner: HM={best_hm} | "
           f"Pass {int(winner_row['global_pass'])}/{int(winner_row['global_total'])} "
           f"({winner_row['pass_rate']:.1f}%) | Sharpe {winner_row['avg_sharpe']:.4f} | "
           f"Chandelier fires first ~bar 12-15; HM=12 exits just before edge-case whipsaws. "
           f"HM≥35 plateau: Chandelier always fires first.")
fig.text(0.5, 0.01, caption, ha='center', fontsize=9, style='italic',
         color='#444', wrap=True)

out_path = f'{OUTDIR}/hold_max_comparison.png'
plt.savefig(out_path, dpi=200, bbox_inches='tight')
print(f"\nSaved: {out_path}")
plt.close()
