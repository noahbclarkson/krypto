#!/usr/bin/env python3
"""
Slippage Sensitivity Comparison Chart
Generates: charts/slippage_comparison.png

Data source: snapshots/slippage_sweep_equity.csv + snapshots/slippage_sweep.csv
"""
import pandas as pd
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import matplotlib.ticker as mticker
import numpy as np

OUT = "charts/slippage_comparison.png"
EQUITY_CSV = "snapshots/slippage_sweep_equity.csv"
METRICS_CSV = "snapshots/slippage_sweep.csv"

CONFIGS = [
    (0.0, "0 bps (Ideal)", "#2ecc71"),
    (10.0, "10 bps (Default)", "#3498db"),
    (30.0, "30 bps (Live Est.)", "#f39c12"),
    (50.0, "50 bps (Stress)", "#e74c3c"),
    (100.0, "100 bps (Extreme)", "#9b59b6"),
]

def load_equity(slip_bps):
    df = pd.read_csv(EQUITY_CSV)
    sub = df[df['slip_bps'] == slip_bps].sort_values("bar")
    return sub["bar"].values, sub["equity_mean"].values

def max_dd(equity):
    peak = equity[0]
    max_dd = 0.0
    for e in equity:
        if e > peak:
            peak = e
        dd = (peak - e) / peak
        if dd > max_dd:
            max_dd = dd
    return max_dd * 100.0

fig, (ax1, ax2) = plt.subplots(2, 1, figsize=(14, 10))
fig.suptitle("Turtle+Chandelier: Slippage Sensitivity\n(9 Universes × 7 Walk-Forward Windows)",
            fontsize=14, fontweight='bold')

# ── Top panel: Equity curves (log scale) ───────────────────────────────────────
for (slip, label, color) in CONFIGS:
    bars, equity = load_equity(slip)
    ax1.plot(bars, equity, label=label, color=color, linewidth=1.8, alpha=0.9)

ax1.set_yscale('log')
ax1.set_ylabel("Portfolio Equity (log scale)", fontsize=11)
ax1.set_title("Equity Curves by Slippage Level", fontsize=11)
ax1.legend(loc="upper left", fontsize=9, framealpha=0.85)
ax1.grid(True, alpha=0.3, linestyle='--')
ax1.set_xlim(0, None)
ax1.yaxis.set_major_formatter(mticker.FuncFormatter(lambda y, _: f'{y:.2f}'))

# ── Bottom panel: Metrics degradation ────────────────────────────────────────
metrics_df = pd.read_csv(METRICS_CSV)
slip_vals = sorted(metrics_df["slippage_bps"].unique())

sharpe_vals = []
dd_vals = []
ret_vals = []
for slip in slip_vals:
    sub = metrics_df[metrics_df["slippage_bps"] == slip]
    sharpe_vals.append(sub["sharpe"].mean())
    dd_vals.append(sub["max_dd_pct"].mean())
    ret_vals.append(sub["return_pct"].mean())

sharpe_norm = np.array(sharpe_vals) / sharpe_vals[0]
ret_norm = np.array(ret_vals) / ret_vals[0]

ax2.plot(slip_vals, sharpe_norm, label="Sharpe (normalised)", color="#3498db", linewidth=2, marker='o', markersize=4)
ax2.plot(slip_vals, ret_norm, label="Return (normalised)", color="#2ecc71", linewidth=2, marker='s', markersize=4)
ax2.axhline(1.0, color='gray', linewidth=1, linestyle='--', alpha=0.6)
ax2.axhline(0.7, color='red', linewidth=1, linestyle=':', alpha=0.5, label="70% threshold")
ax2.set_xlabel("Slippage (bps)", fontsize=11)
ax2.set_ylabel("Metric Value / Ideal Value", fontsize=11)
ax2.set_title("Performance Degradation vs Slippage (normalised to 0 bps)", fontsize=11)
ax2.legend(loc="upper right", fontsize=9)
ax2.grid(True, alpha=0.3, linestyle='--')
ax2.set_xlim(-2, 102)
ax2.set_ylim(0.3, 1.1)

ax2.axvline(10, color='#3498db', linewidth=1.5, linestyle='--', alpha=0.7)
ax2.annotate('Current\nDefault\n(10 bps)', xy=(10, 0.98), fontsize=8,
             color='#3498db', ha='center')

plt.tight_layout()
plt.savefig(OUT, dpi=150, bbox_inches='tight', facecolor='white')
print(f"Saved: {OUT}")

# ── Print key findings ──────────────────────────────────────────────────────────
print("\n========== SLIPPAGE AUDIT RESULTS ==========")
print(f"{'Slip(bps)':>10} {'Sharpe':>10} {'Ret%':>10} {'DD%':>8} {'Pass':>8}")
for slip in [0, 10, 20, 30, 50, 100]:
    sub = metrics_df[metrics_df["slippage_bps"] == slip]
    n_pass = (sub["pass"] == 1).sum()
    n_total = len(sub)
    print(f"{slip:>10.0f} {sub['sharpe'].mean():>10.4f} {sub['return_pct'].mean():>10.2f} "
          f"{sub['max_dd_pct'].mean():>8.2f} "
          f"{n_pass}/{n_total} ({n_pass/n_total*100:.1f}%)")

print("\n========== MAX DRAWDOWN BY CONFIG ==========")
for (slip, label, color) in CONFIGS:
    bars, equity = load_equity(slip)
    dd = max_dd(equity)
    final = equity[-1]
    print(f"  {label:30s}: MaxDD={dd:5.2f}%  Final={final:.4f}")
