#!/usr/bin/env python3
"""
Plot VOL_LOOKBACK sweep comparison chart.
Reads: snapshots/vol_lookback_sweep_equity.csv (step,equity per vl/universe/window)
Reads: snapshots/vol_lookback_sweep_summary.csv (global aggregate per VL)
Output: charts/vol_lookback_comparison.png

Shows:
- Baseline (VL=2, current default) in blue
- Winner (VL=1) in green
- Top 3 runner-ups in distinct colors
- Equity curves aggregated across all universes/windows (step vs median equity)
- Log-scale Y-axis so curves are clearly visible
"""

import pandas as pd
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import matplotlib.ticker as mticker
import sys
import os

CHART_DIR = "charts"
os.makedirs(CHART_DIR, exist_ok=True)

EQUITY_CSV = "snapshots/vol_lookback_sweep_equity.csv"
SUMMARY_CSV = "snapshots/vol_lookback_sweep_summary.csv"
OUT_PNG = f"{CHART_DIR}/vol_lookback_comparison.png"

# Color palette: baseline=blue, winner=green, runner-ups=orange/red/purple
COLOR_BASELINE = "#4C78A8"   # blue (VL=2)
COLOR_WINNER   = "#54A24B"   # green (VL=1)
COLOR_RU1      = "#F58518"   # orange (VL=75)
COLOR_RU2      = "#E45756"   # red (VL=3)
COLOR_RU3      = "#B279A2"   # purple (VL=100)

def load_summary():
    df = pd.read_csv(SUMMARY_CSV)
    df = df.sort_values('avg_sharpe', ascending=False).reset_index(drop=True)
    return df

def load_equity():
    df = pd.read_csv(EQUITY_CSV)
    return df

def compute_median_equity(equity_df, vl_values):
    """
    For each VL, aggregate equity across all universes/windows.
    At each step, take the MEDIAN equity across all runs.
    This gives a representative "typical" equity curve.
    """
    result = {}
    for vl in vl_values:
        subset = equity_df[equity_df['vl'] == vl].copy()
        if subset.empty:
            continue
        # Group by step, take median across all universe/window combinations
        median_by_step = subset.groupby('step')['equity'].median().reset_index()
        median_by_step = median_by_step.sort_values('step')
        result[vl] = median_by_step
    return result

def main():
    print(f"Reading {EQUITY_CSV} ...")
    eq_df = load_equity()
    print(f"  Equity rows: {len(eq_df)}")
    print(f"  Unique VLs: {sorted(eq_df['vl'].unique())}")

    print(f"Reading {SUMMARY_CSV} ...")
    summary_df = load_summary()
    print(f"  Summary rows: {len(summary_df)}")
    print("\n  Global summary (sorted by Sharpe):")
    for _, row in summary_df.iterrows():
        print(f"    VL={row['vl']:3.0f} | Pass {row['global_pass']:.0f}/{row['global_total']:.0f} "
              f"({row['pass_rate']:.1f}%) | Sharpe {row['avg_sharpe']:.3f} | "
              f"Ret {row['avg_ret']:.1f}% | DD {row['avg_dd']:.1f}%")

    # Select what to plot
    baseline_vl = 2
    winner_vl = int(summary_df.iloc[0]['vl'])
    runner_ups = []
    for _, row in summary_df.iloc[1:].iterrows():
        vl = int(row['vl'])
        if vl != baseline_vl and vl != winner_vl and len(runner_ups) < 3:
            runner_ups.append(vl)

    print(f"\n  Winner: VL={winner_vl}")
    print(f"  Baseline: VL={baseline_vl}")
    print(f"  Runner-ups: {runner_ups}")

    # Compute median equity curves
    all_vls = [winner_vl, baseline_vl] + runner_ups
    median_curves = compute_median_equity(eq_df, all_vls)

    # Also compute per-universe median for Base5
    base5_df = eq_df[eq_df['universe'] == 'Base5']

    # ── Figure setup ──────────────────────────────────────────────────────────
    fig, axes = plt.subplots(2, 1, figsize=(14, 10), gridspec_kw={'height_ratios': [3, 1.2]})
    fig.patch.set_facecolor('#1e1e1e')
    for ax in axes:
        ax.set_facecolor('#2d2d2d')
        ax.tick_params(colors='#cccccc', which='both')
        ax.xaxis.label.set_color('#cccccc')
        ax.yaxis.label.set_color('#cccccc')
        ax.title.set_color('#ffffff')
        for spine in ax.spines.values():
            spine.set_color('#444444')

    # ── Top panel: Equity curves (log scale) ─────────────────────────────────
    ax = axes[0]

    label_map = {
        baseline_vl: f"Baseline VL={baseline_vl}",
        winner_vl: f"Winner VL={winner_vl}",
        runner_ups[0]: f"Runner-up VL={runner_ups[0]}",
        runner_ups[1]: f"Runner-up VL={runner_ups[1]}",
        runner_ups[2]: f"Runner-up VL={runner_ups[2]}",
    }
    color_map = {
        baseline_vl: COLOR_BASELINE,
        winner_vl: COLOR_WINNER,
        runner_ups[0]: COLOR_RU1,
        runner_ups[1]: COLOR_RU2,
        runner_ups[2]: COLOR_RU3,
    }
    lw_map = {
        baseline_vl: 2.0,
        winner_vl: 2.5,
    }

    for vl, curve_df in median_curves.items():
        steps = curve_df['step'].values
        equities = curve_df['equity'].values

        label = label_map.get(vl, f"VL={vl}")
        color = color_map.get(vl, '#888888')
        lw = lw_map.get(vl, 1.5)
        is_winner = (vl == winner_vl)
        is_baseline = (vl == baseline_vl)

        ax.plot(steps, equities,
                label=label,
                color=color,
                linewidth=lw,
                alpha=0.9 if (is_winner or is_baseline) else 0.75)

    ax.set_yscale('log')
    ax.yaxis.set_major_formatter(mticker.FormatStrFormatter('%.1fx'))
    ax.set_ylabel("Portfolio Equity (× initial)", fontsize=11)
    ax.set_title(
        f"Turtle+Chandelier — VOL_LOOKBACK Sweep: Winner vs Baseline\n"
        f"Production params: CHAND(11,2.25) EP=24 ATR_ENTRY_MULT=0.90 HM=12 · 9 universes × 54 windows · 14 VL values",
        fontsize=12, fontweight='bold', color='#ffffff', pad=10
    )
    ax.legend(loc="upper left", fontsize=10, framealpha=0.85,
              facecolor='#2d2d2d', edgecolor='#444444', labelcolor='#cccccc')
    ax.grid(True, alpha=0.15, color='#888888', linestyle='--')
    ax.set_xlim(left=0)

    # ── Bottom panel: Bar chart of global Sharpe ───────────────────────────────
    ax2 = axes[1]

    all_vls_sorted = [baseline_vl, winner_vl] + [r for r in runner_ups]
    summary_dict = {int(row['vl']): row for _, row in summary_df.iterrows()}

    bar_colors = [color_map.get(vl, '#888888') for vl in all_vls_sorted]
    bar_labels = [f"VL={vl}" for vl in all_vls_sorted]
    bar_sharpes = [summary_dict[vl]['avg_sharpe'] for vl in all_vls_sorted]
    bar_passes = [summary_dict[vl]['pass_rate'] for vl in all_vls_sorted]
    bar_rets = [summary_dict[vl]['avg_ret'] for vl in all_vls_sorted]

    x = range(len(all_vls_sorted))
    bars = ax2.bar(x, bar_sharpes, color=bar_colors, alpha=0.85, width=0.6)

    for i, (bar, sh, pr, ret) in enumerate(zip(bars, bar_sharpes, bar_passes, bar_rets)):
        ax2.text(bar.get_x() + bar.get_width() / 2, bar.get_height() + 0.03,
                 f"Sharpe {sh:.2f}\n{ret:.0f}% ret\n{pr:.0f}% pass",
                 ha='center', va='bottom', fontsize=8.5, color='#cccccc')

    ax2.set_xticks(list(x))
    ax2.set_xticklabels(bar_labels, fontsize=10, color='#cccccc')
    ax2.set_ylabel("Avg Sharpe", fontsize=10, color='#cccccc')
    ax2.set_title("Per-VL Global Metrics (Sharpe / Return / Pass Rate)", fontsize=10, color='#aaaaaa')
    ax2.grid(True, alpha=0.15, axis='y', color='#888888', linestyle='--')
    ax2.set_xlim(-0.5, len(all_vls_sorted) - 0.5)
    ax2.yaxis.set_major_formatter(mticker.FormatStrFormatter('%.2f'))

    plt.tight_layout(pad=2.0)
    plt.savefig(OUT_PNG, dpi=150, bbox_inches='tight', facecolor=fig.get_facecolor())
    print(f"\nSaved: {OUT_PNG}")

    # ── Also plot Base5 universe only ─────────────────────────────────────────
    fig2, ax3 = plt.subplots(figsize=(14, 6))
    fig2.patch.set_facecolor('#1e1e1e')
    ax3.set_facecolor('#2d2d2d')
    ax3.tick_params(colors='#cccccc', which='both')
    for spine in ax3.spines.values():
        spine.set_color('#444444')

    base5_curves = compute_median_equity(base5_df, all_vls_sorted)
    for vl, curve_df in base5_curves.items():
        steps = curve_df['step'].values
        equities = curve_df['equity'].values
        label = label_map.get(vl, f"VL={vl}")
        color = color_map.get(vl, '#888888')
        lw = lw_map.get(vl, 1.5)
        ax3.plot(steps, equities, label=label, color=color, linewidth=lw, alpha=0.9)

    ax3.set_yscale('log')
    ax3.yaxis.set_major_formatter(mticker.FormatStrFormatter('%.1fx'))
    ax3.set_ylabel("Portfolio Equity (× initial)", fontsize=11)
    ax3.set_title(
        f"Turtle+Chandelier — VOL_LOOKBACK Sweep: Base5 Universe Only\n"
        f"(BTC, ETH, SOL, XRP, DOGE, ADA × 6 windows)",
        fontsize=12, fontweight='bold', color='#ffffff', pad=10
    )
    ax3.legend(loc="upper left", fontsize=10, framealpha=0.85,
               facecolor='#2d2d2d', edgecolor='#444444', labelcolor='#cccccc')
    ax3.grid(True, alpha=0.15, color='#888888', linestyle='--')
    ax3.set_xlim(left=0)

    base5_out = f"{CHART_DIR}/vol_lookback_comparison_base5.png"
    plt.tight_layout()
    plt.savefig(base5_out, dpi=150, bbox_inches='tight', facecolor=fig2.get_facecolor())
    print(f"Saved: {base5_out}")

if __name__ == "__main__":
    main()
