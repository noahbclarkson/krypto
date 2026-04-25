#!/usr/bin/env python3
"""
ATR_ENTRY_MULT Sweep Comparison Chart
Reads snapshots/atr_entry_mult_mean_equity.csv and generates charts/atr_entry_mult_sweep_comparison.png

Expected CSV format:
  atr_entry_mult,bar,mean_equity

Plots equity curves (log scale) for each ATR_ENTRY_MULT config.
"""

import pandas as pd
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import matplotlib.ticker as mticker
import numpy as np
import sys
import os

INPUT_CSV = "snapshots/atr_entry_mult_mean_equity.csv"
OUTPUT_PNG = "charts/atr_entry_mult_sweep_comparison.png"

# ── Color palette for ATR_ENTRY_MULT configs ─────────────────────────────────
COLOR_MAP = {
    0.00: "#2196F3",   # blue   — no filter
    0.50: "#4CAF50",   # green  — moderate filter
    0.85: "#FF5722",   # deep orange — current default
    0.90: "#9C27B0",   # purple — prior best
    1.00: "#F44336",   # red    — moderate-strong
    1.50: "#795548",   # brown  — trade-starving
}

def fmt_equity(v):
    if v >= 1000:
        return f"{v/1000:.1f}k"
    elif v >= 100:
        return f"{v:.0f}"
    elif v >= 10:
        return f"{v:.1f}"
    else:
        return f"{v:.2f}"

def main():
    if not os.path.exists(INPUT_CSV):
        print(f"ERROR: {INPUT_CSV} not found. Run atr_entry_mult_sweep_equity first.")
        sys.exit(1)

    df = pd.read_csv(INPUT_CSV)

    configs = sorted(df['atr_entry_mult'].unique())
    print(f"Loaded {len(configs)} configs: {configs}")

    fig, (ax1, ax2) = plt.subplots(2, 1, figsize=(14, 10), sharex=False)

    # ── Top: Equity curves (log scale) ───────────────────────────────────────
    for em in configs:
        sub = df[df['atr_entry_mult'] == em].sort_values('bar')
        color = COLOR_MAP.get(float(em), None)
        label = f"ATR_EM={em:.2f}"
        if float(em) == 0.85:
            label += " ← current"
        lw = 2.5 if float(em) in [0.00, 0.85, 0.90] else 1.2
        ax1.plot(sub['bar'], sub['mean_equity'],
                 label=label, color=color, linewidth=lw, alpha=0.9)

    ax1.set_yscale('log')
    ax1.set_ylabel("Mean Portfolio Equity (log scale)", fontsize=12)
    ax1.set_title("ATR_ENTRY_MULT Sweep — Mean Equity Curves (9 Universes × 6 Windows)\nTurtle+Chandelier EP=24 Chand(7,2.30) ATR(24,2.0) HM=12 CAP=3",
                  fontsize=13, fontweight='bold')
    ax1.grid(True, which='both', alpha=0.3, linestyle='-')
    ax1.legend(loc='upper left', fontsize=9, framealpha=0.9)
    ax1.yaxis.set_major_formatter(mticker.FuncFormatter(lambda v, _: fmt_equity(v)))

    # Add horizontal line at 1.0 for reference
    ax1.axhline(y=1.0, color='gray', linestyle='--', linewidth=0.8, alpha=0.5)

    # ── Bottom: Summary metrics bar chart ───────────────────────────────────
    summary_csv = "snapshots/atr_entry_mult_sweep_summary.csv"
    if os.path.exists(summary_csv):
        sum_df = pd.read_csv(summary_csv).sort_values('atr_entry_mult')
        # Filter to configs in our equity curves + a couple nearby for context
        filter_vals = [v for v in sum_df['atr_entry_mult'] if v in configs]
        sum_df = sum_df[sum_df['atr_entry_mult'].isin(filter_vals)]

        x = np.arange(len(sum_df))
        width = 0.35

        bars1 = ax2.bar(x - width/2, sum_df['avg_sharpe'], width,
                        label='Avg Sharpe', color='#2196F3', alpha=0.85)
        ax2_twin = ax2.twinx()
        bars2 = ax2_twin.bar(x + width/2, sum_df['pass_pct'], width,
                              label='Pass Rate %', color='#4CAF50', alpha=0.7)

        ax2.set_ylabel('Avg Sharpe (blue)', color='#2196F3', fontsize=11)
        ax2_twin.set_ylabel('Pass Rate % (green)', color='#4CAF50', fontsize=11)
        ax2.set_xticks(x)
        ax2.set_xticklabels([f"{v:.2f}" for v in sum_df['atr_entry_mult']], fontsize=9)
        ax2.set_xlabel("ATR_ENTRY_MULT", fontsize=11)
        ax2.set_title("Sharpe (blue) vs Pass Rate % (green) by ATR Entry Multiplier", fontsize=12)
        ax2.grid(True, axis='y', alpha=0.3)

        # Legend
        h1, l1 = ax1.get_legend_handles_labels()
        h2, l2 = ax2.get_legend_handles_labels()
        ax1.legend(h1 + [bars1, bars2], l1 + ['Avg Sharpe', 'Pass Rate %'],
                   loc='upper left', fontsize=8, framealpha=0.9, ncol=2)

        # Annotate best Sharpe bar
        best_sharpe_row = sum_df.loc[sum_df['avg_sharpe'].idxmax()]
        ax2.annotate(f"Best\nSharpe\n{float(best_sharpe_row['avg_sharpe']):.2f}",
                     xy=(list(sum_df['atr_entry_mult']).index(best_sharpe_row['atr_entry_mult']), best_sharpe_row['avg_sharpe']),
                     xytext=(0, 8), textcoords='offset points',
                     fontsize=8, ha='center', color='#2196F3', fontweight='bold')
    else:
        ax2.set_visible(False)
        ax1.legend(loc='upper left', fontsize=9, framealpha=0.9)

    plt.tight_layout()
    os.makedirs("charts", exist_ok=True)
    plt.savefig(OUTPUT_PNG, dpi=150, bbox_inches='tight', facecolor='white')
    print(f"Saved {OUTPUT_PNG}")

if __name__ == "__main__":
    main()