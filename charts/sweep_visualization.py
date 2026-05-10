#!/usr/bin/env python3
"""
Hyperparameter Sweep Visualization
Reads parameter sweep CSVs and generates comparison chart.
"""

import pandas as pd
import matplotlib.pyplot as plt
import numpy as np
import os

CHARTS_DIR = "/home/ubuntu/.openclaw/workspace-krypto/charts"
SNAPSHOTS_DIR = "/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots"

def load_sweep_data():
    """Load available sweep results."""
    files = [
        "hold_max_current_full_summary.csv",
        "t83_hedge_size_mult_summary.csv",
        "regime_ap_sweep_summary.csv",
    ]
    
    for f in files:
        path = os.path.join(SNAPSHOTS_DIR, f)
        if os.path.exists(path):
            print(f"Loading {f}")
            try:
                return pd.read_csv(path), f
            except Exception as e:
                print(f"Error: {e}")
                continue
    
    return pd.DataFrame(), "sample"

def normalize_columns(df):
    """Normalize column names."""
    rename = {}
    for col in df.columns:
        c = col.lower()
        if 'hold' in c or c == 'fc' or 'hedge' in c:
            rename[col] = 'param'
        elif 'pass' in c and 'pct' in c:
            rename[col] = 'pass_pct'
        elif 'sharpe' in c:
            rename[col] = 'avg_sharpe'
        elif 'return' in c and 'pct' in c:
            rename[col] = 'avg_return'
        elif 'max_dd' in c or 'drawdown' in c:
            rename[col] = 'avg_dd'
    
    df = df.rename(columns=rename)
    if 'param' not in df.columns and len(df.columns) > 0:
        df['param'] = range(len(df))
    return df

def plot_sweep_results(df, name):
    """Generate parameter sweep chart."""
    df = normalize_columns(df)
    
    if 'param' not in df.columns:
        print(f"No param column. Available: {df.columns.tolist()}")
        return
    
    fig, axes = plt.subplots(2, 2, figsize=(14, 10))
    fig.suptitle(f'Hyperparameter Sweep: {name}', fontsize=14, fontweight='bold')
    
    x = df['param']
    
    if 'pass_pct' in df.columns:
        axes[0,0].plot(x, df['pass_pct'], 'b-o', lw=2, ms=6)
        axes[0,0].set_ylabel('Pass Rate (%)')
        axes[0,0].set_title('Walk-Forward Pass')
        axes[0,0].grid(True, alpha=0.3)
    
    if 'avg_sharpe' in df.columns:
        axes[0,1].plot(x, df['avg_sharpe'], 'g-o', lw=2, ms=6)
        axes[0,1].set_ylabel('Sharpe')
        axes[0,1].set_title('Sharpe Ratio')
        axes[0,1].grid(True, alpha=0.3)
        best = df['avg_sharpe'].idxmax()
        wVal = df.loc[best, 'param']
        axes[0,1].scatter([wVal], [df.loc[best, 'avg_sharpe']], color='red', s=150, marker='*', label=f'Winner: {wVal}')
        axes[0,1].legend()
    
    if 'avg_return' in df.columns:
        axes[1,0].plot(x, df['avg_return'], 'r-o', lw=2, ms=6)
        axes[1,0].set_ylabel('Return (%)')
        axes[1,0].set_title('Return')
        axes[1,0].grid(True, alpha=0.3)
    
    if 'avg_dd' in df.columns:
        axes[1,1].plot(x, df['avg_dd'], 'm-o', lw=2, ms=6)
        axes[1,1].set_ylabel('Max DD (%)')
        axes[1,1].set_title('Drawdown')
        axes[1,1].grid(True, alpha=0.3)
    
    for ax in axes.flat:
        ax.set_xlabel(df['param'].name)
    
    plt.tight_layout()
    out = os.path.join(CHARTS_DIR, 'comparison_chart.png')
    plt.savefig(out, dpi=150, bbox_inches='tight')
    print(f"Chart: {out}")

def main():
    print(f"=== Sweep Visualization ===")
    os.makedirs(CHARTS_DIR, exist_ok=True)
    df, name = load_sweep_data()
    if not df.empty:
        print(f"Rows: {len(df)}")
        plot_sweep_results(df, name)
    else:
        print("No data found")

if __name__ == "__main__":
    main()