"""
EP Re-Optimization Chart
Generates comparison_chart.png showing EP sweep results.
Uses dynamic Y-axis (no forced zero) for equity curves.
"""

import csv
import math
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import matplotlib.ticker as mticker
import numpy as np

METRICS_CSV = "snapshots/ep_reopt_metrics.csv"
EQUITY_CSV  = "snapshots/ep_reopt_equity.csv"
OUT_PNG     = "charts/comparison_chart.png"

# ── Load metrics ──────────────────────────────────────────────────────────────
eps, pass_pcts, sharpes, returns, dds = [], [], [], [], []
with open(METRICS_CSV) as f:
    reader = csv.DictReader(f)
    for row in reader:
        eps.append(int(row['ep']))
        pass_pcts.append(float(row['pass_pct']))
        sharpes.append(float(row['avg_sharpe']))
        returns.append(float(row['avg_return_pct']))
        dds.append(float(row['avg_dd_pct']))

eps = np.array(eps)
pass_pcts = np.array(pass_pcts)
sharpes = np.array(sharpes)
returns = np.array(returns)
dds = np.array(dds)

# Key reference points
baseline_ep   = 21
winner_ep     = int(eps[np.argmax(sharpes)])          # highest avg Sharpe
robust_ep     = int(eps[np.argmax(pass_pcts)])        # highest pass rate (first occurrence)
# Find runner-up on Sharpe (excluding winner)
sharpe_order  = np.argsort(sharpes)[::-1]
runner_ep     = int(eps[sharpe_order[1]])

print(f"Baseline EP={baseline_ep}: pass={pass_pcts[eps==baseline_ep][0]:.1f}% sh={sharpes[eps==baseline_ep][0]:.3f}")
print(f"Winner   EP={winner_ep}: pass={pass_pcts[eps==winner_ep][0]:.1f}% sh={sharpes[eps==winner_ep][0]:.3f}")
print(f"Robust   EP={robust_ep}: pass={pass_pcts[eps==robust_ep][0]:.1f}% sh={sharpes[eps==robust_ep][0]:.3f}")
print(f"RunnerUp EP={runner_ep}: pass={pass_pcts[eps==runner_ep][0]:.1f}% sh={sharpes[eps==runner_ep][0]:.3f}")

# ── Load equity curves ────────────────────────────────────────────────────────
eq_data = {}
with open(EQUITY_CSV) as f:
    reader = csv.DictReader(f)
    cols = reader.fieldnames[1:]  # skip 'step'
    for col in cols:
        eq_data[col] = []
    for row in reader:
        for col in cols:
            eq_data[col].append(float(row[col]))

# Map: ep_XX -> values
def get_eq(ep):
    key = f"ep_{ep}"
    return eq_data.get(key, [])

# ── Figure layout: 2×2 grid ───────────────────────────────────────────────────
fig, axes = plt.subplots(2, 2, figsize=(16, 11))
fig.suptitle(
    f"EP (Turtle Entry Period) Re-Optimization\n"
    f"CHAND(P=11, M=2.25) + ATR(24) | 5 Universes × 6 WF Windows Each\n"
    f"Baseline EP=21 vs Sweep EP 5–55 Step 1",
    fontsize=13, fontweight='bold', y=0.98
)
fig.patch.set_facecolor('#0d1117')
for ax in axes.flat:
    ax.set_facecolor('#161b22')
    ax.tick_params(colors='#c9d1d9')
    ax.xaxis.label.set_color('#c9d1d9')
    ax.yaxis.label.set_color('#c9d1d9')
    ax.title.set_color('#e6edf3')
    for spine in ax.spines.values():
        spine.set_edgecolor('#30363d')

BASELINE_COLOR = '#58a6ff'
WINNER_COLOR   = '#3fb950'
RUNNER_COLOR   = '#f0883e'
ROBUST_COLOR   = '#bc8cff'
NEUTRAL_COLOR  = '#484f58'

# ── Panel A: Pass Rate vs EP ───────────────────────────────────────────────────
ax = axes[0, 0]
ax.fill_between(eps, pass_pcts, alpha=0.15, color=BASELINE_COLOR)
ax.plot(eps, pass_pcts, color=BASELINE_COLOR, linewidth=1.5, label='Pass Rate')
ax.axvline(baseline_ep, color=BASELINE_COLOR, linestyle='--', linewidth=1.5, alpha=0.8, label=f'Baseline EP={baseline_ep}')
ax.axvline(robust_ep,   color=ROBUST_COLOR,   linestyle='--', linewidth=1.5, alpha=0.8, label=f'Peak Pass EP={robust_ep}')
ax.axhline(pass_pcts[eps==baseline_ep][0], color=BASELINE_COLOR, linestyle=':', linewidth=1, alpha=0.5)
ax.axhline(max(pass_pcts),                 color=ROBUST_COLOR,   linestyle=':', linewidth=1, alpha=0.5)

# Shade the robust zone (90%+)
robust_zone = (pass_pcts >= 90.0)
ax.fill_between(eps, 0, 100, where=robust_zone, alpha=0.08, color=ROBUST_COLOR, label='≥90% pass zone')

ax.set_xlabel('Entry Period (EP)', fontsize=11)
ax.set_ylabel('Pass Rate (%)', fontsize=11)
ax.set_title('Pass Rate vs Entry Period', fontsize=12, fontweight='bold')
ax.legend(fontsize=9, framealpha=0.2, facecolor='#21262d', edgecolor='#30363d', labelcolor='#c9d1d9')
ax.grid(True, alpha=0.2, color='#30363d')
ax.set_xlim(eps.min(), eps.max())
ax.set_ylim(0, 100)

# ── Panel B: Avg Sharpe vs EP ─────────────────────────────────────────────────
ax = axes[0, 1]
ax.plot(eps, sharpes, color=NEUTRAL_COLOR, linewidth=1, alpha=0.5)
ax.scatter(eps, sharpes, c=sharpes, cmap='RdYlGn', s=25, zorder=3)
ax.axvline(baseline_ep, color=BASELINE_COLOR, linestyle='--', linewidth=1.5, alpha=0.9, label=f'Baseline EP={baseline_ep} (sh={sharpes[eps==baseline_ep][0]:.2f})')
ax.axvline(winner_ep,   color=WINNER_COLOR,   linestyle='--', linewidth=1.5, alpha=0.9, label=f'Sharpe Winner EP={winner_ep} (sh={sharpes[eps==winner_ep][0]:.2f})')
ax.axvline(robust_ep,   color=ROBUST_COLOR,   linestyle='--', linewidth=1.5, alpha=0.9, label=f'Robust EP={robust_ep} (sh={sharpes[eps==robust_ep][0]:.2f})')

ax.set_xlabel('Entry Period (EP)', fontsize=11)
ax.set_ylabel('Avg Walk-Forward Sharpe', fontsize=11)
ax.set_title('Avg Sharpe vs Entry Period', fontsize=12, fontweight='bold')
ax.legend(fontsize=9, framealpha=0.2, facecolor='#21262d', edgecolor='#30363d', labelcolor='#c9d1d9')
ax.grid(True, alpha=0.2, color='#30363d')
ax.set_xlim(eps.min(), eps.max())
# Dynamic Y-axis
sh_pad = (sharpes.max() - sharpes.min()) * 0.1
ax.set_ylim(sharpes.min() - sh_pad, sharpes.max() + sh_pad * 2)

# ── Panel C: Equity Curves (log scale) ────────────────────────────────────────
ax = axes[1, 0]

plot_eps = sorted(set([baseline_ep, winner_ep, robust_ep, runner_ep]))
colors_map = {
    baseline_ep: (BASELINE_COLOR, '--', f'EP={baseline_ep} (baseline, pass={pass_pcts[eps==baseline_ep][0]:.0f}%)'),
    winner_ep:   (WINNER_COLOR,   '-',  f'EP={winner_ep} (Sharpe winner, pass={pass_pcts[eps==winner_ep][0]:.0f}%)'),
    robust_ep:   (ROBUST_COLOR,   '-',  f'EP={robust_ep} (robust peak, pass={pass_pcts[eps==robust_ep][0]:.0f}%)'),
    runner_ep:   (RUNNER_COLOR,   '-',  f'EP={runner_ep} (Sharpe runner-up)'),
}

for ep in plot_eps:
    eq = get_eq(ep)
    if not eq:
        continue
    col, ls, lbl = colors_map.get(ep, ('#888888', '-', f'EP={ep}'))
    x = list(range(len(eq)))
    ax.semilogy(x, eq, color=col, linestyle=ls, linewidth=2 if ep == winner_ep else 1.5, label=lbl)

ax.set_xlabel('Simulation Step (bars)', fontsize=11)
ax.set_ylabel('Portfolio Value (log scale, start=1.0x)', fontsize=11)
ax.set_title('Equity Curves — Base5 Full-Sample\n(TRAIN bars skipped, post-train period only)', fontsize=11, fontweight='bold')
ax.legend(fontsize=9, framealpha=0.2, facecolor='#21262d', edgecolor='#30363d', labelcolor='#c9d1d9')
ax.grid(True, alpha=0.2, color='#30363d', which='both')
ax.yaxis.set_major_formatter(mticker.FuncFormatter(lambda y, _: f'{y:.0f}x'))

# ── Panel D: Avg Return + Avg DD ──────────────────────────────────────────────
ax = axes[1, 1]
width = 0.4
x_idx = np.arange(len(eps))

# Subsample for readability: every 3rd EP
sub = slice(None, None, 3)
ax2 = ax.twinx()
ax2.set_facecolor('#161b22')
ax2.tick_params(colors='#c9d1d9')
ax2.yaxis.label.set_color('#c9d1d9')
for spine in ax2.spines.values():
    spine.set_edgecolor('#30363d')

ax.bar(eps[sub] - 0.3, returns[sub], width=0.55, color='#3fb950', alpha=0.5, label='Avg Return %')
ax2.plot(eps[sub], dds[sub], color='#f85149', linewidth=1.5, marker='o', markersize=3, label='Avg MaxDD %')

ax.axvline(baseline_ep, color=BASELINE_COLOR, linestyle='--', linewidth=1.5, alpha=0.8, label=f'EP={baseline_ep} baseline')
ax.axvline(winner_ep,   color=WINNER_COLOR,   linestyle='--', linewidth=1.5, alpha=0.8, label=f'EP={winner_ep} winner')

ax.set_xlabel('Entry Period (EP)', fontsize=11)
ax.set_ylabel('Avg OOS Return (%)', fontsize=11)
ax2.set_ylabel('Avg Max Drawdown (%)', fontsize=11, color='#f85149')
ax.set_title('Return & Drawdown vs Entry Period', fontsize=12, fontweight='bold')

lines1, labels1 = ax.get_legend_handles_labels()
lines2, labels2 = ax2.get_legend_handles_labels()
ax.legend(lines1 + lines2, labels1 + labels2, fontsize=9, framealpha=0.2,
          facecolor='#21262d', edgecolor='#30363d', labelcolor='#c9d1d9')
ax.grid(True, alpha=0.2, color='#30363d')
ax.set_xlim(eps.min() - 1, eps.max() + 1)

# Dynamic DD Y-axis
dd_pad = (dds.max() - dds.min()) * 0.2
ax2.set_ylim(dds.min() - dd_pad, dds.max() + dd_pad)

# ── Annotation box ────────────────────────────────────────────────────────────
result_text = (
    f"KEY FINDINGS\n"
    f"────────────────────────\n"
    f"Baseline EP={baseline_ep}: {pass_pcts[eps==baseline_ep][0]:.0f}% pass, Sh={sharpes[eps==baseline_ep][0]:.2f}\n"
    f"Robust peak EP={robust_ep}: {pass_pcts[eps==robust_ep][0]:.0f}% pass (+3.3pp), Sh={sharpes[eps==robust_ep][0]:.2f}\n"
    f"Sharpe winner EP={winner_ep}: {pass_pcts[eps==winner_ep][0]:.0f}% pass, Sh={sharpes[eps==winner_ep][0]:.2f}\n"
    f"────────────────────────\n"
    f"VERDICT: EP=23-25 are 93% pass zone\n"
    f"EP=24 recommended: +3.3pp pass, ≈same Sharpe\n"
    f"EP={winner_ep} is Sharpe winner but LESS robust"
)
fig.text(0.01, 0.01, result_text, fontsize=8.5, color='#c9d1d9',
         verticalalignment='bottom', fontfamily='monospace',
         bbox=dict(boxstyle='round', facecolor='#21262d', edgecolor='#30363d', alpha=0.9))

plt.tight_layout(rect=[0, 0.09, 1, 0.96])
plt.savefig(OUT_PNG, dpi=140, bbox_inches='tight', facecolor=fig.get_facecolor())
print(f"\nChart saved → {OUT_PNG}")
plt.close()
