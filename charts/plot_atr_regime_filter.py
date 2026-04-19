#!/usr/bin/env python3
"""
ATR Percentile Regime Filter Comparison Chart
Compares equity curves for threshold=0 (baseline) vs threshold=20 (winner) vs threshold=50 (high filter)
per universe, showing the impact of the regime filter on equity curves.
"""
import csv
import sys
import os
import glob

def read_equity_csv(path):
    """Read equity CSV and return dict of col_name -> list of float values."""
    with open(path, 'r') as f:
        reader = csv.DictReader(f)
        rows = list(reader)
    
    if not rows:
        return {}
    
    # Get step values
    steps = [int(r['step']) for r in rows]
    cols = {k: [] for k in rows[0].keys() if k != 'step'}
    
    for row in rows:
        for k in cols:
            v = row[k]
            cols[k].append(float(v) if v else float('nan'))
    
    return {'step': steps, **cols}

def best_winner_by_universe(results_path):
    """Parse results CSV and return best threshold per universe."""
    winners = {}
    with open(results_path, 'r') as f:
        reader = csv.DictReader(f)
        for row in reader:
            uname = row['universe']
            thresh = int(row['threshold'])
            sharpe = float(row['sharpe'])
            ret = float(row['return_pct'])
            # Score = pass + Sharpe/10
            score = (1 if ret > 0 else 0) + sharpe / 10
            key = (uname, int(row['window']))
            if key not in winners or sharpe > winners[key][2]:
                winners[key] = (thresh, ret, sharpe)
    return winners

def main():
    base_dir = os.path.dirname(os.path.abspath(__file__))
    equity_path = os.path.join(base_dir, '..', 'snapshots', 'turtle_atr_regime_filter_equity.csv')
    results_path = os.path.join(base_dir, '..', 'snapshots', 'turtle_atr_regime_filter_results.csv')
    
    if not os.path.exists(equity_path):
        print(f"ERROR: {equity_path} not found")
        sys.exit(1)
    
    print(f"Reading {equity_path}")
    data = read_equity_csv(equity_path)
    steps = data['step']
    
    print(f"Total columns: {len(data)}")
    print(f"Steps: {len(steps)} bars")
    
    # Identify key columns: baseline (thresh=0), winner (thresh=20), high filter (thresh=50)
    baseline_cols = {k: v for k, v in data.items() if k.endswith('_0') and not k.startswith('step')}
    winner_cols = {k: v for k, v in data.items() if k.endswith('_20') and not k.startswith('step')}
    high_filter_cols = {k: v for k, v in data.items() if k.endswith('_50') and not k.startswith('step')}
    
    print(f"Baseline cols: {len(baseline_cols)}")
    print(f"Winner (th=20) cols: {len(winner_cols)}")
    print(f"High filter (th=50) cols: {len(high_filter_cols)}")
    
    # Get universe names from column keys
    universes = set()
    for k in data.keys():
        if k.startswith('step'):
            continue
        for suffix in ['_0', '_20', '_50']:
            if k.endswith(suffix):
                uname = k.replace(suffix, '').rsplit('_W', 1)[0]
                universes.add(uname)
    
    universes = sorted(list(universes))
    print(f"\nUniverses: {universes}")
    
    # Check for matplotlib
    try:
        import matplotlib
        matplotlib.use('Agg')
        import matplotlib.pyplot as plt
        import numpy as np
    except ImportError:
        print("matplotlib not available, checking pip...")
        import subprocess
        subprocess.run([sys.executable, '-m', 'pip', 'install', 'matplotlib', 'numpy'], check=True)
        import matplotlib
        matplotlib.use('Agg')
        import matplotlib.pyplot as plt
        import numpy as np
    
    # Select 4 key universes for display
    display_universes = ['Base5', 'NoDOGE', 'Legacy5BNB', 'LargeCaps5']
    display_universes = [u for u in display_universes if u in universes]
    
    n = len(display_universes)
    fig, axes = plt.subplots(n, 1, figsize=(14, 4 * n), sharex=False)
    if n == 1:
        axes = [axes]
    
    colors = {'baseline': '#2196F3', 'winner': '#FF5722', 'high_filter': '#4CAF50'}
    
    for ax, uname in zip(axes, display_universes):
        # Find matching columns for this universe
        base_key = next((k for k in baseline_cols if k.startswith(uname + '_W')), None)
        if base_key:
            base_col = baseline_cols[base_key]
        else:
            base_col = None
        
        win_key = next((k for k in winner_cols if k.startswith(uname + '_W')), None)
        if win_key:
            win_col = winner_cols[win_key]
        else:
            win_col = None
        
        hf_key = next((k for k in high_filter_cols if k.startswith(uname + '_W')), None)
        if hf_key:
            hf_col = high_filter_cols[hf_key]
        else:
            hf_col = None
        
        x = list(range(len(steps)))
        
        if base_col:
            ax.plot(x, base_col, color=colors['baseline'], label='Baseline (no filter)', alpha=0.8, linewidth=1.5)
        if win_col:
            ax.plot(x, win_col, color=colors['winner'], label='Threshold=20 (winner)', alpha=0.8, linewidth=1.5)
        if hf_col:
            ax.plot(x, hf_col, color=colors['high_filter'], label='Threshold=50 (high filter)', alpha=0.8, linewidth=1.5)
        
        # Use log scale for equity
        ax.set_yscale('log')
        
        # Set y-axis dynamically based on data range
        all_vals = []
        if base_col: all_vals.extend(base_col)
        if win_col: all_vals.extend(win_col)
        if hf_col: all_vals.extend(hf_col)
        all_vals = [v for v in all_vals if v > 0 and np.isfinite(v)]
        if all_vals:
            ymin = max(min(all_vals) * 0.8, 1e-6)
            ymax = max(all_vals) * 1.2
            ax.set_ylim(ymin, ymax)
        
        ax.set_title(f'{uname} — ATR Regime Filter Equity Curves (log scale)', fontsize=12, fontweight='bold')
        ax.set_xlabel('Bar (252-bar windows)')
        ax.set_ylabel('Equity (log)')
        ax.legend(loc='upper left', fontsize=9)
        ax.grid(True, alpha=0.3)
        
        # Add final equity annotations
        for col, label, color in [(base_col, 'Base', colors['baseline']),
                                   (win_col, 'Th=20', colors['winner']),
                                   (hf_col, 'Th=50', colors['high_filter'])]:
            if col:
                final = col[-1]
                if np.isfinite(final):
                    ax.annotate(f'{final:.1f}x', xy=(len(x)-1, final),
                               fontsize=8, color=color, ha='left')
    
    plt.tight_layout()
    out_path = os.path.join(base_dir, 'atr_regime_filter_comparison.png')
    plt.savefig(out_path, dpi=150, bbox_inches='tight')
    print(f"\nSaved: {out_path}")
    
    # Also create a summary comparison bar chart
    fig2, axes2 = plt.subplots(1, 2, figsize=(14, 5))
    
    # Read results CSV for aggregate stats
    thresholds = [0, 20, 30, 40, 50, 60, 70]
    pass_counts = [0] * len(thresholds)
    total_counts = [0] * len(thresholds)
    avg_sharpes = [0.0] * len(thresholds)
    avg_rets = [0.0] * len(thresholds)
    
    with open(results_path, 'r') as f:
        reader = csv.DictReader(f)
        rows_by_thresh = {t: [] for t in thresholds}
        for row in reader:
            t = int(row['threshold'])
            if t in rows_by_thresh:
                rows_by_thresh[t].append(row)
    
    for i, t in enumerate(thresholds):
        rows = rows_by_thresh[t]
        total_counts[i] = len(rows)
        pass_counts[i] = sum(1 for r in rows if float(r['return_pct']) > 0)
        if rows:
            avg_sharpes[i] = sum(float(r['sharpe']) for r in rows) / len(rows)
            avg_rets[i] = sum(float(r['return_pct']) for r in rows) / len(rows)
    
    # Pass rate bar chart
    pass_rates = [p / max(t, 1) * 100 for p, t in zip(pass_counts, total_counts)]
    bars = axes2[0].bar([str(t) for t in thresholds], pass_rates,
                         color=['#2196F3' if t == 0 else '#FF5722' if t == 20 else '#4CAF50' if t == 50 else '#9E9E9E' for t in thresholds])
    axes2[0].set_xlabel('ATR Percentile Threshold')
    axes2[0].set_ylabel('Pass Rate (%)')
    axes2[0].set_title('Pass Rate by Threshold (higher = better)', fontweight='bold')
    axes2[0].set_ylim(0, 100)
    axes2[0].axhline(y=50, color='gray', linestyle='--', alpha=0.5)
    for bar, rate in zip(bars, pass_rates):
        axes2[0].text(bar.get_x() + bar.get_width()/2, bar.get_height() + 1,
                     f'{rate:.1f}%', ha='center', fontsize=8)
    
    # Avg Sharpe bar chart
    bar_colors = ['#2196F3' if t == 0 else '#FF5722' if t == 20 else '#4CAF50' if t == 50 else '#9E9E9E' for t in thresholds]
    bars2 = axes2[1].bar([str(t) for t in thresholds], avg_sharpes, color=bar_colors)
    axes2[1].set_xlabel('ATR Percentile Threshold')
    axes2[1].set_ylabel('Avg Sharpe Ratio')
    axes2[1].set_title('Average Sharpe by Threshold', fontweight='bold')
    axes2[1].axhline(y=0, color='gray', linestyle='--', alpha=0.5)
    for bar, sh in zip(bars2, avg_sharpes):
        axes2[1].text(bar.get_x() + bar.get_width()/2, bar.get_height() + 0.05,
                     f'{sh:.2f}', ha='center', fontsize=8)
    
    plt.tight_layout()
    out_path2 = os.path.join(base_dir, 'atr_regime_filter_summary.png')
    plt.savefig(out_path2, dpi=150, bbox_inches='tight')
    print(f"Saved: {out_path2}")

if __name__ == '__main__':
    main()
