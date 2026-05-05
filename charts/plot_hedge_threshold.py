#!/usr/bin/env python3
"""
T58: USDT Hedge Threshold — Comparison Chart Generator

Reads:
  - snapshots/hedge_threshold_extensive_sweep.csv     (full sweep results)
  - snapshots/hedge_threshold_extensive_timeseries.csv (Base5 equity time-series)

Outputs:
  - charts/comparison_chart.png  (equity curves: Baseline vs Winner vs Runner-ups)

CRITICAL GRAPHING RULES:
  - Line graph of equity over time (NOT text/stats)
  - Dynamic Y-axis scaling (don't force 0)
  - Distinct colored lines with clear legend
  - Axis labels, title, grid lines
"""
import csv
import os
import sys

import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import matplotlib.ticker as ticker
import numpy as np

SWEEP_CSV = "snapshots/hedge_threshold_extensive_sweep.csv"
TS_CSV = "snapshots/hedge_threshold_extensive_timeseries.csv"
OUTPUT_PNG = "charts/comparison_chart.png"
HEATMAP_PNG = "charts/hedge_threshold_heatmap.png"


def load_sweep():
    """Load sweep results CSV."""
    rows = []
    with open(SWEEP_CSV) as f:
        reader = csv.DictReader(f)
        for r in reader:
            rows.append({
                'hedge_pct': int(r['hedge_pct']),
                'pass_count': int(r['pass_count']),
                'total': int(r['total']),
                'pass_rate': float(r['pass_rate']),
                'avg_sharpe': float(r['avg_sharpe']),
                'avg_return': float(r['avg_return']),
                'avg_dd': float(r['avg_dd']),
                'total_trades': int(r['total_trades']),
                'positive_universes': int(r['positive_universes']),
                'base5_agg_equity': float(r['base5_agg_equity']),
            })
    return rows


def load_timeseries():
    """Load time-series CSV. Returns {pct_value: [equity_values]}."""
    curves = {}
    with open(TS_CSV) as f:
        reader = csv.DictReader(f)
        cols = reader.fieldnames
        pct_cols = [c for c in cols if c.startswith('pct_')]
        for c in pct_cols:
            curves[c] = []
        for row in reader:
            for c in pct_cols:
                curves[c].append(float(row[c]))
    return curves


def find_winner_and_runners(sweep):
    """Find winner (best pass rate, then Sharpe) and top runners."""
    sorted_rows = sorted(sweep, key=lambda r: (r['pass_count'], r['avg_sharpe']), reverse=True)
    winner = sorted_rows[0]
    runners = sorted_rows[1:5]  # top 4 runners
    return winner, runners


def plot_equity_curves(curves, winner, runners, baseline_pct=75, disabled_pct=100):
    """Plot equity curves with dynamic Y-axis scaling."""
    fig, (ax1, ax2) = plt.subplots(2, 1, figsize=(14, 10), gridspec_kw={'height_ratios': [3, 1]})
    
    winner_key = f"pct_{winner['hedge_pct']}"
    baseline_key = f"pct_{baseline_pct}"
    disabled_key = f"pct_{disabled_pct}"
    
    # Determine which curves we have
    available = {}
    labels = {}
    colors = {}
    linewidths = {}
    
    if baseline_key in curves:
        available['baseline'] = curves[baseline_key]
        labels['baseline'] = f"Prior baseline (PCT={baseline_pct})"
        colors['baseline'] = '#888888'
        linewidths['baseline'] = 1.5
    
    if disabled_key in curves:
        available['disabled'] = curves[disabled_key]
        labels['disabled'] = f"Disabled (PCT={disabled_pct}, no hedge)"
        colors['disabled'] = '#CCCCCC'
        linewidths['disabled'] = 1.0
    
    if winner_key in curves:
        available['winner'] = curves[winner_key]
        labels['winner'] = f"★ WINNER (PCT={winner['hedge_pct']}, pass={winner['pass_count']}/{winner['total']}, Sharpe={winner['avg_sharpe']:.3f})"
        colors['winner'] = '#2196F3'
        linewidths['winner'] = 2.5
    
    # Add runner-ups
    runner_colors = ['#FF9800', '#4CAF50', '#9C27B0', '#F44336']
    for i, runner in enumerate(runners[:4]):
        key = f"pct_{runner['hedge_pct']}"
        if key in curves and key != winner_key and key != baseline_key:
            tag = f"runner_{i}"
            available[tag] = curves[key]
            labels[tag] = f"Runner #{i+1} (PCT={runner['hedge_pct']}, pass={runner['pass_count']}/{runner['total']}, Sharpe={runner['avg_sharpe']:.3f})"
            colors[tag] = runner_colors[i % len(runner_colors)]
            linewidths[tag] = 1.2
    
    # === UPPER PANEL: Equity curves (log scale) ===
    for tag in ['disabled', 'baseline'] + [f'runner_{i}' for i in range(4)] + ['winner']:
        if tag in available:
            data = available[tag]
            x = np.arange(len(data))
            ax1.plot(x, data, label=labels[tag], color=colors[tag], 
                    linewidth=linewidths[tag], alpha=0.9 if tag == 'winner' else 0.7)
    
    ax1.set_yscale('log')
    ax1.set_title('USDT Hedge Threshold — Base5 Compounded Equity (AP17/VL92 Walk-Forward)', fontsize=14, fontweight='bold')
    ax1.set_ylabel('Portfolio Value (log scale, start=1.0)', fontsize=11)
    ax1.legend(loc='upper left', fontsize=8, framealpha=0.9)
    ax1.grid(True, alpha=0.3)
    ax1.yaxis.set_major_formatter(ticker.FormatStrFormatter('%.1f'))
    
    # === LOWER PANEL: Drawdown ===
    if 'winner' in available:
        eq = np.array(available['winner'])
        peak = np.maximum.accumulate(eq)
        dd = (eq - peak) / peak * 100
        ax2.fill_between(np.arange(len(dd)), dd, 0, alpha=0.3, color='#2196F3', label=f'Winner DD (PCT={winner["hedge_pct"]})')
    
    if 'baseline' in available:
        eq = np.array(available['baseline'])
        peak = np.maximum.accumulate(eq)
        dd = (eq - peak) / peak * 100
        ax2.plot(np.arange(len(dd)), dd, color='#888888', linewidth=0.8, alpha=0.7, label='Baseline DD (PCT=75)')
    
    ax2.set_ylabel('Drawdown (%)', fontsize=11)
    ax2.set_xlabel('Bar (walk-forward compounded)', fontsize=11)
    ax2.legend(loc='lower left', fontsize=8)
    ax2.grid(True, alpha=0.3)
    
    plt.tight_layout()
    plt.savefig(OUTPUT_PNG, dpi=150, bbox_inches='tight')
    print(f"Chart saved to {OUTPUT_PNG}")
    plt.close()


def plot_sweep_heatmap(sweep):
    """Plot pass rate and Sharpe across hedge_pct values."""
    fig, (ax1, ax2) = plt.subplots(2, 1, figsize=(14, 6), sharex=True)
    
    pcts = [r['hedge_pct'] for r in sweep]
    pass_rates = [r['pass_rate'] for r in sweep]
    sharpes = [r['avg_sharpe'] for r in sweep]
    
    ax1.bar(pcts, pass_rates, color='#2196F3', alpha=0.7, width=1.0)
    ax1.set_ylabel('Pass Rate (%)', fontsize=11)
    ax1.set_title('USDT Hedge Threshold Sweep — Pass Rate & Sharpe (AP17/VL92)', fontsize=14, fontweight='bold')
    ax1.axvline(x=75, color='red', linestyle='--', linewidth=1.5, label='Prior baseline (75)')
    best_pct = max(sweep, key=lambda r: (r['pass_count'], r['avg_sharpe']))['hedge_pct']
    ax1.axvline(x=best_pct, color='green', linestyle='--', linewidth=1.5, label=f'Winner ({best_pct})')
    ax1.legend(fontsize=9)
    ax1.grid(True, alpha=0.3, axis='y')
    
    ax2.bar(pcts, sharpes, color='#FF9800', alpha=0.7, width=1.0)
    ax2.set_ylabel('Avg Sharpe', fontsize=11)
    ax2.set_xlabel('HEDGE_PCT (percentile threshold)', fontsize=11)
    ax2.axvline(x=75, color='red', linestyle='--', linewidth=1.5)
    ax2.axvline(x=best_pct, color='green', linestyle='--', linewidth=1.5)
    ax2.grid(True, alpha=0.3, axis='y')
    
    plt.tight_layout()
    plt.savefig(HEATMAP_PNG, dpi=150, bbox_inches='tight')
    print(f"Heatmap saved to {HEATMAP_PNG}")
    plt.close()


if __name__ == '__main__':
    os.chdir(os.path.dirname(os.path.abspath(__file__)) + '/..')
    
    if not os.path.exists(SWEEP_CSV):
        print(f"ERROR: {SWEEP_CSV} not found. Run the Rust harness first.")
        sys.exit(1)
    
    sweep = load_sweep()
    winner, runners = find_winner_and_runners(sweep)
    
    print(f"Winner: PCT={winner['hedge_pct']}, Pass={winner['pass_count']}/{winner['total']} ({winner['pass_rate']:.1f}%), Sharpe={winner['avg_sharpe']:.3f}")
    print(f"Baseline (75): Pass={sweep[75]['pass_count']}/{sweep[75]['total']} ({sweep[75]['pass_rate']:.1f}%), Sharpe={sweep[75]['avg_sharpe']:.3f}")
    
    # Plot sweep heatmap
    plot_sweep_heatmap(sweep)
    
    # Plot equity curves
    if os.path.exists(TS_CSV):
        curves = load_timeseries()
        plot_equity_curves(curves, winner, runners)
    else:
        print(f"WARNING: {TS_CSV} not found. Skipping equity curve chart.")
    
    print("Done!")
