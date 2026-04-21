#!/usr/bin/env python3
"""
Hyperopt Equity Comparison Chart Generator
Reads equity curve CSV from hold_max sweeps and produces comparison PNG.

Usage: python3 charts/hyperopt_equity_chart.py [--input snapshots/hold_max_9way_equity.csv]
"""

import argparse
import pandas as pd
import numpy as np
import matplotlib.pyplot as plt
import matplotlib.ticker as mticker
from pathlib import Path

def load_equity_csv(path: str) -> pd.DataFrame:
    df = pd.read_csv(path)
    # Expected columns: hm, universe, window, step, equity
    return df

def compute_composite_equity(df: pd.DataFrame, hm_value: int) -> pd.Series:
    """
    Average equity across all (universe, window) combos for a given HM value.
    Normalize to start at 1.0.
    """
    sub = df[df['hm'] == hm_value].copy()
    if sub.empty:
        return None
    
    # Group by (universe, window, step) — take mean if duplicates
    grouped = sub.groupby(['universe', 'window', 'step'])['equity'].mean()
    
    # Average across universes and windows at each step
    composite = grouped.groupby('step').mean()
    
    # Normalize to start at 1.0
    if composite.iloc[0] > 0:
        composite = composite / composite.iloc[0]
    
    return composite

def compute_global_aggregate(df: pd.DataFrame) -> dict:
    """
    Compute pass rate, avg Sharpe, avg return per HM value.
    """
    # Load the metrics CSV if available
    metrics_path = Path("snapshots/hold_max_9way_summary.csv")
    if metrics_path.exists():
        sm = pd.read_csv(metrics_path)
        return {row['hm']: {'pass_rate': row['pass_rate'], 'avg_sharpe': row['avg_sharpe'], 
                            'avg_ret': row['avg_ret']} for _, row in sm.iterrows()}
    return {}

def plot_comparison(df: pd.DataFrame, hms_to_plot: list, baseline_hm: int, output_path: str):
    """
    Plot equity curves for a set of HM values.
    Baseline = current production default (HM=45)
    Winner = best HM from the sweep
    Runner-ups = 2nd and 3rd best
    """
    fig, (ax1, ax2) = plt.subplots(2, 1, figsize=(14, 10), 
                                     gridspec_kw={'height_ratios': [3, 1]})
    
    colors = ['#2196F3', '#4CAF50', '#FF9800', '#9C27B0', '#F44336', '#00BCD4']
    
    for i, hm in enumerate(hms_to_plot):
        composite = compute_composite_equity(df, hm)
        if composite is None:
            continue
        
        label = f"HM={hm}"
        if hm == baseline_hm:
            label += " (baseline)"
        linestyle = '-' if hm != baseline_hm else '--'
        linewidth = 2.5 if hm != baseline_hm else 1.8
        
        ax1.plot(composite.index, composite.values, 
                 label=label, color=colors[i % len(colors)],
                 linestyle=linestyle, linewidth=linewidth, alpha=0.9)
    
    ax1.set_title("HOLD_MAX Hyperopt — Composite Equity Curves\n(9-universe average, normalized)", 
                  fontsize=14, fontweight='bold')
    ax1.set_ylabel("Equity (normalized, start=1.0)", fontsize=11)
    ax1.set_xlabel("Time Step (bar index)", fontsize=11)
    ax1.legend(loc='upper left', fontsize=10)
    ax1.grid(True, alpha=0.3)
    
    # Log scale for equity
    ax1.set_yscale('log')
    ax1.yaxis.set_major_formatter(mticker.FuncFormatter(lambda x, _: f'{x:.2f}x'))
    
    # Drawdown panel
    for i, hm in enumerate(hms_to_plot):
        composite = compute_composite_equity(df, hm)
        if composite is None:
            continue
        
        peak = composite.cummax()
        dd = (composite - peak) / peak * 100  # percentage drawdown
        
        label = f"HM={hm}"
        if hm == baseline_hm:
            label += " (baseline)"
        linestyle = '-' if hm != baseline_hm else '--'
        
        ax2.fill_between(dd.index, dd.values, 0, 
                         alpha=0.15, color=colors[i % len(colors)])
        ax2.plot(dd.index, dd.values, 
                 label=label, color=colors[i % len(colors)],
                 linestyle=linestyle, linewidth=1.5, alpha=0.9)
    
    ax2.set_title("Drawdown (linear scale)", fontsize=11)
    ax2.set_ylabel("Drawdown %", fontsize=10)
    ax2.set_xlabel("Time Step (bar index)", fontsize=10)
    ax2.legend(loc='lower left', fontsize=9)
    ax2.grid(True, alpha=0.3)
    ax2.set_ylim(bottom=-80, top=0)
    
    plt.tight_layout()
    plt.savefig(output_path, dpi=150, bbox_inches='tight', 
                facecolor='white', edgecolor='none')
    plt.close()
    print(f"Saved: {output_path}")

def main():
    parser = argparse.ArgumentParser(description='Hyperopt equity comparison chart')
    parser.add_argument('--input', default='snapshots/hold_max_9way_equity.csv',
                        help='Path to equity curve CSV')
    parser.add_argument('--output', default='charts/hyperopt_equity_comparison.png',
                        help='Output PNG path')
    parser.add_argument('--baseline', type=int, default=45,
                        help='Baseline HM value (current production default)')
    parser.add_argument('--top-n', type=int, default=5,
                        help='Number of top HM values to plot')
    args = parser.parse_args()
    
    Path(args.output).parent.mkdir(parents=True, exist_ok=True)
    
    print(f"Loading: {args.input}")
    df = load_equity_csv(args.input)
    
    # Get all HM values sorted
    all_hms = sorted(df['hm'].unique())
    print(f"HM values in data: {all_hms}")
    
    # Compute pass rates / Sharpe for ranking
    global_agg = compute_global_aggregate(df)
    
    # If no metrics CSV, rank by final equity
    ranked = []
    for hm in all_hms:
        composite = compute_composite_equity(df, hm)
        if composite is not None and len(composite) > 0:
            final_equity = composite.iloc[-1]
            ranked.append((hm, final_equity))
    
    ranked.sort(key=lambda x: x[1], reverse=True)
    print(f"Ranked by final equity: {ranked[:5]}")
    
    # Plot: baseline + winner + 2 runner-ups
    winner = ranked[0][0]
    runnerups = [r[0] for r in ranked[1:3]]
    
    hms_to_plot = [args.baseline, winner] + runnerups
    hms_to_plot = [h for h in hms_to_plot if h in all_hms][:5]
    
    print(f"Plotting: {hms_to_plot}")
    print(f"  Baseline: HM={args.baseline}")
    print(f"  Winner: HM={winner}")
    print(f"  Runner-ups: {runnerups}")
    
    plot_comparison(df, hms_to_plot, args.baseline, args.output)
    
    # Print summary table
    print("\n=== HOLD_MAX Summary ===")
    print(f"{'HM':>5} {'Pass%':>8} {'AvgSharpe':>10} {'FinalEquity':>12} {'Status'}")
    print("-" * 50)
    for hm, eq in ranked[:8]:
        agg = global_agg.get(hm, {})
        pass_rate = agg.get('pass_rate', 0)
        sharpe = agg.get('avg_sharpe', 0)
        status = ""
        if hm == winner: status = "← WINNER"
        elif hm == args.baseline: status = "← BASELINE"
        print(f"  {hm:>3} {pass_rate:>7.1f}% {sharpe:>10.4f} {eq:>12.4f}  {status}")

if __name__ == '__main__':
    main()