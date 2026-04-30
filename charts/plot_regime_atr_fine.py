#!/usr/bin/env python3
"""
Regime ATR Fine-Sweep Comparison Chart
Reads equity curves from snapshots/regime_atr_equity/ and generates comparison chart.
"""
import pandas as pd
import numpy as np
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import matplotlib.ticker as mticker
import os
import glob

EQUITY_DIR = "snapshots/regime_atr_equity/"
OUT_DIR = "charts/"

# Configs to compare: (label, AP, LB, T, color)
CONFIGS = [
    ("Baseline (AP=12,LB=42,T=0)", 12, 42, 0, "#cccccc", "--"),
    ("Winner (AP=12,LB=42,T=5)", 12, 42, 5, "#2196F3", "-"),
    ("Runner-up A (AP=8,LB=42,T=5)", 8, 42, 5, "#4CAF50", "-."),
    ("Runner-up B (AP=56,LB=42,T=5)", 56, 42, 5, "#FF9800", ":"),
    ("Runner-up C (AP=8,LB=126,T=5)", 8, 126, 5, "#9C27B0", "--"),
]

def load_equity(ap, lb, t):
    fname = f"{EQUITY_DIR}equity_AP{ap:03d}_LB{lb:03d}_T{t:03d}.csv"
    if not os.path.exists(fname):
        return None
    df = pd.read_csv(fname)
    return df

def compute_drawdown(equity):
    peak = equity.cummax()
    dd = (equity - peak) / peak
    return dd * 100

def main():
    os.makedirs(OUT_DIR, exist_ok=True)
    
    # Load all configs
    curves = {}
    for label, ap, lb, t, color, ls in CONFIGS:
        df = load_equity(ap, lb, t)
        if df is not None and len(df) > 0:
            curves[label] = {
                'equity': df['equity'].values,
                'color': color,
                'linestyle': ls,
                'final': df['equity'].iloc[-1]
            }
            print(f"Loaded {label}: {len(df)} bars, final={df['equity'].iloc[-1]:.4f}")
        else:
            print(f"MISSING: {label} ({ap}/{lb}/{t})")
    
    if not curves:
        print("No equity data found!")
        return
    
    # Determine max length for plotting
    max_len = max(len(c['equity']) for c in curves.values())
    
    # Pad all curves to max_len
    for name, c in curves.items():
        arr = c['equity']
        if len(arr) < max_len:
            pad = np.full(max_len - len(arr), np.nan)
            arr = np.concatenate([pad, arr])
        c['equity_padded'] = arr
    
    # Create figure with 2 subplots: equity (log) + drawdown (linear)
    fig, (ax1, ax2) = plt.subplots(2, 1, figsize=(14, 9), 
                                     gridspec_kw={'height_ratios': [3, 1.2]},
                                     sharex=True)
    fig.suptitle("Regime ATR Period Fine-Sweep: Equity Curves & Drawdown", 
                 fontsize=14, fontweight='bold', y=0.98)
    
    # Plot equity curves (log scale)
    for name, c in curves.items():
        arr = c['equity_padded']
        steps = np.arange(len(arr))
        # Replace 0/nan at start with first valid value for log plot
        first_valid = np.where(~np.isnan(arr))[0][0] if np.any(~np.isnan(arr)) else 0
        arr_safe = arr.copy()
        arr_safe[:first_valid] = np.nan
        ax1.plot(steps, arr_safe, label=f"{name} ({c['final']:.2f}x)", 
                 color=c['color'], linestyle=c['linestyle'], linewidth=1.8)
    
    ax1.set_ylabel("Portfolio Value (log scale)", fontsize=11)
    ax1.set_yscale('log')
    ax1.yaxis.set_major_formatter(mticker.FuncFormatter(lambda x, _: f'{x:.1f}x'))
    ax1.grid(True, alpha=0.3, which='both')
    ax1.legend(loc='upper left', fontsize=9, framealpha=0.9)
    ax1.set_title("Equity Curves (log scale)", fontsize=10, loc='left')
    
    # Plot drawdown (linear)
    for name, c in curves.items():
        arr = c['equity_padded']
        dd = compute_drawdown(pd.Series(arr))
        steps = np.arange(len(dd))
        ax2.fill_between(steps, dd, 0, alpha=0.15, color=c['color'])
        ax2.plot(steps, dd, label=name, color=c['color'], 
                 linestyle=c['linestyle'], linewidth=1.2)
    
    ax2.set_ylabel("Drawdown (%)", fontsize=11)
    ax2.set_xlabel("Bar", fontsize=11)
    ax2.set_ylim(-100, 5)
    ax2.grid(True, alpha=0.3)
    ax2.set_title("Drawdown (linear scale)", fontsize=10, loc='left')
    
    # Add caption with key metrics
    caption = "Regime ATR Period Fine-Sweep | AP=12/LB=42/T=5 winner | Coarse grid 56×6×8=2688 configs | Source: regime_atr_hyperopt.rs"
    fig.text(0.5, 0.01, caption, ha='center', fontsize=8, color='gray')
    
    plt.tight_layout(rect=[0, 0.03, 1, 0.97])
    out_path = f"{OUT_DIR}regime_atr_fine_sweep.png"
    plt.savefig(out_path, dpi=150, bbox_inches='tight', facecolor='white')
    plt.close()
    print(f"\nSaved: {out_path}")
    
    # Also create a bar chart of final equity values
    fig2, ax = plt.subplots(figsize=(10, 5))
    labels = [name.split('(')[1].replace(')', '') for name in curves.keys()]
    finals = [c['final'] for c in curves.values()]
    colors = [c['color'] for c in curves.values()]
    bars = ax.bar(range(len(finals)), finals, color=colors, alpha=0.8, edgecolor='black')
    ax.set_xticks(range(len(finals)))
    ax.set_xticklabels(list(curves.keys()), rotation=15, ha='right', fontsize=9)
    ax.set_ylabel("Final Portfolio Value (x)", fontsize=11)
    ax.set_title("Regime ATR Fine-Sweep: Final Equity Comparison", fontsize=12, fontweight='bold')
    ax.yaxis.set_major_formatter(mticker.FuncFormatter(lambda x, _: f'{x:.2f}x'))
    for bar, val in zip(bars, finals):
        ax.text(bar.get_x() + bar.get_width()/2, bar.get_height() + 0.05, 
                f'{val:.2f}x', ha='center', va='bottom', fontsize=9, fontweight='bold')
    ax.grid(True, alpha=0.3, axis='y')
    plt.tight_layout()
    bar_path = f"{OUT_DIR}regime_atr_fine_sweep_bar.png"
    plt.savefig(bar_path, dpi=150, bbox_inches='tight', facecolor='white')
    plt.close()
    print(f"Saved: {bar_path}")

if __name__ == "__main__":
    main()
