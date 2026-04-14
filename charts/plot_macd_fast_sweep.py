#!/usr/bin/env python3
"""
Plot MACD Fast Period Sweep — Equity Curve Comparison
Reads: snapshots/macd_fast_sweep_equity_latest.csv
Outputs: charts/comparison_chart.png

Equity = log scale (critical for comparing strategies with very different return magnitudes)
Drawdown = linear scale
Always caption with metrics
"""

import sys
import csv
import math
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import matplotlib.ticker as ticker
import numpy as np

EQUITY_CSV = "/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/macd_fast_sweep_equity_latest.csv"
RESULTS_CSV = "/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/macd_fast_sweep_results.csv"
OUTPUT_PNG = "/home/ubuntu/.openclaw/workspace-krypto/charts/comparison_chart.png"

# Color scheme: distinct colors for each line
COLORS = [
    '#1f77b4',  # blue (baseline)
    '#ff7f0e',  # orange (winner)
    '#2ca02c',  # green
    '#d62728',  # red
    '#9467bd',  # purple
]

def load_equity_data():
    """Load equity curves from CSV. Returns dict: fast_period -> list of equity values."""
    data = {}
    with open(EQUITY_CSV, 'r') as f:
        reader = csv.DictReader(f)
        for row in reader:
            step = int(row['step'])
            for col, val in row.items():
                if col == 'step':
                    continue
                period = int(col.replace('fast_', ''))
                if period not in data:
                    data[period] = []
                try:
                    v = float(val)
                    data[period].append((step, v))
                except (ValueError, KeyError):
                    pass

    # Convert to simple lists (step is just index)
    result = {}
    for period, steps in data.items():
        steps.sort(key=lambda x: x[0])
        result[period] = [v for _, v in steps]
    return result

def load_results_data():
    """Load sweep results CSV. Returns list of dicts sorted by Sharpe descending."""
    rows = []
    with open(RESULTS_CSV, 'r') as f:
        reader = csv.DictReader(f)
        for row in reader:
            rows.append({
                'fast_period': int(row['fast_period']),
                'windows': int(row['windows']),
                'passes': int(row['passes']),
                'avg_oos_pct': float(row['avg_oos_pct']),
                'avg_sharpe': float(row['avg_sharpe']),
                'worst_dd_pct': float(row['worst_dd_pct']),
                'avg_trades': float(row['avg_trades']),
                'full_return_pct': float(row['full_return_pct']),
            })
    rows.sort(key=lambda r: r['avg_sharpe'], reverse=True)
    return rows

def plot_comparison():
    equity_data = load_equity_data()
    results = load_results_data()

    if not equity_data or not results:
        print("ERROR: No data loaded from CSVs", file=sys.stderr)
        sys.exit(1)

    # Select top 5 + baseline (12)
    baseline = 12
    top5 = [r['fast_period'] for r in results[:5]]
    # Ensure baseline is included and appears first
    selected = [baseline] + [p for p in top5 if p != baseline]
    selected = selected[:5]

    # Color assignment
    color_map = {}
    for i, fp in enumerate(selected):
        color_map[fp] = COLORS[i % len(COLORS)]

    fig, (ax1, ax2) = plt.subplots(2, 1, figsize=(14, 10), gridspec_kw={'height_ratios': [3, 1]})
    fig.patch.set_facecolor('#0d1117')
    for ax in (ax1, ax2):
        ax.set_facecolor('#0d1117')
        ax.tick_params(colors='#8b949e', labelsize=10)
        ax.spines['bottom'].set_color('#30363d')
        ax.spines['left'].set_color('#30363d')
        ax.spines['top'].set_color('#30363d')
        ax.spines['right'].set_color('#30363d')
        ax.yaxis.label.set_color('#8b949e')
        ax.xaxis.label.set_color('#8b949e')
        ax.title.set_color('#e6edf3')
        ax.grid(True, alpha=0.15, color='#30363d', linestyle='--')

    # Plot equity curves (log scale)
    max_len = 0
    for fp in selected:
        if fp not in equity_data:
            continue
        eq = equity_data[fp]
        max_len = max(max_len, len(eq))
        steps = list(range(len(eq)))
        color = color_map[fp]
        label = f"fast={fp}" + (" (BASELINE)" if fp == baseline else "")
        lw = 2.0 if fp == baseline else 1.5
        alpha = 1.0 if fp == baseline else 0.85
        ax1.plot(steps, eq, label=label, color=color, linewidth=lw, alpha=alpha)

    # Log scale for equity
    ax1.set_yscale('log')
    ax1.set_ylabel('Portfolio Equity (log scale)', fontsize=11, color='#8b949e')
    ax1.set_title('MACD Fast Period Sweep — Equity Curves (MACD+Regime, Base5, Walk-Forward)', 
                  fontsize=13, fontweight='bold', color='#e6edf3', pad=12)
    ax1.legend(loc='upper left', fontsize=9, framealpha=0.2, labelcolor='#e6edf3',
               facecolor='#161b22', edgecolor='#30363d')

    # Find y-axis limits to avoid flat lines
    all_vals = []
    for fp in selected:
        if fp in equity_data:
            all_vals.extend(equity_data[fp])
    if all_vals:
        min_val = min(v for v in all_vals if v > 0)
        max_val = max(all_vals)
        ymin = min_val * 0.8
        ymax = max_val * 1.2
        ax1.set_ylim(ymin, ymax)

    # Bottom panel: Sharpe bar chart for all tested periods
    ax2.set_xlabel('MACD Fast Period', fontsize=11, color='#8b949e')
    ax2.set_ylabel('Avg Sharpe', fontsize=11, color='#8b949e')

    # Bar chart
    periods = [r['fast_period'] for r in results]
    sharpes = [r['avg_sharpe'] for r in results]

    # Color bars: baseline orange, winner green, others grey
    bar_colors = []
    winner_fp = results[0]['fast_period']
    for r in results:
        if r['fast_period'] == baseline:
            bar_colors.append('#ff7f0e')
        elif r['fast_period'] == winner_fp:
            bar_colors.append('#2ca02c')
        else:
            bar_colors.append('#30363d')

    bars = ax2.bar(periods, sharpes, color=bar_colors, width=1.5, alpha=0.9, edgecolor='none')
    ax2.axhline(y=0, color='#8b949e', linewidth=0.8, linestyle='-', alpha=0.5)
    ax2.set_xticks(periods)
    ax2.set_xticklabels([str(p) for p in periods], fontsize=8, color='#8b949e', rotation=45)
    ax2.yaxis.set_major_locator(ticker.MultipleLocator(0.5))

    # Annotate baseline and winner
    for r in results:
        if r['fast_period'] in (baseline, winner_fp):
            x = periods.index(r['fast_period'])
            ax2.annotate(
                f"{r['fast_period']}\nsh={r['avg_sharpe']:.2f}",
                xy=(x, r['avg_sharpe']),
                xytext=(0, 8),
                textcoords='offset points',
                ha='center', va='bottom',
                fontsize=7.5, color='#e6edf3',
                fontweight='bold',
            )

    plt.tight_layout(pad=2.0)

    # Save
    import os
    os.makedirs(os.path.dirname(OUTPUT_PNG), exist_ok=True)
    plt.savefig(OUTPUT_PNG, dpi=150, bbox_inches='tight', facecolor='#0d1117', edgecolor='none')
    plt.close()
    print(f"Saved: {OUTPUT_PNG}")

    # Print summary table
    print("\n═══ TOP 5 CONFIGURATIONS ═══")
    print(f"{'Rank':>4} | {'Fast':>4} | {'Sharpe':>7} | {'Avg OOS%':>9} | {'Worst DD%':>10} | {'Pass':>4} | {'Avg Trades':>10}")
    print("-" * 65)
    for i, r in enumerate(results[:5], 1):
        flag = " ←WINNER" if i == 1 else (" ←BASELINE" if r['fast_period'] == baseline else "")
        print(f"{i:>4}  | {r['fast_period']:>4}{flag} | {r['avg_sharpe']:>+7.3f} | {r['avg_oos_pct']:>+9.1f} | {r['worst_dd_pct']:>+10.1f} | {r['passes']:>4}/{r['windows']} | {r['avg_trades']:>10.1f}")

if __name__ == '__main__':
    plot_comparison()
