#!/usr/bin/env python3
"""
ATR Rank Threshold Sweep — Equity Curve Comparison Chart
Baseline (T=0, no filter) vs Winner (T=5) vs Production (AP=12/LB=42/T=5)

Data: snapshots/atr_rank_filter_prod_equity.csv
Sweep: T ∈ {0,5,10,...,100} step 5 (21 values) × 9 universes × 6 windows
Params: CHAND(7,2.30)/EP=21/HM=12/CAP=3/VL=96/ATR(24,2.0)
"""
import csv
import os
import sys

import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import matplotlib.ticker as mticker
import numpy as np

EQUITY_CSV = "snapshots/atr_rank_filter_prod_equity.csv"
RESULTS_CSV = "snapshots/atr_rank_filter_prod_results.csv"
OUT_PNG = "charts/atr_rank_filter_comparison.png"

# Color scheme
COLOR_BASELINE = "#2196F3"   # blue
COLOR_WINNER    = "#4CAF50"   # green
COLOR_PROD     = "#FF9800"   # orange

# Top candidates by pass rate + Sharpe
CANDIDATES = {
    "Baseline (T=0)":    (0,   COLOR_BASELINE),
    "Winner (T=5)":      (5,   COLOR_WINNER),
    "Runner-up (T=10)":  (10,  "#9C27B0"),  # purple
    "Runner-up (T=85)":  (85,  "#795548"),  # brown
}

UNIVERSE_NAMES = [
    "Base5", "NoDOGE", "Legacy4", "Legacy5BNB",
    "OldGuardNoBNB", "LargeCaps5", "Legacy3",
    "LowVolume5", "OldGuard4"
]
WINDOWS_PER_UNIVERSE = 6

def load_summary():
    """Load the results CSV keyed by (universe, window, threshold)."""
    results = {}
    if not os.path.exists(RESULTS_CSV):
        return results
    with open(RESULTS_CSV) as f:
        reader = csv.DictReader(f)
        for row in reader:
            try:
                uname = row.get('universe', '').strip()
                wi = int(row.get('window', -1))
                t = int(row.get('threshold', -1))
                key = (uname, wi, t)
                results[key] = {
                    'pass': int(row.get('pass', 0)),
                    'sharpe': float(row.get('sharpe', 0)),
                    'ret': float(row.get('return_pct', 0)),
                    'dd': float(row.get('max_dd_pct', 0)),
                    'trades': int(row.get('trades', 0)),
                    'win_rate': float(row.get('win_rate', 0)),
                }
            except:
                pass
    return results

def load_equity_csv(path):
    """Load equity CSV: step, col1, col2, ...
    Columns are Universe_Window_T<threshold>
    Returns dict: (uname, window, threshold) -> list of equity values
    """
    if not os.path.exists(path):
        return {}
    
    with open(path) as f:
        reader = csv.reader(f)
        header = next(reader)
    
    # Parse column names: e.g. "Base5_W0_T5"
    col_map = {}
    for i, col in enumerate(header):
        if i == 0:
            continue
        parts = col.rsplit('_T', 1)
        if len(parts) == 2:
            base = parts[0]
            t = int(parts[1])
            # Parse universe and window: e.g. "Base5_W0"
            u_parts = base.rsplit('_W', 1)
            if len(u_parts) == 2:
                uname = u_parts[0]
                wi = int(u_parts[1])
                col_map[i] = (uname, wi, t)
    
    # Read rows
    equity_data = {}  # (uname, wi, t) -> list of floats
    with open(path) as f:
        reader = csv.reader(f)
        next(reader)  # skip header
        for row in reader:
            if not row or row[0] == '':
                continue
            for i, val in enumerate(row[1:], start=1):
                if i in col_map:
                    key = col_map[i]
                    if key not in equity_data:
                        equity_data[key] = []
                    try:
                        equity_data[key].append(float(val))
                    except:
                        pass
    
    return equity_data

def aggregate_equity(equity_data, uname, t):
    """Average equity curves across all windows for a given universe and threshold."""
    curves = []
    for wi in range(WINDOWS_PER_UNIVERSE):
        key = (uname, wi, t)
        if key in equity_data:
            curves.append(equity_data[key])
    
    if not curves:
        return None
    
    # Find common length
    min_len = min(len(c) for c in curves)
    if min_len == 0:
        return None
    
    # Truncate and average
    truncated = [c[:min_len] for c in curves]
    return np.mean(truncated, axis=0)

def aggregate_global(equity_data, t):
    """Average equity curves across ALL universes and windows for a given threshold."""
    curves = []
    for (uname, wi, t_val), curve in equity_data.items():
        if t_val == t and len(curve) > 0:
            curves.append(curve)
    
    if not curves:
        return None
    
    min_len = min(len(c) for c in curves)
    if min_len == 0:
        return None
    
    truncated = [c[:min_len] for c in curves]
    return np.mean(truncated, axis=0)

def compute_summary_stats(results, t):
    """Compute aggregated stats for a given threshold across all universes/windows."""
    pass_list = []
    sharpe_list = []
    ret_list = []
    dd_list = []
    trades_list = []
    
    for (uname, wi, t_val), d in results.items():
        if t_val == t:
            pass_list.append(d['pass'])
            sharpe_list.append(d['sharpe'])
            ret_list.append(d['ret'])
            dd_list.append(d['dd'])
            trades_list.append(d['trades'])
    
    if not pass_list:
        return None
    
    return {
        'pass_rate': sum(pass_list) / len(pass_list),
        'avg_sharpe': np.mean(sharpe_list),
        'avg_ret': np.mean(ret_list),
        'avg_dd': np.mean(dd_list),
        'avg_trades': np.mean(trades_list),
        'n': len(pass_list),
    }

def main():
    print("Loading equity data...")
    equity_data = load_equity_csv(EQUITY_CSV)
    print(f"  Loaded {len(equity_data)} equity curves")
    
    print("Loading summary results...")
    results = load_summary()
    print(f"  Loaded {len(results)} result entries")
    
    # =====================================================================
    # PANEL 1: Global equity curve comparison (all universes averaged)
    # =====================================================================
    fig, axes = plt.subplots(2, 1, figsize=(14, 10), sharex=False)
    fig.suptitle(
        "ATR Rank Entry Threshold Sweep — Equity Curve Comparison\n"
        "Params: EP=21, CHAND(7,2.30), HM=12, CAP=3, VL=96, ATR(24,2.0) | 9 Universes × 6 WF Windows",
        fontsize=13, fontweight='bold', y=0.98
    )
    
    ax1 = axes[0]
    ax2 = axes[1]
    
    # Thresholds to plot (interesting ones)
    thresholds_to_plot = [0, 5, 10, 15, 85]
    colors = [COLOR_BASELINE, COLOR_WINNER, "#9C27B0", "#795548", "#607D8B"]
    
    plotted_lines = {}
    for (label, (t, color)) in CANDIDATES.items():
        global_eq = aggregate_global(equity_data, t)
        if global_eq is not None:
            steps = np.arange(len(global_eq))
            line, = ax1.plot(steps, global_eq, color=color, linewidth=2.0,
                           label=f"{label} (T={t})", alpha=0.9)
            plotted_lines[label] = line
    
    # Log scale for equity
    ax1.set_yscale('log')
    ax1.yaxis.set_major_formatter(mticker.FuncFormatter(lambda y, _: f'{y:.2f}'))
    ax1.set_ylabel('Portfolio Equity (log scale)', fontsize=11)
    ax1.set_title('Global Equity Curve (all universes averaged)', fontsize=11)
    ax1.legend(loc='upper left', fontsize=9)
    ax1.grid(True, alpha=0.3, which='both')
    ax1.set_xlim(0, None)
    
    # =====================================================================
    # PANEL 2: Per-universe subplot for key candidates
    # =====================================================================
    # Show Base5 (our production universe) equity for Baseline, Winner, T=10
    for_unames = ['Base5', 'NoDOGE', 'Legacy4']
    ax2_title_parts = []
    
    line_styles = {'Baseline (T=0)': '-', 'Winner (T=5)': '-', "Runner-up (T=10)": '--'}
    
    for (label, (t, color)) in CANDIDATES.items():
        for uname in for_unames:
            eq = aggregate_equity(equity_data, uname, t)
            if eq is not None:
                ls = line_styles.get(label, '-')
                alpha = 0.9 if label == 'Baseline (T=0)' else (0.7 if label == 'Winner (T=5)' else 0.5)
                ax2.plot(np.arange(len(eq)), eq, color=color, linewidth=1.5,
                        linestyle=ls, label=f'{uname}/{label}', alpha=alpha)
    
    ax2.set_yscale('log')
    ax2.yaxis.set_major_formatter(mticker.FuncFormatter(lambda y, _: f'{y:.2f}'))
    ax2.set_ylabel('Portfolio Equity (log scale)', fontsize=11)
    ax2.set_title('Base5 + Key Universes Equity (Baseline vs Winner vs Runner-up)', fontsize=11)
    ax2.legend(loc='upper left', fontsize=8, ncol=2)
    ax2.grid(True, alpha=0.3, which='both')
    ax2.set_xlabel('Test Bar (252-bar OOS window)', fontsize=11)
    ax2.set_xlim(0, None)
    
    plt.tight_layout(rect=[0, 0, 1, 0.95])
    
    os.makedirs("charts", exist_ok=True)
    plt.savefig(OUT_PNG, dpi=150, bbox_inches='tight', facecolor='white')
    print(f"Chart saved: {OUT_PNG}")
    
    # =====================================================================
    # Summary stats table
    # =====================================================================
    print("\n=== Summary Statistics by Threshold ===")
    print(f"{'T':>4} {'pass%':>7} {'avg_sharpe':>10} {'avg_ret%':>9} {'avg_DD%':>8} {'avg_trades':>10}")
    print('-' * 60)
    
    all_thresholds = sorted(set(t for (_, _, t) in equity_data.keys()))
    stats_data = []
    for t in all_thresholds:
        s = compute_summary_stats(results, t)
        if s is not None:
            stats_data.append((t, s))
            print(f"{t:>4} {s['pass_rate']*100:>6.1f}% {s['avg_sharpe']:>10.3f} {s['avg_ret']:>9.1f} {s['avg_dd']:>8.1f} {s['avg_trades']:>10.0f}")
    
    print("\n=== Key Comparison ===")
    for (label, (t, color)) in CANDIDATES.items():
        s = compute_summary_stats(results, t)
        if s:
            print(f"{label}: {s['pass_rate']*100:.1f}% pass, Sharpe {s['avg_sharpe']:.3f}, ret {s['avg_ret']:.1f}%, DD {s['avg_dd']:.1f}%")

if __name__ == '__main__':
    main()
