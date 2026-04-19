#!/usr/bin/env python3
"""
HOLD_MAX Sweep Comparison Chart
Generates: charts/hm_sweep_comparison.png
"""
import pandas as pd
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import matplotlib.ticker as mticker
import os

CSV = "snapshots/hm_sweep_p15m150.csv"
OUT = "charts/hm_sweep_comparison.png"
os.makedirs("charts", exist_ok=True)

df = pd.read_csv(CSV)
print(f"Loaded {len(df)} rows")

# Aggregate by HM
agg = df.groupby('hm').agg(
    avg_ret=('return_pct', 'mean'),
    avg_sharpe=('sharpe', 'mean'),
    avg_dd=('max_dd_pct', 'mean'),
    total_trades=('trades', 'mean'),
    pass_count=('pass', lambda x: (x == True).sum()),
    n_windows=('window', 'count'),
).reset_index()
agg['pass_rate'] = agg['pass_count'] / agg['n_windows'] * 100
agg = agg.sort_values('avg_sharpe', ascending=False).reset_index(drop=True)
print(agg[['hm','avg_sharpe','avg_ret','avg_dd','pass_rate']].to_string())

baseline_hm = 45
winner_hm = int(agg.iloc[0]['hm'])
# Runner-up: highest Sharpe among non-winner, non-baseline HMs
runnerup_hm = int(agg[~agg['hm'].isin([winner_hm, baseline_hm])].iloc[0]['hm'])

print(f"\nWinner HM={winner_hm}, Baseline HM={baseline_hm}, Runner-up HM={runnerup_hm}")

candidates = {
    f'Winner (HM={winner_hm})': winner_hm,
    f'Baseline (HM={baseline_hm})': baseline_hm,
    f'Runner-up (HM={runnerup_hm})': runnerup_hm,
}

colors_pool = ['#3fb950', '#f0883e', '#58a6ff']
line_pool = ['-', '--', '-.']
colors = {}
lines = {}
for i, (label, _) in enumerate(candidates.items()):
    colors[label] = colors_pool[i % len(colors_pool)]
    lines[label] = line_pool[i % len(line_pool)]

fig, axes = plt.subplots(1, 3, figsize=(18, 6))
fig.patch.set_facecolor('#0d1117')
for ax in axes:
    ax.set_facecolor('#0d1117')
    ax.tick_params(colors='white')
    ax.xaxis.label.set_color('white')
    ax.yaxis.label.set_color('white')
    ax.title.set_color('white')
    ax.grid(True, alpha=0.15, color='white')
    ax.spines['bottom'].set_color('#30363d')
    ax.spines['left'].set_color('#30363d')
    ax.spines['top'].set_visible(False)
    ax.spines['right'].set_visible(False)

# ── Panel 1: Sharpe vs HM ──────────────────────────────────────────────────
ax1 = axes[0]
all_hms = sorted(agg['hm'].unique())
all_sharpes = [float(agg[agg['hm'] == h]['avg_sharpe'].values[0]) for h in all_hms]
all_pass = [float(agg[agg['hm'] == h]['pass_rate'].values[0]) for h in all_hms]

ax1.plot(all_hms, all_sharpes, color='#58a6ff', linewidth=2.5, marker='o', markersize=5, label='Avg Sharpe')
ax1.set_xlabel('HOLD_MAX (bars)', fontsize=11)
ax1.set_ylabel('Annualised Sharpe', fontsize=11, color='#58a6ff')
ax1.tick_params(axis='y', labelcolor='#58a6ff')

# Shaded plateau region
hm_plateau_start = min([h for h in all_hms if h >= 25])
hm_plateau_end = max([h for h in all_hms if h <= 180])
plateau_sharpe = all_sharpes[all_hms.index(hm_plateau_start)]
ax1.axhline(plateau_sharpe, color='#8b949e', linestyle=':', alpha=0.5, label=f'Plateau {plateau_sharpe:.2f}')

ax1.axvline(winner_hm, color='#3fb950', linestyle='--', alpha=0.7, label=f'Winner HM={winner_hm}')
ax1.axvline(baseline_hm, color='#f0883e', linestyle='--', alpha=0.7, label=f'Baseline HM={baseline_hm}')
ax1.set_title('HOLD_MAX Sweep: Sharpe vs Parameter\nP=15/M=1.50 · Base5 · 6 windows', fontsize=12, pad=10)
ax1.legend(loc='upper right', facecolor='#161b22', edgecolor='#30363d', labelcolor='white', fontsize=9)

ax1b = ax1.twinx()
ax1b.bar(all_hms, all_pass, alpha=0.15, color='#8b949e', width=5, label='Pass rate %')
ax1b.set_ylabel('Pass Rate (%)', color='#8b949e', fontsize=10)
ax1b.tick_params(axis='y', labelcolor='#8b949e')
ax1b.set_ylim(0, 120)

# ── Panel 2: Equity progression across windows ────────────────────────────────
ax2 = axes[1]
for label, hm_val in candidates.items():
    wdata = df[(df['hm'] == hm_val) & (df['universe'] == 'Base5')].sort_values('window')
    eq = wdata['equity_final'].values
    cum = [1.0]
    for e in eq:
        cum.append(cum[-1] * e)
    windows = list(wdata['window'].values)
    ax2.plot(range(len(windows)), cum[1:], label=label,
             color=colors[label], linewidth=2.5, linestyle=lines[label],
             marker='o', markersize=5)

ax2.set_xlabel('Walk-Forward Window', fontsize=11)
ax2.set_ylabel('Cumulative Equity (×)', fontsize=11)
ax2.set_title('Equity Across Walk-Forward Windows\n(Base5 Cumulative)', fontsize=12, pad=10)
ax2.set_xticks(range(len(windows)))
ax2.set_xticklabels([f'W{w}' for w in windows], fontsize=9)
ax2.yaxis.set_major_formatter(mticker.FormatStrFormatter('%.1fx'))
ax2.legend(loc='upper left', facecolor='#161b22', edgecolor='#30363d', labelcolor='white', fontsize=9)

# ── Panel 3: Metrics bar chart ────────────────────────────────────────────────
ax3 = axes[2]
xlabels = list(candidates.keys())
n = len(xlabels)
bar_w = 0.22

sharpes = [float(agg[agg['hm'] == candidates[l]]['avg_sharpe'].values[0]) for l in xlabels]
rets = [float(agg[agg['hm'] == candidates[l]]['avg_ret'].values[0]) / 10 for l in xlabels]
passes = [float(agg[agg['hm'] == candidates[l]]['pass_rate'].values[0]) / 10 for l in xlabels]
inv_dd = [10 / float(agg[agg['hm'] == candidates[l]]['avg_dd'].values[0]) for l in xlabels]

metrics = [('Sharpe', sharpes), ('Ret÷10', rets), ('Pass÷10', passes), ('1/DD×10', inv_dd)]
for i, (metric_name, vals) in enumerate(metrics):
    offset = (i - 1.5) * bar_w
    bars = ax3.bar([j + offset for j in range(n)], vals, width=bar_w,
                   label=metric_name, alpha=0.85)
    for j, (bar_obj, v) in enumerate(zip(bars, vals)):
        ax3.text(j + offset, v + 0.03, f'{v:.2f}', ha='center', va='bottom',
                color=colors[xlabels[j]], fontsize=8, fontweight='bold')

ax3.set_xticks(range(n))
ax3.set_xticklabels([l.replace(' (', '\n(') for l in xlabels], fontsize=9)
ax3.set_ylabel('Value (scaled)', fontsize=11)
ax3.set_title('Key Metrics Comparison', fontsize=12, pad=10)
ax3.legend(loc='upper right', facecolor='#161b22', edgecolor='#30363d', labelcolor='white', fontsize=8, ncols=2)
ax3.yaxis.set_major_formatter(mticker.FormatStrFormatter('%.1f'))

plt.tight_layout(pad=1.5)
plt.savefig(OUT, dpi=150, bbox_inches='tight', facecolor='#0d1117', edgecolor='none')
print(f"\nSaved: {OUT}")

# Summary
wrow = agg[agg['hm'] == winner_hm].iloc[0]
brow = agg[agg['hm'] == baseline_hm].iloc[0]
rrow = agg[agg['hm'] == runnerup_hm].iloc[0]
print(f"""
=== HOLD_MAX SWEEP SUMMARY (P=15/M=1.50, Base5, 6 windows) ===
WINNER:    HM={winner_hm} | Sharpe={wrow['avg_sharpe']:.3f} | Ret={wrow['avg_ret']:.1f}% | DD={wrow['avg_dd']:.1f}% | Pass={int(wrow['pass_count'])}/{int(wrow['n_windows'])} ({wrow['pass_rate']:.0f}%)
BASELINE:  HM={baseline_hm} | Sharpe={brow['avg_sharpe']:.3f} | Ret={brow['avg_ret']:.1f}% | DD={brow['avg_dd']:.1f}% | Pass={int(brow['pass_count'])}/{int(brow['n_windows'])} ({brow['pass_rate']:.0f}%)
RUNNER-UP: HM={runnerup_hm} | Sharpe={rrow['avg_sharpe']:.3f} | Ret={rrow['avg_ret']:.1f}% | DD={rrow['avg_dd']:.1f}% | Pass={int(rrow['pass_count'])}/{int(rrow['n_windows'])} ({rrow['pass_rate']:.0f}%)
""")
