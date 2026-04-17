#!/usr/bin/env python3
"""Plot VOL_LOOKBACK hyperopt results: equity curves + Sharpe heatmap."""
import csv
import os
import sys

import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import matplotlib.ticker as mticker
import numpy as np

OUTDIR = '/home/ubuntu/.openclaw/workspace-krypto/krypto/charts'
os.makedirs(OUTDIR, exist_ok=True)

BASE_DIR = '/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots'

# ── 1. Equity curve data ─────────────────────────────────────────────────────
def read_equity_csv(path):
    days, equities = [], []
    with open(path) as f:
        reader = csv.reader(f)
        next(reader)  # header
        for row in reader:
            days.append(float(row[0]))
            equities.append(float(row[1]))
    return np.array(days), np.array(equities)

baseline_days, baseline_eq = read_equity_csv(f'{BASE_DIR}/vol_ema_extended_hyperopt_baseline.csv')
winner_days, winner_eq = read_equity_csv(f'{BASE_DIR}/vol_ema_extended_hyperopt_winner.csv')
ru1_days, ru1_eq = read_equity_csv(f'{BASE_DIR}/vol_ema_extended_hyperopt_runnerup1.csv')
ru2_days, ru2_eq = read_equity_csv(f'{BASE_DIR}/vol_ema_extended_hyperopt_runnerup2.csv')

# ── 2. Sharpe sweep data ──────────────────────────────────────────────────────
sma_sharme = {}  # vl -> avg_sharpe_nodoge
ema_sharme = {}
with open(f'{BASE_DIR}/vol_ema_extended_hyperopt.csv') as f:
    reader = csv.DictReader(f)
    for row in reader:
        vl = int(row['vl'])
        method = row['method'].strip()
        sh = float(row['avg_sharpe_nodoge'])
        if method == 'SMA':
            sma_sharme[vl] = sh
        else:
            ema_sharme[vl] = sh

vls = sorted(set(sma_sharme.keys()) | set(ema_sharme.keys()))
sma_vals = [sma_sharme.get(v, None) for v in vls]
ema_vals = [ema_sharme.get(v, None) for v in vls]

# ── 3. Summary table ──────────────────────────────────────────────────────────
summary = []
with open(f'{BASE_DIR}/vol_ema_extended_hyperopt.csv') as f:
    reader = csv.DictReader(f)
    for row in reader:
        summary.append({
            'vl': int(row['vl']),
            'method': row['method'].strip(),
            'nodoge_sharpe': float(row['avg_sharpe_nodoge']),
            'nodoge_pass': float(row['pass_rate_nodoge']),
            'total_w': int(row['total_windows']),
            'sh9w': float(row['9way_avg_sharpe']),
            'ret9w': float(row['9way_avg_ret']),
            'pass9w': int(row['9way_pass_sum']),
            'total9w': int(row['9way_total_sum']),
        })
summary.sort(key=lambda x: -x['sh9w'])

WINNER = summary[0]
RUNNERUP1 = summary[1]
RUNNERUP2 = summary[2]
BASELINE_VL1 = {'vl': 1, 'method': 'SMA', 'nodoge_sharpe': 7.5007, 'sh9w': 5.93, 'ret9w': 99.2, 'pass9w': 41, 'total9w': 54}
BASELINE_VL2 = {'vl': 2, 'method': 'SMA', 'nodoge_sharpe': 6.4132, 'sh9w': 6.4132, 'ret9w': 123.7, 'pass9w': 47, 'total9w': 54}

# ── Build figure ──────────────────────────────────────────────────────────────
fig = plt.figure(figsize=(16, 14))
fig.patch.set_facecolor('#0d1117')
axes_color = '#161b22'
text_color = '#c9d1d9'
grid_color = '#21262d'
accent_sma = '#58a6ff'
accent_ema = '#f78166'
accent_win = '#3fb950'
accent_base = '#8b949e'
accent_ru1 = '#d29922'
accent_ru2 = '#a371f7'

def configure_axis(ax):
    ax.set_facecolor(axes_color)
    ax.tick_params(colors=text_color)
    ax.xaxis.label.set_color(text_color)
    ax.yaxis.label.set_color(text_color)
    ax.title.set_color(text_color)
    ax.spines['bottom'].set_color(grid_color)
    ax.spines['left'].set_color(grid_color)
    ax.spines['top'].set_visible(False)
    ax.spines['right'].set_visible(False)
    ax.grid(True, color=grid_color, linestyle='--', linewidth=0.5)

# ── Panel 1: Equity curves (log scale) ──────────────────────────────────────
ax1 = fig.add_subplot(3, 1, 1)
configure_axis(ax1)

ax1.plot(baseline_days, baseline_eq, color=accent_base, linewidth=1.5, alpha=0.8, label=f'Baseline VL=1 (SMA)')
ax1.plot(winner_days, winner_eq, color=accent_win, linewidth=2.0, label=f'Winner VL={WINNER["vl"]} ({WINNER["method"]}), Sharpe={WINNER["sh9w"]:.3f}')
ax1.plot(ru1_days, ru1_eq, color=accent_ru1, linewidth=1.5, alpha=0.8, linestyle='--', label=f'Runner-up1 VL={RUNNERUP1["vl"]} ({RUNNERUP1["method"]}), Sharpe={RUNNERUP1["sh9w"]:.3f}')
ax1.plot(ru2_days, ru2_eq, color=accent_ru2, linewidth=1.5, alpha=0.8, linestyle=':', label=f'Runner-up2 VL={RUNNERUP2["vl"]} ({RUNNERUP2["method"]}), Sharpe={RUNNERUP2["sh9w"]:.3f}')

ax1.set_yscale('log')
ax1.set_ylabel('Equity (log scale)', fontsize=11)
ax1.set_title('VOL_LOOKBACK Hyperopt: Equity Curves — NoDOGE Universe (6 walk-forward windows)', fontsize=13, fontweight='bold', pad=12)
ax1.legend(loc='upper left', fontsize=9, framealpha=0.3, facecolor=axes_color, labelcolor=text_color)
ax1.set_xlim(0, max(len(baseline_eq), len(winner_eq)))

# Annotation for final values
ax1.annotate(f'Final: {baseline_eq[-1]:.2f}x', xy=(baseline_days[-1], baseline_eq[-1]),
             xytext=(5, 0), textcoords='offset points', color=accent_base, fontsize=8)
ax1.annotate(f'Final: {winner_eq[-1]:.2f}x', xy=(winner_days[-1], winner_eq[-1]),
             xytext=(5, 0), textcoords='offset points', color=accent_win, fontsize=8)

# ── Panel 2: Full Sharpe sweep SMA vs EMA ─────────────────────────────────────
ax2 = fig.add_subplot(3, 1, 2)
configure_axis(ax2)

vl_arr = np.array(vls)
sma_arr = np.array(sma_vals)
ema_arr = np.array(ema_vals)

# Shade the plateau region (VL=53-61 where SMA peaks)
plateau_mask = (vl_arr >= 53) & (vl_arr <= 61)
if plateau_mask.any():
    ax2.fill_between(vl_arr[plateau_mask], 0, sma_arr[plateau_mask], alpha=0.15, color=accent_sma)

ax2.plot(vl_arr, sma_arr, color=accent_sma, linewidth=2.0, label='SMA smoothing', marker='o', markersize=2, markevery=5)
ax2.plot(vl_arr, ema_arr, color=accent_ema, linewidth=1.5, alpha=0.7, label='EMA smoothing', marker='s', markersize=2, markevery=5)

# Mark winner
ax2.axvline(x=WINNER['vl'], color=accent_win, linewidth=1.5, linestyle='--', alpha=0.8)
ax2.scatter([WINNER['vl']], [WINNER['nodoge_sharpe']], color=accent_win, s=80, zorder=5, marker='*', label=f'Winner: VL={WINNER["vl"]} SMA (Sharpe={WINNER["nodoge_sharpe"]:.2f})')
ax2.scatter([1], [BASELINE_VL1['nodoge_sharpe']], color=accent_base, s=60, zorder=5, marker='D', label=f'Baseline VL=1 (Sharpe={BASELINE_VL1["nodoge_sharpe"]:.2f})')

ax2.set_xlabel('VOL_LOOKBACK (smoothing window, bars)', fontsize=11)
ax2.set_ylabel('Avg OOS Sharpe (NoDOGE)', fontsize=11)
ax2.set_title('Full Range Sharpe Sweep: SMA vs EMA (VL=1-100, step 1)', fontsize=13, fontweight='bold', pad=12)
ax2.legend(loc='upper right', fontsize=9, framealpha=0.3, facecolor=axes_color, labelcolor=text_color)
ax2.set_xlim(0, 101)

# ── Panel 3: Metrics table + key insight text ─────────────────────────────────
ax3 = fig.add_subplot(3, 1, 3)
configure_axis(ax3)
ax3.set_xlim(0, 1)
ax3.set_ylim(0, 1)
ax3.axis('off')

# Table data
table_data = [
    ['Config', 'VL', 'Method', 'NoDOGE Sharpe', '9w Sharpe', '9w Pass Rate', '9w Avg Ret'],
    ['Baseline (VL=1)', 1, 'SMA', f'{BASELINE_VL1["nodoge_sharpe"]:.4}', '~5.93', f'{BASELINE_VL1["pass9w"]}/{BASELINE_VL1["total9w"]} (76%)', f'{BASELINE_VL1["ret9w"]:+.1}%'],
    ['Prior best (VL=2)', 2, 'SMA', f'{BASELINE_VL2["nodoge_sharpe"]:.4}', f'{BASELINE_VL2["sh9w"]:.4}', f'{BASELINE_VL2["pass9w"]}/{BASELINE_VL2["total9w"]} (87%)', f'{BASELINE_VL2["ret9w"]:+.1}%'],
    [f'★ WINNER', WINNER['vl'], WINNER['method'], f'{WINNER["nodoge_sharpe"]:.4}', f'{WINNER["sh9w"]:.4}',
     f'{WINNER["pass9w"]}/{WINNER["total9w"]} ({WINNER["pass9w"]/WINNER["total9w"]*100:.0f}%)', f'{WINNER["ret9w"]:+.1}%'],
    ['Runner-up 1', RUNNERUP1['vl'], RUNNERUP1['method'], f'{RUNNERUP1["nodoge_sharpe"]:.4}', f'{RUNNERUP1["sh9w"]:.4}',
     f'{RUNNERUP1["pass9w"]}/{RUNNERUP1["total9w"]} ({RUNNERUP1["pass9w"]/RUNNERUP1["total9w"]*100:.0f}%)', f'{RUNNERUP1["ret9w"]:+.1}%'],
    ['Runner-up 2', RUNNERUP2['vl'], RUNNERUP2['method'], f'{RUNNERUP2["nodoge_sharpe"]:.4}', f'{RUNNERUP2["sh9w"]:.4}',
     f'{RUNNERUP2["pass9w"]}/{RUNNERUP2["total9w"]} ({RUNNERUP2["pass9w"]/RUNNERUP2["total9w"]*100:.0f}%)', f'{RUNNERUP2["ret9w"]:+.1}%'],
]

col_widths = [0.22, 0.07, 0.10, 0.18, 0.15, 0.18, 0.14]
col_starts = [0.01]
for w in col_widths[:-1]:
    col_starts.append(col_starts[-1] + w)

# Header row
y = 0.90
for ci, (hdr, cs, cw) in enumerate(zip(table_data[0], col_starts, col_widths)):
    ax3.text(cs + cw/2, y, hdr, ha='center', va='center', fontsize=8.5, fontweight='bold', color=text_color)
ax3.axhline(y=y - 0.025, xmin=0.01, xmax=0.99, color=grid_color, linewidth=0.5)

# Data rows
row_colors = [accent_base, '#6e7681', accent_win, accent_ru1, accent_ru2]
for ri, row in enumerate(table_data[1:]):
    y = 0.90 - (ri + 1) * 0.075
    row_col = row_colors[ri]
    for ci, (cell, cs, cw) in enumerate(zip(row, col_starts, col_widths)):
        ax3.text(cs + cw/2, y, str(cell), ha='center', va='center', fontsize=8.5, color=row_col if ci > 0 else text_color)
    ax3.axhline(y=y - 0.025, xmin=0.01, xmax=0.99, color=grid_color, linewidth=0.3)

# Key insight text
insight_y = 0.50
ax3.text(0.5, insight_y,
    'KEY FINDINGS',
    ha='center', va='center', fontsize=11, fontweight='bold', color=text_color, transform=ax3.transAxes)
insight_y -= 0.07
insights = [
    f'★ WINNER: VL=55 SMA — 9-way avg Sharpe 6.999 (+9.1% vs VL=2 baseline 6.41), pass 45/54 (83%)',
    f'  Plateau region: VL=53-61 SMA all share peak NoDOGE Sharpe (~11.45) — robust optimum',
    f'  EMA never beats SMA — exponential smoothing adds no value for DV ranking (all EMA < SMA)',
    f'  Extended range test (1-100): optimum sits at VL=53-61, well beyond prior test range (1-14)',
    f'  Trade-off: VL=55 lower pass rate (83% vs VL=2 87%) — VL=2 may remain preferred for robustness',
    f'  Previous VL=1 hardcoded default: NoDOGE Sharpe 7.50, 9-way ~5.93 — leaving ~18% Sharpe on table',
]
for line in insights:
    ax3.text(0.5, insight_y, line, ha='center', va='center', fontsize=8.5,
             color=text_color, transform=ax3.transAxes)
    insight_y -= 0.055

ax3.set_title('Results Summary + Key Insights', fontsize=11, fontweight='bold', pad=8)

plt.tight_layout(pad=2.0)
out_path = f'{OUTDIR}/vol_ema_comparison.png'
plt.savefig(out_path, dpi=150, bbox_inches='tight', facecolor=fig.get_facecolor())
plt.close()
print(f"Saved: {out_path}")
sys.exit(0)
