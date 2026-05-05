#!/usr/bin/env python3
"""
Plot comparison chart for HEDGE_SIZE_MULT sweep.
Reads hedge_size_mult_equity.csv (per-window compound equity).
Plots Baseline (SM=1.00), Winner (SM=0.40), and runner-ups (SM=0.50, SM=0.30).
"""
import pandas as pd
import matplotlib.pyplot as plt
import matplotlib.ticker as mticker

def main():
    path = "snapshots/hedge_size_mult_equity.csv"
    df = pd.read_csv(path)

    # Columns: window, sm_0_30, sm_0_40, ..., sm_1_00
    # Actual column names in the CSV
    col_map = {
        'sm_0_30': ('SM=0.30', '#1d3557'),
        'sm_0_40': ('SM=0.40 (winner)', '#e63946'),
        'sm_0_50': ('SM=0.50 (runner-up)', '#457b9d'),
        'sm_0_70': ('SM=0.70 (old default)', '#aaaaaa'),
        'sm_1_00': ('SM=1.00 (baseline, no hedge)', '#888888'),
    }

    plot_order = ['sm_1_00', 'sm_0_70', 'sm_0_50', 'sm_0_40', 'sm_0_30']
    available = [c for c in plot_order if c in df.columns]
    print(f"Available columns to plot: {available}")
    print(f"All columns: {list(df.columns)}")

    fig, ax = plt.subplots(figsize=(14, 8))

    for col in available:
        equity = df[col]
        windows = df.index
        label, color = col_map[col]
        ax.plot(windows, equity.values, label=label, color=color, linewidth=2.0, marker='o', markersize=4)

    ax.set_yscale('log')
    ax.yaxis.set_major_formatter(mticker.FuncFormatter(lambda v, _: f'{v:.1f}x'))
    ax.set_xlabel("Walk-Forward Window", fontsize=12)
    ax.set_ylabel("Cumulative Equity (log scale)", fontsize=12)
    ax.set_title("HEDGE_SIZE_MULT Sweep — Per-Window Compound Equity\n(13 values × 9 universes × 7 windows = 819 sims)", fontsize=14)
    ax.legend(fontsize=11, loc="upper left")
    ax.grid(True, alpha=0.3)

    # Annotate final values
    for col in available:
        final = df[col].iloc[-1]
        label, color = col_map[col]
        ax.annotate(f'{label}\n{final:.2f}x',
                    xy=(len(df)-1, final),
                    fontsize=8.5, color=color,
                    xytext=(5, 0), textcoords='offset points')

    plt.tight_layout()
    out = "charts/comparison_chart.png"
    plt.savefig(out, dpi=150)
    print(f"Saved {out}")

if __name__ == "__main__":
    main()