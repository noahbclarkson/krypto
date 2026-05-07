#!/usr/bin/env python3
"""
Equity comparison chart generator for krypto hyperparameter optimization.
Reads equity time-series CSVs and generates comparison line charts.

Usage:
  python3 charts/hyperopt_chart.py [options]

CSV format expected (per parameter value):
  window,bar,equity_value
  (or: step,bar,equity if step-based sweep)

With --comparison mode for multi-run comparison:
  window,bar,param1_equity,param2_equity,param3_equity

Columns detected automatically. Key columns:
  - "bar" or "step": x-axis (time/step)
  - Any other numeric column: equity line for a parameter value
  - Column header used as label

Output: /home/ubuntu/.openclaw/workspace-krypto/charts/comparison_chart.png
"""

import sys
import os
import argparse
import warnings
warnings.filterwarnings('ignore')

import numpy as np
import pandas as pd
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import matplotlib.ticker as mticker

CHARTS_DIR = '/home/ubuntu/.openclaw/workspace-krypto/charts/'
OUTPUT_PATH = os.path.join(CHARTS_DIR, 'comparison_chart.png')

# Color palette — distinct, colorblind-friendly
COLORS = [
    '#1f77b4', '#ff7f0e', '#2ca02c', '#d62728', '#9467bd',
    '#8c564b', '#e377c2', '#7f7f7f', '#bcbd22', '#17becf',
    '#ff6b6b', '#4ecdc4', '#a8e6cf', '#ffd93d', '#6bcb77',
]


def load_csv(path):
    """Load CSV, auto-detect separator and columns."""
    for sep in [',', '\t', ';']:
        try:
            df = pd.read_csv(path, sep=sep)
            if len(df.columns) >= 2:
                return df
        except Exception:
            continue
    raise ValueError(f"Could not parse CSV: {path}")


def detect_columns(df):
    """Detect x-axis and equity columns."""
    # Find bar/step column
    bar_col = None
    for candidate in ['bar', 'step', 'index', 'date', 'day', 'time']:
        if candidate in df.columns.str.lower():
            bar_col = df.columns[df.columns.str.lower() == candidate][0]
            break
    if bar_col is None:
        # Use first column as x-axis
        bar_col = df.columns[0]

    # All other numeric columns are equity series
    equity_cols = [c for c in df.columns if c != bar_col and pd.api.types.is_numeric_dtype(df[c])]
    return bar_col, equity_cols


def aggregate_windows(df, bar_col, equity_col):
    """
    Aggregate equity across windows: at each time step, take the geometric mean
    of equity across all windows that have data at that step.
    Returns (mean_bars, mean_equity) arrays.
    """
    grouped = df.groupby(bar_col)[equity_col].agg(
        lambda s: s.prod() ** (1.0 / max(s.count(), 1))
    )
    mean_bars = grouped.index.values
    mean_equity = grouped.values
    return mean_bars, mean_equity


def plot_comparison(ax, df, bar_col, equity_cols, labels=None, title="Hyperparameter Comparison"):
    """Plot equity curves with dynamic Y-axis scaling."""

    if labels is None:
        labels = list(equity_cols)

    # Use a soft log-scale: plot log(equity) but label as equity values
    # Determine x range
    x_vals = df[bar_col].values
    x_min, x_max = x_vals.min(), x_vals.max()

    # For each equity column, compute per-window geometric mean at each bar
    lines = {}
    for i, ecol in enumerate(equity_cols):
        label = labels[i] if i < len(labels) else str(ecol)
        color = COLORS[i % len(COLORS)]

        eq_vals = df[[bar_col, ecol]].dropna()
        if eq_vals.empty:
            continue

        # Aggregate across windows: geometric mean at each bar
        x_agg, y_agg = aggregate_windows(eq_vals, bar_col, ecol)

        # Remove degenerate values (equity <= 0)
        mask = y_agg > 1e-10
        x_agg = x_agg[mask]
        y_agg = y_agg[mask]

        if len(x_agg) < 2:
            continue

        ax.plot(x_agg, y_agg, color=color, label=label,
                linewidth=1.5, alpha=0.9)

        lines[label] = (x_agg, y_agg)

    ax.set_xlabel(bar_col.replace('_', ' ').title(), fontsize=10)
    ax.set_ylabel('Equity (× return)', fontsize=10)
    ax.set_title(title, fontsize=11, fontweight='bold')
    ax.legend(fontsize=8, loc='upper left', framealpha=0.85)
    ax.grid(True, alpha=0.3, linestyle='--')

    # Dynamic Y-axis: use symlog for mixed positive/negative, or log for positive-only
    all_vals = []
    for x, y in lines.values():
        all_vals.extend(y.tolist())

    if all_vals:
        y_min = min(all_vals)
        y_max = max(all_vals)
        y_range = y_max - y_min

        if y_min > 0 and y_range > 0:
            # All positive — use log scale for better visibility
            ax.set_yscale('log')
            # Set nice log ticks
            ax.yaxis.set_major_formatter(matplotlib.ticker.FormatStrFormatter('%.2f'))
        else:
            # Mixed values — linear with auto limits
            margin = y_range * 0.05
            ax.set_ylim(max(y_min - margin, 1e-6), y_max + margin)

    return lines


def plot_multi_run_summary(ax, results_df, param_col='param', metric_col='sharpe'):
    """Bar chart of metric by parameter value."""
    if param_col not in results_df.columns or metric_col not in results_df.columns:
        return

    params = results_df[param_col].values
    metrics = results_df[metric_col].values

    # Sort by param value
    order = np.argsort(params)
    params = params[order]
    metrics = metrics[order]

    colors = [COLORS[i % len(COLORS)] for i in range(len(params))]
    bars = ax.bar(range(len(params)), metrics, color=colors, alpha=0.8, edgecolor='black', linewidth=0.5)

    ax.set_xticks(range(len(params)))
    ax.set_xticklabels([f'{p:.1f}' if isinstance(p, float) else str(p) for p in params],
                       rotation=45, ha='right', fontsize=7)
    ax.set_xlabel(param_col.replace('_', ' ').title(), fontsize=10)
    ax.set_ylabel(metric_col.replace('_', ' ').title(), fontsize=10)
    ax.set_title(f'{metric_col.title()} by {param_col}', fontsize=11, fontweight='bold')
    ax.grid(True, alpha=0.3, axis='y', linestyle='--')


def main():
    parser = argparse.ArgumentParser(description='Generate hyperparameter equity comparison charts')
    parser.add_argument('--csv', '-c', default=None,
                        help='Path to equity time-series CSV')
    parser.add_argument('--summary-csv', '-s', default=None,
                        help='Path to summary CSV (with per-param metrics)')
    parser.add_argument('--output', '-o', default=OUTPUT_PATH,
                        help='Output PNG path')
    parser.add_argument('--title', '-t', default='Hyperparameter Equity Comparison',
                        help='Chart title')
    parser.add_argument('--labels', '-l', default=None,
                        help='Comma-separated labels for equity columns')
    parser.add_argument('--width', '-W', type=float, default=12.0,
                        help='Figure width in inches')
    parser.add_argument('--height', '-H', type=float, default=5.0,
                        help='Figure height in inches')
    parser.add_argument('--two-panel', action='store_true',
                        help='Show equity curve AND bar chart of per-param Sharpe')
    args = parser.parse_args()

    os.makedirs(CHARTS_DIR, exist_ok=True)

    if args.csv is None and args.summary_csv is None:
        print("Error: must provide --csv or --summary-csv")
        sys.exit(1)

    n_panels = 0
    if args.csv:
        n_panels += 1
    if args.summary_csv:
        n_panels += 1

    if args.two_panel:
        n_panels += 1

    fig_h = args.height
    if n_panels == 2:
        fig_h = args.height * 1.4
    elif n_panels >= 3:
        fig_h = args.height * 1.8

    fig, axes = plt.subplots(n_panels, 1, figsize=(args.width, fig_h),
                             sharex=False)
    if n_panels == 1:
        axes = [axes]

    panel = 0
    labels = args.labels.split(',') if args.labels else None

    if args.csv:
        print(f"Loading equity CSV: {args.csv}")
        df = load_csv(args.csv)
        bar_col, equity_cols = detect_columns(df)
        print(f"  Bar col: {bar_col}, Equity cols: {equity_cols}")

        plot_comparison(axes[panel], df, bar_col, equity_cols,
                        labels=labels, title=args.title)
        panel += 1

    if args.summary_csv:
        print(f"Loading summary CSV: {args.summary_csv}")
        sdf = load_csv(args.summary_csv)
        print(f"  Columns: {list(sdf.columns)}")

        if panel < len(axes):
            plot_multi_run_summary(axes[panel], sdf,
                                  param_col=sdf.columns[0],
                                  metric_col=sdf.columns[3] if len(sdf.columns) > 3 else sdf.columns[1])
            panel += 1

    plt.tight_layout(pad=1.5)
    plt.savefig(args.output, dpi=150, bbox_inches='tight',
                facecolor='white', edgecolor='none')
    print(f"\nSaved: {args.output}")
    plt.close()


if __name__ == '__main__':
    main()
