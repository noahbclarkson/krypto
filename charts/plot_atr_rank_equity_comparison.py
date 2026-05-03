#!/usr/bin/env python3
"""
Plot ATR_RANK threshold comparison chart.
Reads equity time-series CSVs exported by equity_timeseries_export.rs.
Plots: Baseline (T=0), Old Default (T=5), Winner (T=24), Runner-up (T=30).
Aggregated across Base5 windows as a line chart.
Y-axis: dynamic (NOT forced to start at 0).
"""

import csv
import glob
import statistics
import sys

import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import matplotlib.ticker as mticker

def load_equity(universe: str, t: float) -> list[tuple]:
    """Load equity time-series for given universe/threshold.
    Returns list of (window, bar_idx, equity) tuples."""
    path = f"snapshots/equity_ts_{universe}_{t:.0f}.csv"
    rows = []
    with open(path) as f:
        reader = csv.DictReader(f)
        for row in reader:
            rows.append((int(row['window']), int(row['bar_idx']), float(row['equity'])))
    return rows

def compute_aggregated_equity(rows: list[tuple]) -> list[float]:
    """Aggregate equity across windows using compound growth.
    For each bar_idx: compound the equity values across all windows that have that bar."""
    from collections import defaultdict
    by_bar = defaultdict(list)
    for w, bi, eq in rows:
        by_bar[bi].append(eq)

    agg = []
    for bi in sorted(by_bar.keys()):
        vals = by_bar[bi]
        # Compound all window equities for this bar
        compound = 1.0
        for v in vals:
            compound *= v ** (1.0 / len(vals))  # geometric mean normalization
        agg.append(compound)
    return agg

def compute_equity_by_window(rows: list[tuple]) -> dict[int, list[float]]:
    """Separate equity curves by window."""
    from collections import defaultdict
    by_w = defaultdict(list)
    for w, bi, eq in rows:
        by_w[w].append(eq)
    return dict(sorted(by_w.items()))

def annualised_sharpe(equity_curve: list[float]) -> float:
    if len(equity_curve) < 10:
        return float('nan')
    rets = [equity_curve[i] / equity_curve[i-1] - 1 for i in range(1, len(equity_curve))]
    # Filter zero returns (flat periods)
    rets = [r for r in rets if abs(r) > 1e-10]
    if not rets:
        return float('nan')
    mean = sum(rets) / len(rets)
    if len(rets) == 1:
        return 0.0
    # Population variance
    var = sum((r - mean) ** 2 for r in rets) / len(rets)
    if var == 0:
        return 0.0
    return (mean / (var ** 0.5)) * (365 ** 0.5)

def max_drawdown(equity_curve: list[float]) -> float:
    peak = 1.0
    max_dd = 0.0
    for e in equity_curve:
        if e > peak:
            peak = e
        dd = 1.0 - e / peak
        if dd > max_dd:
            max_dd = dd
    return max_dd * 100.0

def final_return(equity_curve: list[float]) -> float:
    return (equity_curve[-1] / equity_curve[0] - 1) * 100.0

def compute_summary(equity_curve: list[float]) -> dict:
    return {
        'final_return': final_return(equity_curve),
        'max_dd': max_drawdown(equity_curve),
        'sharpe': annualised_sharpe(equity_curve),
        'bars': len(equity_curve),
    }

def main():
    universe = "Base5"
    thresholds = [0.0, 5.0, 24.0, 30.0]
    labels = {
        0.0: "T=0 (Baseline, no filter)",
        5.0: "T=5 (Old default)",
        24.0: "T=24 (Winner)",
        30.0: "T=30 (Runner-up)",
    }
    colors = {
        0.0: '#888888',  # gray
        5.0: '#2196F3',  # blue
        24.0: '#4CAF50', # green (winner)
        30.0: '#FF9800', # orange
    }

    all_data = {}
    for t in thresholds:
        rows = load_equity(universe, t)
        agg = compute_aggregated_equity(rows)
        by_w = compute_equity_by_window(rows)
        all_data[t] = {
            'aggregated': agg,
            'by_window': by_w,
            'rows': rows,
        }

    # ===== CHART 1: Aggregated equity curve (line chart) =====
    fig, ax = plt.subplots(figsize=(12, 7))

    for t in thresholds:
        agg = all_data[t]['aggregated']
        ax.plot(agg, label=labels[t], color=colors[t], linewidth=1.5, alpha=0.85)

    ax.set_xlabel("Bar (daily)", fontsize=11)
    ax.set_ylabel("Portfolio Equity (normalized)", fontsize=11)
    ax.set_title(f"Turtle+Chandelier — ATR_RANK Threshold Comparison\n"
                 f"Base5 Universe, 7 Walk-Forward Windows, Live-Compatible Harness",
                 fontsize=12)
    ax.legend(fontsize=10, framealpha=0.9)
    ax.grid(True, alpha=0.3, linestyle='--')
    ax.yaxis.set_major_formatter(mticker.FuncFormatter(lambda x, _: f"{x:.2f}x"))

    # Log scale for y-axis to show multiplicative growth
    ax.set_yscale('log')
    ax.set_ylim(bottom=0.1)

    plt.tight_layout()
    plt.savefig("charts/atr_rank_equity_comparison.png", dpi=150, bbox_inches='tight')
    plt.close()
    print("Saved: charts/atr_rank_equity_comparison.png")

    # ===== CHART 2: Per-window equity curves (subplot grid) =====
    n_windows = max(len(all_data[t]['by_window']) for t in thresholds)
    fig, axes = plt.subplots(n_windows, 1, figsize=(12, 3 * n_windows), sharex=False)

    if n_windows == 1:
        axes = [axes]

    window_metrics = {}
    for wi in range(n_windows):
        ax = axes[wi]
        for t in thresholds:
            w_data = all_data[t]['by_window'].get(wi, [])
            if w_data:
                ax.plot(w_data, label=labels[t], color=colors[t],
                        linewidth=1.2, alpha=0.85)

        ax.set_ylabel("Equity", fontsize=9)
        ax.set_title(f"Window {wi}", fontsize=9)
        ax.grid(True, alpha=0.25, linestyle='--')
        ax.set_yscale('log')
        if wi == 0:
            ax.legend(fontsize=7, ncol=4, loc='upper right')

    axes[-1].set_xlabel("Bar (daily)", fontsize=9)
    plt.suptitle("Per-Window Equity Curves — Base5 — ATR_RANK Thresholds",
                 fontsize=11, y=1.0)
    plt.tight_layout()
    plt.savefig("charts/atr_rank_equity_per_window.png", dpi=150, bbox_inches='tight')
    plt.close()
    print("Saved: charts/atr_rank_equity_per_window.png")

    # ===== SUMMARY TABLE =====
    print("\n=== Base5 Equity Summary ===")
    print(f"{'Label':<35} {'FinalRet':>10} {'MaxDD':>8} {'Sharpe':>8} {'Bars':>6}")
    print("-" * 70)
    for t in thresholds:
        agg = all_data[t]['aggregated']
        s = compute_summary(agg)
        print(f"{labels[t]:<35} {s['final_return']:>9.1f}% {s['max_dd']:>7.1f}% "
              f"{s['sharpe']:>8.2f} {s['bars']:>6}")

    # Per-window summary
    print(f"\n{'Window':<8}", end="")
    for t in thresholds:
        print(f"{'T='+str(int(t)):>12}", end="")
    print()
    print("-" * (8 + 12 * len(thresholds)))
    for wi in range(n_windows):
        print(f"W{wi:<7}", end="")
        for t in thresholds:
            w_data = all_data[t]['by_window'].get(wi, [])
            if w_data:
                s = compute_summary(w_data)
                print(f"{s['final_return']:>11.1f}%", end="")
            else:
                print(f"{'N/A':>12}", end="")
        print()

    print("\nCharts saved:")
    print("  charts/atr_rank_equity_comparison.png  — aggregated equity line chart")
    print("  charts/atr_rank_equity_per_window.png  — per-window equity subplot grid")

if __name__ == "__main__":
    main()