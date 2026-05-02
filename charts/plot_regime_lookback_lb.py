#!/usr/bin/env python3
"""REGIME_LOOKBACK Hyperopt Charting
Generates: regime_lookback_comparison.png
- Panel 1: Pass rate + Sharpe vs LB (all 196 values)
- Panel 2: Equity curves for Baseline(LB=42) and Winner(LB=45) — Base5 daily compound

Usage: python3 charts/plot_regime_lookback_lb.py
"""
import csv
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import matplotlib.gridspec as gridspec
import sys
import os

# ── Load sweep data ────────────────────────────────────────────────────────────
sweep_path = "snapshots/regime_lookback_lb_sweep.csv"
lb_vals, passes, totals, pass_pcts, sharpes, rets, dds, trades, pos_unis = [], [], [], [], [], [], [], [], []

with open(sweep_path) as f:
    reader = csv.DictReader(f)
    for row in reader:
        lb_vals.append(int(row['lb']))
        passes.append(int(row['pass']))
        totals.append(int(row['total']))
        pass_pcts.append(float(row['pass_pct']))
        sharpes.append(float(row['avg_sharpe']))
        rets.append(float(row['avg_ret_pct']))
        dds.append(float(row['avg_dd_pct']))
        trades.append(int(row['trades']))
        pos_unis.append(int(row['pos_universes']))

# Sanity-check: filter out obvious NaN/Infinity artifacts (LB >= 130 have garbage Sharpe)
clean_indices = [i for i, s in enumerate(sharpes) if abs(s) < 1e10]
lb_c   = [lb_vals[i]  for i in clean_indices]
sh_c   = [sharpes[i]  for i in clean_indices]
pp_c   = [pass_pcts[i] for i in clean_indices]
dd_c   = [dds[i]      for i in clean_indices]

# ── Load equity curves ─────────────────────────────────────────────────────────
def load_equity(path):
    windows, equities = [], []
    if not os.path.exists(path):
        return windows, equities
    with open(path) as f:
        reader = csv.DictReader(f)
        for row in reader:
            windows.append(int(row['window']))
            equities.append(float(row['equity']))
    return windows, equities

base5_42_w, base5_42_e = load_equity("snapshots/regime_lookback_lb42_base5_daily.csv")
base5_45_w, base5_45_e = load_equity("snapshots/regime_lookback_lb45_base5_daily.csv")
base5_44_w, base5_44_e = load_equity("snapshots/regime_lookback_lb44_base5_daily.csv")
global_42_w, global_42_e = load_equity("snapshots/regime_lookback_lb42_global_daily.csv")
global_45_w, global_45_e = load_equity("snapshots/regime_lookback_lb45_global_daily.csv")

# ── Plot ───────────────────────────────────────────────────────────────────────
fig = plt.figure(figsize=(16, 12))
fig.patch.set_facecolor('#0d1117')
gs = gridspec.GridSpec(2, 2, figure=fig, hspace=0.35, wspace=0.25)

ax_pass = fig.add_subplot(gs[0, 0])
ax_sharpe = fig.add_subplot(gs[0, 1])
ax_equity = fig.add_subplot(gs[1, :])

for ax in [ax_pass, ax_sharpe, ax_equity]:
    ax.set_facecolor('#161b22')
    ax.spines['bottom'].set_color('#3d444d')
    ax.spines['left'].set_color('#3d444d')
    ax.spines['right'].set_visible(False)
    ax.spines['top'].set_visible(False)
    ax.tick_params(colors='#8b949e')
    ax.yaxis.label.set_color('#8b949e')
    ax.xaxis.label.set_color('#8b949e')
    ax.title.set_color('#e6edf3')
    ax.grid(color='#21262d', linestyle='--', linewidth=0.5)

# Panel 1: Pass Rate
ax_pass.plot(lb_c, pp_c, color='#58a6ff', linewidth=1.5, alpha=0.9)
ax_pass.axvline(42, color='#f0883e', linewidth=1.2, linestyle='--', alpha=0.8, label='Baseline LB=42')
ax_pass.axvline(45, color='#3fb950', linewidth=1.2, linestyle='--', alpha=0.8, label='Winner LB=45')
ax_pass.fill_between(lb_c, 0, pp_c, alpha=0.08, color='#58a6ff')
ax_pass.set_xlabel('REGIME_LOOKBACK', fontsize=10)
ax_pass.set_ylabel('Pass Rate (%)', fontsize=10)
ax_pass.set_title('Pass Rate vs REGIME_LOOKBACK (LB ∈ [5..200])', fontsize=11, fontweight='bold')
ax_pass.legend(loc='lower right', labelcolor='#8b949e', fontsize=8)
ax_pass.set_ylim(50, 95)

# Panel 2: Sharpe
ax_sharpe.plot(lb_c, sh_c, color='#d29922', linewidth=1.5, alpha=0.9)
ax_sharpe.axvline(42, color='#f0883e', linewidth=1.2, linestyle='--', alpha=0.8, label='Baseline LB=42')
ax_sharpe.axvline(45, color='#3fb950', linewidth=1.2, linestyle='--', alpha=0.8, label='Winner LB=45')
ax_sharpe.axhline(0, color='#f85149', linewidth=0.8, linestyle='-', alpha=0.5)
ax_sharpe.fill_between(lb_c, 0, sh_c, where=[s >= 0 for s in sh_c], alpha=0.1, color='#3fb950')
ax_sharpe.fill_between(lb_c, 0, sh_c, where=[s < 0 for s in sh_c], alpha=0.1, color='#f85149')
ax_sharpe.set_xlabel('REGIME_LOOKBACK', fontsize=10)
ax_sharpe.set_ylabel('Avg OOS Sharpe', fontsize=10)
ax_sharpe.set_title('Avg Sharpe Ratio vs REGIME_LOOKBACK (LB ∈ [5..200])', fontsize=11, fontweight='bold')
ax_sharpe.legend(loc='lower right', labelcolor='#8b949e', fontsize=8)

# Annotate Sharpe cliff
cliff_lb = 59
ax_sharpe.annotate('Sharpe cliff\nat LB=59', xy=(59, 0), xytext=(75, 3),
    arrowprops=dict(arrowstyle='->', color='#f85149', lw=1.2),
    color='#f85149', fontsize=8, ha='center')
ax_sharpe.axvline(59, color='#f85149', linewidth=0.8, linestyle=':', alpha=0.6)

# Panel 3: Equity Curves — Base5 daily compound per window
ax_equity.plot(base5_42_w, base5_42_e, 'o-', color='#f0883e', linewidth=1.8,
    markersize=4, label='LB=42 (Baseline)', alpha=0.9)
ax_equity.plot(base5_45_w, base5_45_e, 's-', color='#3fb950', linewidth=1.8,
    markersize=4, label='LB=45 (Winner)', alpha=0.9)
ax_equity.plot(base5_44_w, base5_44_e, '^--', color='#a371f7', linewidth=1.2,
    markersize=4, label='LB=44 (Runner-up)', alpha=0.7)

ax_equity.set_xlabel('Walk-Forward Window', fontsize=10)
ax_equity.set_ylabel('Portfolio Equity (× initial)', fontsize=10)
ax_equity.set_title('Base5 Walk-Forward Equity — Baseline (LB=42) vs Winner (LB=45) — Per Window Compound', 
    fontsize=11, fontweight='bold')
ax_equity.legend(loc='upper left', labelcolor='#8b949e', fontsize=9)
ax_equity.yaxis.set_major_formatter(matplotlib.ticker.FuncFormatter(lambda x, _: f'{x:.1f}×'))

# Annotate final values
if base5_42_e:
    ax_equity.annotate(f'LB=42: {base5_42_e[-1]:.1f}×', 
        xy=(base5_42_w[-1], base5_42_e[-1]),
        xytext=(base5_42_w[-1]-1.5, base5_42_e[-1]*0.95),
        color='#f0883e', fontsize=8)
if base5_45_e:
    ax_equity.annotate(f'LB=45: {base5_45_e[-1]:.1f}×', 
        xy=(base5_45_w[-1], base5_45_e[-1]),
        xytext=(base5_45_w[-1]-1.5, base5_45_e[-1]*1.05),
        color='#3fb950', fontsize=8)

# ── Caption / Header ─────────────────────────────────────────────────────────
fig.text(0.5, 0.985, 
    'REGIME_LOOKBACK Hyperopt — Live Turtle-Only Path (2026-05-02)',
    ha='center', va='top', fontsize=13, fontweight='bold', color='#e6edf3')
fig.text(0.5, 0.955,
    'LB ∈ [5..=200] step 1 (196 values) × 9 universes × 7 WF windows | Fixed: EP=21, ATR(64,2.0), T=24, HOLD_MAX=12, CAP=3',
    ha='center', va='top', fontsize=9, color='#8b949e')

# ── Metrics inset ──────────────────────────────────────────────────────────────
metrics_text = (
    "Robustness Results\n"
    "─────────────────\n"
    f"LB=42 (baseline): 55/63 pass (87.3%)\n"
    f"                    Sharpe 6.188\n"
    f"                    DD 19.7% | 464 trades\n"
    f"                    9/9 positive universes\n"
    f"LB=45 (winner):    55/63 pass (87.3%)\n"
    f"                    Sharpe 6.188\n"
    f"                    DD 19.7% | 464 trades\n"
    f"                    9/9 positive universes\n"
    "─────────────────\n"
    "VERDICT: LB=42-45 plateau\n"
    "Identical metrics. LB=42\n"
    "remains production default.\n"
    "LB ≥ 59 → Sharpe → 0 or negative\n"
)
fig.text(0.78, 0.62, metrics_text,
    ha='left', va='top', fontsize=8, color='#8b949e',
    family='monospace',
    bbox=dict(boxstyle='round,pad=0.5', facecolor='#161b22', edgecolor='#30363d', alpha=0.9))

out_path = "charts/regime_lookback_comparison.png"
os.makedirs("charts", exist_ok=True)
fig.savefig(out_path, dpi=150, bbox_inches='tight', facecolor=fig.get_facecolor())
print(f"Saved: {out_path}")
plt.close(fig)