#!/usr/bin/env python3
"""
Freshness Cooldown Hyperopt - Equity Curve Comparison Chart

Generates a comparison chart showing equity curves for:
- Baseline: cooldown=0 (current hardcoded default)
- Winner: cooldown=53 (best pass rate in T70)
- Runner-ups: cooldown=28, 55, 85

Data source: snapshots/t70_freshness_cooldown_equity.csv
"""

import pandas as pd
import matplotlib.pyplot as plt
import matplotlib.ticker as mticker
import numpy as np
from pathlib import Path

# Absolute paths
WORKSPACE_ROOT = Path("/home/ubuntu/.openclaw/workspace-krypto")
DATA_FILE = WORKSPACE_ROOT / "krypto/snapshots/t70_freshness_cooldown_equity.csv"
OUTPUT_CHART = WORKSPACE_ROOT / "charts/comparison_chart.png"

# Candidates to plot
CANDIDATES = {
    0: "Baseline (FC=0)",      # Current hardcoded default
    53: "Winner (FC=53)",      # Best pass rate 96.7%
    28: "Runner-up 1 (FC=28)", # 85% pass
    55: "Runner-up 2 (FC=55)", # 95% pass
    85: "Runner-up 3 (FC=85)", # 70% pass (variety)
}

COLORS = {
    0: "#e74c3c",    # Red - baseline
    53: "#27ae60",    # Green - winner
    28: "#3498db",    # Blue
    55: "#9b59b6",   # Purple
    85: "#f39c12",    # Orange
}

def load_equity_curves(data_file: Path) -> dict:
    """Load equity curves for each cooldown value."""
    df = pd.read_csv(data_file)
    
    curves = {}
    for cd in CANDIDATES.keys():
        subset = df[df['cooldown'] == cd].copy()
        if not subset.empty:
            # Sort by step to get chronological order
            subset = subset.sort_values('step')
            curves[cd] = subset['equity'].values
    
    return curves

def compute_metrics(curves: dict) -> pd.DataFrame:
    """Compute summary metrics from equity curves."""
    summary_file = WORKSPACE_ROOT / "krypto/snapshots/t70_freshness_cooldown_summary.csv"
    if summary_file.exists():
        df = pd.read_csv(summary_file)
        return df[df['cooldown'].isin(CANDIDATES.keys())]
    return pd.DataFrame()

def plot_equity_comparison(curves: dict, metrics: pd.DataFrame):
    """Create comparison chart with equity curves."""
    
    fig, (ax1, ax2) = plt.subplots(2, 1, figsize=(14, 10), 
                                gridspec_kw={'height_ratios': [3, 1]})
    
    # Plot equity curves (log scale)
    for cd, label in CANDIDATES.items():
        if cd in curves and len(curves[cd]) > 0:
            equity = curves[cd]
            x = np.arange(len(equity))
            
            # Use log scale for equity
            ax1.semilogy(x, equity, label=label, color=COLORS[cd], linewidth=1.5, alpha=0.85)
    
    ax1.set_xlabel("Trading Days", fontsize=12)
    ax1.set_ylabel("Portfolio Equity (log scale)", fontsize=12)
    ax1.set_title("FRESHNESS_COOLDOWN Hyperopt: Equity Curve Comparison\n(T70 9-universe walk-forward, HOLD_MAX=12/CHAND=dual-exit)", 
                fontsize=14, fontweight='bold')
    ax1.legend(loc='upper left', fontsize=10)
    ax1.grid(True, alpha=0.3, which='both')
    ax1.set_xlim(0, 1800)
    
    # Add pass rate annotation from metrics
    if not metrics.empty:
        pass_text = " | ".join([f"{row['cooldown']}: {row['pass_rate_pct']:.1f}%" 
                           for _, row in metrics.iterrows()])
        ax1.annotate(f"Pass rates: {pass_text}", 
                  xy=(0.98, 0.02), xycoords='axes fraction',
                  fontsize=9, ha='right', va='bottom',
                  bbox=dict(boxstyle='round', facecolor='wheat', alpha=0.5))
    
    # Drawdown comparison (bar chart)
    if not metrics.empty:
        cd_values = [metrics[metrics['cooldown'] == cd]['avg_max_dd_pct'].values[0] 
                   for cd in CANDIDATES.keys() 
                   if cd in metrics['cooldown'].values]
        labels = [f"FC={cd}" for cd in CANDIDATES.keys() 
                 if cd in metrics['cooldown'].values]
        
        bar_colors = [COLORS.get(cd, 'gray') for cd in CANDIDATES.keys() 
                    if cd in metrics['cooldown'].values]
        
        bars = ax2.bar(labels, cd_values, color=bar_colors, alpha=0.7, edgecolor='black')
        ax2.set_ylabel("Max Drawdown %", fontsize=11)
        ax2.set_xlabel("Cooldown Value", fontsize=11)
        ax2.set_title("Max Drawdown by Cooldown Value", fontsize=12)
        ax2.grid(True, alpha=0.3, axis='y')
        
        # Annotate bars with values
        for bar, val in zip(bars, cd_values):
            height = bar.get_height()
            ax2.annotate(f'{val:.1f}%',
                       xy=(bar.get_x() + bar.get_width() / 2, height),
                       xytext=(0, 3), textcoords="offset points",
                       ha='center', va='bottom', fontsize=9)
    
    plt.tight_layout()
    
    # Save chart
    OUTPUT_CHART.parent.mkdir(parents=True, exist_ok=True)
    plt.savefig(OUTPUT_CHART, dpi=150, bbox_inches='tight', 
               facecolor='white', edgecolor='none')
    print(f"Chart saved to {OUTPUT_CHART}")
    
    return fig

def main():
    print("Loading equity curves...")
    curves = load_equity_curves(DATA_FILE)
    print(f"Loaded {len(curves)} candidate curves")
    
    # Get metrics
    metrics = compute_metrics(curves)
    print(f"Loaded metrics for {len(metrics)} candidates")
    
    # Plot
    print("Generating comparison chart...")
    fig = plot_equity_comparison(curves, metrics)
    
    # Print summary
    print("\n" + "="*60)
    print("FRESHNESS_COOLDOWN Hyperopt Summary")
    print("="*60)
    if not metrics.empty:
        for _, row in metrics.iterrows():
            cd = row['cooldown']
            label = CANDIDATES.get(cd, f"FC={cd}")
            print(f"{label:20s} | Pass: {row['pass_rate_pct']:5.1f}% | "
                  f"Sharpe: {row['avg_sharpe']:5.2f} | "
                  f"DD: {row['avg_max_dd_pct']:5.2f}% | "
                  f"Trades: {row['total_trades']:4.0f}")
    
    print(f"\nChart: {OUTPUT_CHART}")
    print("="*60)

if __name__ == "__main__":
    main()