#!/usr/bin/env python3
"""
T88 HEDGE_ATR_PCT Hyperopt — Comparison Chart

Generates: /home/ubuntu/.openclaw/workspace-krypto/charts/comparison_chart.png

Data sources:
  snapshots/t88_hedge_atr_pct_summary.csv  — aggregated metrics per PCT value
  snapshots/t88_hedge_atr_pct_windows.csv   — per-window pass/sharpe/return
  snapshots/t88_hedge_atr_pct_equity.csv    — daily equity for baseline + winners
  snapshots/live_bot_exact_equity.csv       — production baseline (exact-live path)

This script reads the equity CSVs and generates:
  1. Equity curve comparison (log scale): baseline PCT=0.45 vs winners
  2. Metrics summary bar chart (pass rate, Sharpe, MaxDD)
"""

import os
import pandas as pd
import numpy as np
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import matplotlib.gridspec as gridspec

WORKSPACE = "/home/ubuntu/.openclaw/workspace-krypto/krypto"
CHARTS_DIR = "/home/ubuntu/.openclaw/workspace-krypto/charts"
OUT_PATH = os.path.join(CHARTS_DIR, "comparison_chart.png")

os.makedirs(CHARTS_DIR, exist_ok=True)

# ─────────────────────────────────────────────────────────────────────────────
# 1. Load sweep summary
# ─────────────────────────────────────────────────────────────────────────────
summary_path = os.path.join(WORKSPACE, "snapshots/t88_hedge_atr_pct_summary.csv")
windows_path = os.path.join(WORKSPACE, "snapshots/t88_hedge_atr_pct_windows.csv")
equity_path  = os.path.join(WORKSPACE, "snapshots/t88_hedge_atr_pct_equity.csv")
live_eq_path = os.path.join(WORKSPACE, "snapshots/live_bot_exact_equity.csv")

has_summary = os.path.exists(summary_path)
has_windows = os.path.exists(windows_path)
has_equity  = os.path.exists(equity_path)
has_live    = os.path.exists(live_eq_path)

print(f"Summary: {has_summary}, Windows: {has_windows}, Equity: {has_equity}, Live: {has_live}")

if has_summary:
    df_sum = pd.read_csv(summary_path)
    print("Summary columns:", df_sum.columns.tolist())
    print(df_sum.head(5))
else:
    df_sum = None

if has_equity:
    df_eq = pd.read_csv(equity_path)
    print("Equity columns:", df_eq.columns.tolist())
    print(df_eq.head(3))
else:
    df_eq = None

# ─────────────────────────────────────────────────────────────────────────────
# 2. Find winners and baseline
# ─────────────────────────────────────────────────────────────────────────────
# Production baseline: PCT=0.45 (current default)
baseline_pct = 45

if df_sum is not None and len(df_sum) > 0:
    # Normalize pct column
    if 'pct' in df_sum.columns:
        pass
    elif 'hedge_atr_pct' in df_sum.columns:
        df_sum = df_sum.rename(columns={'hedge_atr_pct': 'pct'})
    else:
        print("Unknown columns:", df_sum.columns.tolist())
        df_sum = None

if df_sum is not None:
    # Sort by pass rate desc, then sharpe desc
    df_sorted = df_sum.sort_values(['pass_rate','sharpe'], ascending=[False, False])
    top5 = df_sorted.head(5)
    print("\nTop 5 by pass rate then Sharpe:")
    print(top5[['pct','pass_rate','sharpe','avg_return','max_dd','total_trades']].to_string(index=False))
    
    winner_pct = int(top5.iloc[0]['pct'])
    runner1_pct = int(top5.iloc[1]['pct']) if len(top5) > 1 else None
    runner2_pct = int(top5.iloc[2]['pct']) if len(top5) > 2 else None
else:
    print("No summary data — using equity-based comparison")
    winner_pct = baseline_pct
    runner1_pct = None
    runner2_pct = None

# ─────────────────────────────────────────────────────────────────────────────
# 3. Build equity comparison
# ─────────────────────────────────────────────────────────────────────────────
if df_eq is not None:
    print("\nEquity CSV columns:", df_eq.columns.tolist())
    # Find the pct columns available
    all_cols = df_eq.columns.tolist()
    pct_cols = [c for c in all_cols if c.startswith('pct_') or c.startswith('hedge_')]
    print("PCT equity columns:", pct_cols[:10])
    
    # Build figure
    fig = plt.figure(figsize=(16, 12))
    gs = gridspec.GridSpec(3, 2, figure=fig, height_ratios=[3, 1, 1])

    ax_equity = fig.add_subplot(gs[0, :])   # full width top
    ax_bar    = fig.add_subplot(gs[1, 0])   # bottom left
    ax_metrics= fig.add_subplot(gs[1, 1])   # bottom right
    ax_pass   = fig.add_subplot(gs[2, :])   # full width bottom

    # ── Equity curve comparison ────────────────────────────────────────────────
    date_col = 'date' if 'date' in df_eq.columns else df_eq.columns[1]
    bar_col  = 'bar'  if 'bar'  in df_eq.columns else df_eq.columns[0]

    # Load live exact equity for reference
    if has_live:
        live_eq = pd.read_csv(live_eq_path)
        live_eq['date'] = pd.to_datetime(live_eq['date'])
        ax_equity.plot(live_eq['date'], live_eq['equity'],
                       color='orange', linewidth=1.5, alpha=0.8,
                       label=f'Exact-live (baseline, PCT=0.45)')

    # Plot T88 equity curves
    pct_col_map = {}
    for col in df_eq.columns:
        if col.startswith('pct_'):
            try:
                pct_val = float(col.replace('pct_','').replace('_','.'))
                pct_col_map[pct_val] = col
            except:
                pass

    if pct_col_map:
        colors = plt.cm.viridis(np.linspace(0.2, 0.9, len(pct_col_map)))
        for idx, (pct_val, col) in enumerate(sorted(pct_col_map.items())):
            label = None
            lw = 1.0
            ls = '-'
            alpha = 0.4
            color = colors[idx]
            if pct_val == baseline_pct:
                continue  # skip baseline, use exact-live
            if pct_val == winner_pct:
                label = f'Winner PCT={pct_val}'
                lw = 2.0
                ls = '-'
                alpha = 1.0
                color = 'green'
            elif pct_val in [runner1_pct, runner2_pct]:
                label = f'Runner-up PCT={pct_val}'
                lw = 1.5
                ls = '--'
                alpha = 0.9
                color = 'blue'
            if label:
                ax_equity.plot(pd.to_datetime(df_eq[date_col]), df_eq[col],
                               color=color, linewidth=lw, linestyle=ls, alpha=alpha,
                               label=label)

    ax_equity.set_yscale('log')
    ax_equity.set_ylabel('Equity (log scale)', fontsize=11)
    ax_equity.set_title('T88 HEDGE_ATR_PCT Hyperopt — Equity Comparison', fontsize=13, fontweight='bold')
    ax_equity.legend(loc='upper left', fontsize=9)
    ax_equity.grid(True, alpha=0.3)
    ax_equity.set_xlabel('')

    # ── Metrics bar chart ──────────────────────────────────────────────────────
    if df_sum is not None:
        df_plot = df_sum.sort_values('pct')
        x = range(len(df_plot))
        ax_bar.bar(x, df_plot['sharpe'], color='steelblue', alpha=0.7, label='Sharpe')
        ax_bar.axvline(x=list(df_plot['pct']).index(baseline_pct) if baseline_pct in df_plot['pct'].values else -1,
                       color='orange', linestyle='--', linewidth=2, label=f'Baseline PCT={baseline_pct}')
        if winner_pct != baseline_pct:
            ax_bar.axvline(x=list(df_plot['pct']).index(winner_pct) if winner_pct in df_plot['pct'].values else -1,
                           color='green', linestyle=':', linewidth=2, label=f'Winner PCT={winner_pct}')
        ax_bar.set_xlabel('PCT index', fontsize=9)
        ax_bar.set_ylabel('Sharpe', fontsize=9)
        ax_bar.set_title('Sharpe by HEDGE_ATR_PCT', fontsize=10)
        ax_bar.legend(fontsize=7)
        ax_bar.grid(True, alpha=0.3, axis='y')

        # Pass rate
        ax_pass.bar(x, df_plot['pass_rate'], color='darkgreen', alpha=0.7, label='Pass Rate')
        ax_pass.axhline(y=70, color='red', linestyle='--', alpha=0.5, label='70% threshold')
        ax_pass.axvline(x=list(df_plot['pct']).index(baseline_pct) if baseline_pct in df_plot['pct'].values else -1,
                        color='orange', linestyle='--', linewidth=2)
        if winner_pct != baseline_pct:
            ax_pass.axvline(x=list(df_plot['pct']).index(winner_pct) if winner_pct in df_plot['pct'].values else -1,
                            color='green', linestyle=':', linewidth=2)
        ax_pass.set_xlabel('HEDGE_ATR_PCT (%)', fontsize=9)
        ax_pass.set_ylabel('Pass Rate (%)', fontsize=9)
        ax_pass.set_title('Walk-Forward Pass Rate by HEDGE_ATR_PCT', fontsize=10)
        ax_pass.legend(fontsize=7)
        ax_pass.grid(True, alpha=0.3, axis='y')
        ax_pass.set_xticks(x[::3])
        ax_pass.set_xticklabels([f'{int(p):.0f}' for p in df_plot['pct'].values[::3]], fontsize=7)

    else:
        ax_bar.set_title('No summary data yet', fontsize=10)
        ax_pass.set_title('No window data yet', fontsize=10)

    # MaxDD scatter
    ax_metrics.set_title('MaxDD vs Sharpe (bubble=PCT)', fontsize=10)
    if df_sum is not None:
        scatter = ax_metrics.scatter(df_sum['sharpe'], df_sum['max_dd'],
                                     c=df_sum['pct'], cmap='viridis',
                                     s=60, alpha=0.7)
        ax_metrics.axvline(x=df_sum[df_sum['pct']==baseline_pct]['sharpe'].values[0] if baseline_pct in df_sum['pct'].values else 0,
                           color='orange', linestyle='--', linewidth=1.5)
        if winner_pct != baseline_pct and winner_pct in df_sum['pct'].values:
            ax_metrics.scatter(df_sum[df_sum['pct']==winner_pct]['sharpe'].values[0],
                               df_sum[df_sum['pct']==winner_pct]['max_dd'].values[0],
                               color='green', s=150, marker='*', zorder=5, label=f'Winner {winner_pct}%')
        plt.colorbar(scatter, ax=ax_metrics, label='PCT (%)', fontsize=7)
        ax_metrics.set_xlabel('Sharpe', fontsize=9)
        ax_metrics.set_ylabel('MaxDD (%)', fontsize=9)
        ax_metrics.legend(fontsize=7)
    ax_metrics.grid(True, alpha=0.3)

    plt.tight_layout()
    plt.savefig(OUT_PATH, dpi=150, bbox_inches='tight')
    print(f"\nChart saved to: {OUT_PATH}")
    print(f"  File size: {os.path.getsize(OUT_PATH)/1024:.1f} KB")

else:
    # No data yet — create placeholder
    fig, ax = plt.subplots(figsize=(12, 6))
    ax.text(0.5, 0.5, "T88 sweep in progress...\nEquity comparison will appear here when complete.",
            ha='center', va='center', fontsize=14, color='gray')
    ax.set_xlim(0, 1)
    ax.set_ylim(0, 1)
    ax.axis('off')
    plt.title('T88 HEDGE_ATR_PCT Hyperopt — Waiting for data', fontsize=14)
    plt.savefig(OUT_PATH, dpi=100, bbox_inches='tight')
    print(f"Placeholder chart saved: {OUT_PATH}")