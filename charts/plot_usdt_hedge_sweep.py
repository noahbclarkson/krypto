#!/usr/bin/env python3
"""Chart USDT hedge size_mult sweep results from equity CSV exports."""

import csv
import sys
import os
from pathlib import Path

import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt

CHarts_DIR = Path(__file__).parent
KRYPT_DIR = CHarts_DIR.parent
SNAP = KRYPT_DIR / "snapshots"

def load_equity_csv(path):
    """Load equity CSV: columns are window indices, values are equity multipliers."""
    data = {}
    if not path.exists():
        return None
    with open(path) as f:
        reader = csv.reader(f)
        header = next(reader)
        for row in reader:
            if len(row) < 2:
                continue
            # Row format: window,equity
            try:
                w = int(row[0])
                eq = float(row[1])
                data[w] = eq
            except:
                pass
    return data

def load_all_equities(base_name, values):
    """Load equity for each size_mult value."""
    results = {}
    for v in values:
        path = SNAP / f"usdt_hedge_{base_name}_sm{v:.2f}.csv"
        results[v] = load_equity_csv(path)
    return results

def plot_comparison(values, equity_data, baseline, winner, outpath):
    """Plot equity comparison chart with dynamic Y scaling."""
    fig, ax = plt.subplots(figsize=(12, 7))

    colors = matplotlib.colors.LinearSegmentedColormap.from_list(
        'viridis', ['#440154','#31688e','#35b779','#fde725')
    cmap = colors

    x = sorted(equity_data[baseline].keys())

    # Plot each size_mult as a faint line
    for v in values:
        if equity_data[v] is None:
            continue
        ys = [equity_data[v].get(xi, 0) for xi in x]
        ax.plot(x, ys, color='#cccccc', linewidth=0.5, alpha=0.5)

    # Plot baseline
    ax.plot(x, [equity_data[baseline].get(xi, 0) for xi in x],
            color='blue', linewidth=2.5, label=f'Baseline (sm={baseline:.2f})', zorder=3)

    # Plot winner
    ax.plot(x, [equity_data[winner].get(xi, 0) for xi in x],
            color='red', linewidth=2.5, label=f'Winner (sm={winner:.2f})', zorder=4)

    ax.set_xlabel('Walk-Forward Window', fontsize=12)
    ax.set_ylabel('Cumulative Equity (×)', fontsize=12)
    ax.set_title(f'USDT Hedge Size_Mult Sweep — Base5 Equity\n'
                 f'Baseline sm={baseline:.2f} vs Winner sm={winner:.2f}', fontsize=13)
    ax.legend(fontsize=11)
    ax.grid(True, alpha=0.3)

    # Dynamic Y: don't start at 0 if data is squeezed
    all_vals = []
    for v in values:
        if equity_data[v]:
            all_vals.extend(equity_data[v].values())
    if all_vals:
        ymin = min(all_vals)
        ymax = max(all_vals)
        pad = (ymax - ymin) * 0.1
        ax.set_ylim(ymin - pad, ymax + pad)

    fig.tight_layout()
    fig.savefig(outpath, dpi=150)
    plt.close(fig)
    print(f"Saved: {outpath}")

def main():
    # Extensive size_mult sweep values
    values = [round(x * 0.05, 2) for x in range(10, 21)]  # 0.50 to 1.00 step 0.05
    baseline = 1.00

    equity_data = load_all_equities("base5", values)

    # Find winner by total equity
    winners = []
    for v in values:
        if equity_data[v]:
            total = sum(equity_data[v].values())
            winners.append((v, total))
    winners.sort(key=lambda x: x[1], reverse=True)
    winner = winners[0][0]

    print(f"Baseline (sm={baseline:.2f}): final equity = {equity_data[baseline]}")
    print(f"Winner  (sm={winner:.2f}): final equity = {equity_data[winner]}")
    print(f"Top 5: {winners[:5]}")

    outpath = KRYPT_DIR / "charts" / "comparison_chart.png"
    plot_comparison(values, equity_data, baseline, winner, outpath)

    # Also plot sharpe comparison bar chart
    summary_path = SNAP / "usdt_hedge_sweep_summary.csv"
    if summary_path.exists():
        fig2, ax2 = plt.subplots(figsize=(12, 6))
        sm_values = []
        sharpes = []
        passes = []
        with open(summary_path) as f:
            reader = csv.DictReader(f)
            for row in reader:
                sm_values.append(float(row['size_mult']))
                sharpes.append(float(row['avg_sharpe']))
                passes.append(float(row['pass_rate']))
        ax2_twin = ax2.twinx()
        l1 = ax2.bar(range(len(sm_values)), sharpes, color='steelblue', alpha=0.7, label='Avg Sharpe')
        l2 = ax2_twin.plot(range(len(sm_values)), passes, 'ro-', linewidth=2, label='Pass Rate')
        ax2.set_xticks(range(len(sm_values)))
        ax2.set_xticklabels([f'{v:.2f}' for v in sm_values], rotation=45)
        ax2.set_xlabel('Size_Mult')
        ax2.set_ylabel('Avg Sharpe', color='steelblue')
        ax2_twin.set_ylabel('Pass Rate (%)', color='red')
        ax2.set_title('USDT Hedge: Avg Sharpe & Pass Rate by Size_Mult')
        ax2.grid(True, alpha=0.3)
        lines = [l1, l2[0]]
        ax2.legend(lines, ['Avg Sharpe', 'Pass Rate'], loc='upper left')
        fig2.tight_layout()
        bar_path = KRYPT_DIR / "charts" / "usdt_hedge_sweep_bars.png"
        fig2.savefig(bar_path, dpi=150)
        plt.close(fig2)
        print(f"Saved: {bar_path}")

if __name__ == '__main__':
    main()
