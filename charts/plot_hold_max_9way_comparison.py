#!/usr/bin/env python3
"""
HOLD_MAX 9-Universe Hyperopt Comparison Chart
Generates: charts/hold_max_9way_comparison.png

Reads: snapshots/hold_max_9way_summary.csv
       snapshots/hold_max_9way_equity.csv
       snapshots/hold_max_9way.csv
"""
import pandas as pd
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import matplotlib.ticker as mticker
import os

os.chdir("/home/ubuntu/.openclaw/workspace-krypto/krypto")

SUMMARY_CSV = "snapshots/hold_max_9way_summary.csv"
METRICS_CSV = "snapshots/hold_max_9way.csv"
EQUITY_CSV  = "snapshots/hold_max_9way_equity.csv"
OUT_PNG     = "charts/hold_max_9way_comparison.png"
os.makedirs("charts", exist_ok=True)

# ── Load data ────────────────────────────────────────────────────────────────
summary = pd.read_csv(SUMMARY_CSV)
metrics = pd.read_csv(METRICS_CSV)

print(f"Summary rows: {len(summary)}")
print(f"Metrics rows: {len(metrics)}")
print(summary[['hm','pass_rate','avg_sharpe','avg_ret','avg_dd','global_pass','global_total']].to_string())

# ── Identify Winner, Baseline, Runner-up ────────────────────────────────────
# Winner = highest avg Sharpe
summary = summary.sort_values('avg_sharpe', ascending=False).reset_index(drop=True)
baseline_hm = 45
winner_hm   = int(summary.iloc[0]['hm'])
# Runner-up = next-highest Sharpe that's not winner or baseline
runnerup_row = summary[(summary['hm'] != winner_hm) & (summary['hm'] != baseline_hm)].iloc[0]
runnerup_hm  = int(runnerup_row['hm'])

print(f"\nBaseline HM={baseline_hm}")
print(f"Winner    HM={winner_hm}")
print(f"Runner-up HM={runnerup_hm}")

wrow = summary[summary['hm'] == winner_hm].iloc[0]
brow = summary[summary['hm'] == baseline_hm].iloc[0]
rrow = summary[summary['hm'] == runnerup_hm].iloc[0]

# ── Color scheme ──────────────────────────────────────────────────────────────
colors_pool  = ['#3fb950', '#f0883e', '#58a6ff']
line_pool    = ['-', '--', ':']
candidates   = {
    f'Winner (HM={winner_hm})':     (winner_hm,   colors_pool[0], line_pool[0]),
    f'Baseline (HM={baseline_hm})': (baseline_hm, colors_pool[1], line_pool[1]),
    f'Runner-up (HM={runnerup_hm})':(runnerup_hm,  colors_pool[2], line_pool[2]),
}

# ── Figure ───────────────────────────────────────────────────────────────────
fig = plt.figure(figsize=(18, 12))
fig.patch.set_facecolor('#0d1117')

# Use GridSpec for more control
from matplotlib.gridspec import GridSpec
gs = GridSpec(3, 3, figure=fig, hspace=0.45, wspace=0.35)

def dark_axes(ax):
    ax.set_facecolor('#0d1117')
    ax.tick_params(colors='white', labelsize=9)
    ax.xaxis.label.set_color('white')
    ax.yaxis.label.set_color('white')
    ax.title.set_color('white')
    ax.grid(True, alpha=0.15, color='white', linewidth=0.5)
    ax.spines['bottom'].set_color('#30363d')
    ax.spines['left'].set_color('#30363d')
    ax.spines['top'].set_visible(False)
    ax.spines['right'].set_visible(False)
    return ax

# ── Panel 1: Sharpe vs HM (all values) ───────────────────────────────────────
ax1 = fig.add_subplot(gs[0, :2])
all_hms_sorted = sorted(summary['hm'].unique())
all_sharpes    = [float(summary[summary['hm'] == h]['avg_sharpe'].values[0]) for h in all_hms_sorted]
all_pass_rates = [float(summary[summary['hm'] == h]['pass_rate'].values[0]) for h in all_hms_sorted]

ax1.plot(all_hms_sorted, all_sharpes, color='#58a6ff', linewidth=2.5,
         marker='o', markersize=4, label='Avg Sharpe (9 universes)')
ax1.axvline(winner_hm,   color='#3fb950', linestyle='--', alpha=0.8, linewidth=1.5, label=f'Winner HM={winner_hm}')
ax1.axvline(baseline_hm, color='#f0883e', linestyle='--', alpha=0.8, linewidth=1.5, label=f'Baseline HM={baseline_hm}')

# Shade pass/fail regions
ax1.fill_between(all_hms_sorted, 0, all_sharpes, alpha=0.08, color='#58a6ff')
ax1.set_xlabel('HOLD_MAX (bars)', fontsize=11)
ax1.set_ylabel('Annualised Sharpe', fontsize=11)
ax1.set_title('HOLD_MAX Sweep: Sharpe vs Parameter\nP=15/M=1.50 · 9 Universes · All Windows', fontsize=12, pad=8)
ax1.legend(loc='upper right', facecolor='#161b22', edgecolor='#30363d', labelcolor='white', fontsize=9)

ax1b = ax1.twinx()
ax1b.bar(all_hms_sorted, all_pass_rates, alpha=0.18, color='#8b949e', width=4, label='Pass Rate %')
ax1b.set_ylabel('Pass Rate (%)', color='#8b949e', fontsize=10)
ax1b.tick_params(axis='y', labelcolor='#8b949e')
ax1b.set_ylim(0, 120)
dark_axes(ax1)

# ── Panel 2: Key metrics bar chart ────────────────────────────────────────────
ax2 = fig.add_subplot(gs[0, 2])
xlabs = list(candidates.keys())
n = len(xlabs)
bar_w = 0.22
sharpes_v  = [float(summary[summary['hm'] == v[0]]['avg_sharpe'].values[0]) for _, v in candidates.items()]
passes_v   = [float(summary[summary['hm'] == v[0]]['pass_rate'].values[0]) for _, v in candidates.items()]
inv_dds    = [100.0 / float(summary[summary['hm'] == v[0]]['avg_dd'].values[0]) / 10 for _, v in candidates.items()]

j_positions = range(n)
ax2.bar([j - bar_w for j in j_positions], sharpes_v,  width=bar_w, label='Sharpe',   alpha=0.85, color=[v[1] for _, v in candidates.items()])
ax2.bar(j_positions,              passes_v,   width=bar_w, label='Pass%',    alpha=0.85, color=[v[1] for _, v in candidates.items()], hatch='//')
ax2.bar([j + bar_w for j in j_positions], inv_dds,   width=bar_w, label='100/DD/10', alpha=0.85, color=[v[1] for _, v in candidates.items()], hatch='xx')
ax2.set_xticks(j_positions)
ax2.set_xticklabels([k.replace(' (', '\n(') for k in xlabs], fontsize=8)
ax2.set_ylabel('Value', fontsize=10)
ax2.set_title('Key Metrics\nWinner vs Baseline vs Runner-up', fontsize=11)
ax2.legend(loc='upper right', facecolor='#161b22', edgecolor='#30363d', labelcolor='white', fontsize=8)
dark_axes(ax2)

# ── Panel 3: Equity across windows (Base5) ────────────────────────────────────
ax3 = fig.add_subplot(gs[1, :2])
for label, (hm_val, color, ls) in candidates.items():
    wdata = metrics[(metrics['hm'] == hm_val) & (metrics['universe'] == 'Base5')].sort_values('window')
    eq = wdata['equity_final'].values
    cum = [1.0]
    for e in eq:
        cum.append(cum[-1] * e)
    windows = list(wdata['window'].values)
    ax3.plot(range(len(windows)), cum[1:], label=label,
             color=color, linewidth=2.5, linestyle=ls, marker='o', markersize=5)

ax3.set_xlabel('Walk-Forward Window', fontsize=11)
ax3.set_ylabel('Cumulative Equity (×)', fontsize=11)
ax3.set_title('Equity Across Walk-Forward Windows (Base5)\nCumulative compounded', fontsize=12, pad=8)
ax3.set_xticks(range(len(windows)))
ax3.set_xticklabels([f'W{w}' for w in windows], fontsize=9)
ax3.yaxis.set_major_formatter(mticker.FormatStrFormatter('%.1fx'))
ax3.legend(loc='upper left', facecolor='#161b22', edgecolor='#30363d', labelcolor='white', fontsize=9)
dark_axes(ax3)

# ── Panel 4: Global equity (all 9 universes, pooled) ──────────────────────────
ax4 = fig.add_subplot(gs[1, 2])
for label, (hm_val, color, ls) in candidates.items():
    wdata = metrics[metrics['hm'] == hm_val].sort_values(['universe', 'window'])
    eq_by_uni = []
    for uni in wdata['universe'].unique():
        uni_eq = wdata[wdata['universe'] == uni]['equity_final'].values
        cum = [1.0]
        for e in uni_eq:
            cum.append(cum[-1] * e)
        eq_by_uni.append(cum[-1])
    overall_cum = 1.0
    for e in eq_by_uni:
        overall_cum *= e
    # Just show per-universe final equities as bar
    ax4.bar([label.split('(')[1].rstrip(')') + f'\n({overall_cum:.1f}x)'],
            [overall_cum], color=color, alpha=0.8)

ax4.set_ylabel('Cumulative Equity (×)', fontsize=10)
ax4.set_title('Final Equity\n(9 universes compounded)', fontsize=11)
ax4.tick_params(axis='x', rotation=15, labelsize=8)
dark_axes(ax4)

# ── Panel 5: Global pass rate bar chart ──────────────────────────────────────
ax5 = fig.add_subplot(gs[2, :])
x_pos = range(len(all_hms_sorted))
bar_colors = ['#3fb950' if h == winner_hm else '#f0883e' if h == baseline_hm else '#8b949e'
              for h in all_hms_sorted]
ax5.bar(x_pos, all_pass_rates, color=bar_colors, alpha=0.85, width=3)
ax5.axhline(70, color='#f85149', linestyle=':', alpha=0.7, label='70% threshold')
ax5.axhline(80, color='#58a6ff', linestyle=':', alpha=0.7, label='80% threshold')
ax5.set_xticks(x_pos)
ax5.set_xticklabels([str(h) for h in all_hms_sorted], fontsize=9)
ax5.set_xlabel('HOLD_MAX (bars)', fontsize=11)
ax5.set_ylabel('Global Pass Rate (%)', fontsize=11)
ax5.set_title('Global Pass Rate by HOLD_MAX (9 Universes)', fontsize=12, pad=8)
ax5.legend(loc='upper right', facecolor='#161b22', edgecolor='#30363d', labelcolor='white', fontsize=9)
ax5.set_ylim(0, 105)
dark_axes(ax5)

# ── Annotations ───────────────────────────────────────────────────────────────
for ax in [ax1, ax3, ax5]:
    for spine in ax.spines.values():
        spine.set_visible(False)
    ax.spines['bottom'].set_visible(True)
    ax.spines['left'].set_visible(True)

fig.text(0.99, 0.01,
         f'P=15/M=1.50 · EP=21 · ATR(24,2.0) · CAP=3 · 9 Universes · Walk-Forward 252/252',
         ha='right', va='bottom', fontsize=8, color='#8b949e', style='italic')

plt.savefig(OUT_PNG, dpi=150, bbox_inches='tight', facecolor='#0d1117', edgecolor='none')
print(f"\nSaved: {OUT_PNG}")

# ── Print summary ──────────────────────────────────────────────────────────────
print(f"""
=== HOLD_MAX 9-WAY HYPEROPT SUMMARY ===
WINNER:    HM={winner_hm} | Pass {int(wrow['global_pass'])}/{int(wrow['global_total'])} ({wrow['pass_rate']:.1f}%) | Sharpe {wrow['avg_sharpe']:.4f} | Ret {wrow['avg_ret']:+.1f}% | DD {wrow['avg_dd']:.1f}%
BASELINE:  HM={baseline_hm} | Pass {int(brow['global_pass'])}/{int(brow['global_total'])} ({brow['pass_rate']:.1f}%) | Sharpe {brow['avg_sharpe']:.4f} | Ret {brow['avg_ret']:+.1f}% | DD {brow['avg_dd']:.1f}%
RUNNER-UP: HM={runnerup_hm} | Pass {int(rrow['global_pass'])}/{int(rrow['global_total'])} ({rrow['pass_rate']:.1f}%) | Sharpe {rrow['avg_sharpe']:.4f} | Ret {rrow['avg_ret']:+.1f}% | DD {rrow['avg_dd']:.1f}%
""")
