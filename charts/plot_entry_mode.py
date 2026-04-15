#!/usr/bin/env python3
"""
Turtle Entry Mode Sweep — Equity Curve Comparison Chart
Compares: max_close (MODE=0) vs max_high (MODE=1) entry signal

Generates: charts/turtle_entry_mode_comparison.png
"""
import csv
import sys
import math
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import matplotlib.ticker as mticker
import numpy as np

INFILE = "snapshots/turtle_entry_mode_multi_window.csv"
OUTFILE = "charts/turtle_entry_mode_comparison.png"
SUMMARY_FILE = "snapshots/turtle_entry_mode_wf.csv"

def load_summary():
    """Load per-window results to compute aggregate stats."""
    by_mode = {0: [], 1: []}
    try:
        with open(SUMMARY_FILE) as f:
            reader = csv.DictReader(f)
            for row in reader:
                mode = 0 if row['entry_mode'] == 'max_close' else 1
                by_mode[mode].append({
                    'universe': row['universe'],
                    'window': int(row['window']),
                    'ret': float(row['return_pct']),
                    'sharpe': float(row['sharpe']),
                    'dd': float(row['max_dd_pct']),
                    'trades': int(row['trades']),
                    'wr': float(row['win_rate_pct']),
                    'pass': row['pass'] == 'true',
                })
    except Exception as e:
        print(f"WARNING: Could not load summary: {e}")
        return {}
    return by_mode

def aggregate_stats(by_mode):
    """Aggregate stats per mode."""
    stats = {}
    for mode, rows in by_mode.items():
        if not rows:
            continue
        n = len(rows)
        passes = sum(1 for r in rows if r['pass'])
        avg_sharpe = sum(r['sharpe'] for r in rows) / n
        avg_ret = sum(r['ret'] for r in rows) / n
        avg_dd = sum(r['dd'] for r in rows) / n
        total_trades = sum(r['trades'] for r in rows)
        label = "close > max(close)" if mode == 0 else "close > max(high)"
        stats[mode] = {
            'label': label,
            'pass_rate': passes / n * 100,
            'avg_sharpe': avg_sharpe,
            'avg_ret': avg_ret,
            'avg_dd': avg_dd,
            'total_trades': total_trades,
            'n': n,
            'passes': passes,
        }
    return stats

def compound_equity(rows, mode):
    """
    Build a compound equity curve by compounding returns across all windows.
    rows: list of equity strings for a single mode, window, step
    Returns: list of (step, compounded_equity)
    """
    if not rows:
        return []
    # Group by step
    steps_data = {}
    for row in rows:
        parts = row.split(',')
        if len(parts) >= 5:
            window_label = parts[0]
            step = int(parts[1])
            eq = float(parts[4])
            if step not in steps_data:
                steps_data[step] = []
            steps_data[step].append(eq)
    if not steps_data:
        return []
    # Compound across windows: multiply per-step values across all windows
    max_step = max(steps_data.keys())
    compounded = []
    running = 1.0
    for step in range(max_step + 1):
        if step in steps_data:
            # Geometric mean of window equities at this step
            vals = steps_data[step]
            geo = math.exp(sum(math.log(max(v, 0.001)) for v in vals) / len(vals))
            running *= geo
        compounded.append((step, running))
    return compounded

def load_equity_by_universe_mode():
    """Load multi-window equity and compound per universe."""
    data = {}  # (universe, mode) -> list of rows
    try:
        with open(INFILE) as f:
            reader = csv.reader(f)
            header = next(reader)
            for row in reader:
                if len(row) < 5:
                    continue
                window_label = row[0]  # "Base5_0", "NoDOGE_1", etc.
                # Parse universe and window number
                parts = window_label.rsplit('_', 1)
                if len(parts) != 2:
                    continue
                universe = parts[0]
                try:
                    window_num = int(parts[1])
                except:
                    continue
                step = int(row[1])
                mode = 0 if row[2] == 'max_close' else 1
                eq = float(row[3])
                key = (universe, mode)
                if key not in data:
                    data[key] = {}
                if step not in data[key]:
                    data[key][step] = []
                data[key][step].append(eq)
    except Exception as e:
        print(f"WARNING: Could not load equity: {e}")
        return {}
    return data

def compound_equity_for_key(steps_dict):
    """Compound equity across windows for a given (universe, mode)."""
    if not steps_dict:
        return []
    max_step = max(steps_dict.keys())
    running = 1.0
    result = []
    for step in range(max_step + 1):
        if step in steps_dict and steps_dict[step]:
            vals = steps_dict[step]
            geo = math.exp(sum(math.log(max(v, 0.0001)) for v in vals) / len(vals))
            running *= geo
        result.append((step, running))
    return result

def equity_to_logscale(equity_series):
    """Convert equity series to log scale, handling zeros/negatives."""
    result = []
    for step, eq in equity_series:
        if eq <= 0:
            eq = 0.0001
        log_eq = math.log10(max(eq, 0.0001))
        result.append((step, log_eq))
    return result

def main():
    print("Loading summary stats...")
    by_mode = load_summary()
    stats = aggregate_stats(by_mode)
    if not stats:
        print("ERROR: No summary data found. Run turtle_entry_mode_walkforward first.")
        sys.exit(1)

    print("Loading equity curves...")
    equity_data = load_equity_by_universe_mode()

    # ── Figure setup ────────────────────────────────────────────────────────────
    fig, axes = plt.subplots(2, 2, figsize=(16, 12))
    fig.suptitle("Turtle+Chandelier — Entry Signal Mode Comparison\nclose > max(close) [MODE=0] vs close > max(high) [MODE=1]",
                 fontsize=14, fontweight='bold', y=0.98)

    colors = {0: '#2196F3', 1: '#F44336'}  # blue, red

    # ── Top-left: Per-universe pass rates ───────────────────────────────────────
    ax1 = axes[0, 0]
    universes = []
    pass_rates = [[], []]  # [mode0, mode1]
    for (universe, _), steps_dict in sorted(equity_data.items()):
        if universe not in universes:
            universes.append(universe)
        mode = 1 if 'max_high' in str(steps_dict) else 0  # can't tell from steps_dict, use data key

    # Re-read properly
    universe_list = []
    pr0, pr1 = [], []
    for key in sorted(equity_data.keys()):
        universe, mode = key
        if mode == 0 and universe not in universe_list:
            universe_list.append(universe)
    for universe in universe_list:
        m0_key = (universe, 0)
        m1_key = (universe, 1)
        m0_pass = sum(1 for r in by_mode.get(0, []) if r['universe'] == universe and r['pass'])
        m0_total = sum(1 for r in by_mode.get(0, []) if r['universe'] == universe)
        m1_pass = sum(1 for r in by_mode.get(1, []) if r['universe'] == universe and r['pass'])
        m1_total = sum(1 for r in by_mode.get(1, []) if r['universe'] == universe)
        pr0.append(m0_pass / m0_total * 100 if m0_total > 0 else 0)
        pr1.append(m1_pass / m1_total * 100 if m1_total > 0 else 0)

    x = np.arange(len(universe_list))
    width = 0.35
    bars1 = ax1.bar(x - width/2, pr0, width, label='close > max(close) [MODE=0]', color=colors[0], alpha=0.85)
    bars2 = ax1.bar(x + width/2, pr1, width, label='close > max(high) [MODE=1]', color=colors[1], alpha=0.85)
    ax1.set_ylabel('Pass Rate (%)')
    ax1.set_xlabel('Universe')
    ax1.set_title('Pass Rate by Universe (9 Universes × 6 Windows)')
    ax1.set_xticks(x)
    ax1.set_xticklabels(universe_list, rotation=30, ha='right', fontsize=8)
    ax1.legend(fontsize=8)
    ax1.axhline(70, color='gray', linestyle='--', linewidth=0.8, label='70% threshold')
    ax1.set_ylim(0, 110)
    ax1.grid(axis='y', alpha=0.3)

    # ── Top-right: Base5 equity curve (log scale) ──────────────────────────────
    ax2 = axes[0, 1]
    base5_key_0 = ('Base5', 0)
    base5_key_1 = ('Base5', 1)
    if base5_key_0 in equity_data and base5_key_1 in equity_data:
        eq0 = compound_equity_for_key(equity_data[base5_key_0])
        eq1 = compound_equity_for_key(equity_data[base5_key_1])
        if eq0 and eq1:
            steps0 = [s for s, _ in eq0]
            steps1 = [s for s, _ in eq1]
            # Normalize to start at 1
            start_eq = eq0[0][1] if eq0 else 1.0
            vals0_norm = [eq / start_eq for _, eq in eq0]
            vals1_norm = [eq / start_eq for _, eq in eq1]

            ax2.plot(steps0, vals0_norm, color=colors[0], label='close > max(close) [MODE=0]',
                     linewidth=1.5, alpha=0.9)
            ax2.plot(steps1, vals1_norm, color=colors[1], label='close > max(high) [MODE=1]',
                     linewidth=1.5, alpha=0.9)
            ax2.set_yscale('log')
            ax2.set_ylabel('Normalized Equity (log scale)')
            ax2.set_xlabel('Test Window Step (bar number within window)')
            ax2.set_title('Base5: Compound Equity Curve (Log Scale)\nAggregated across all 6 walk-forward windows')
            ax2.legend(fontsize=8)
            ax2.grid(True, alpha=0.3)
            # Add final values as annotations
            final0 = vals0_norm[-1] if vals0_norm else 1
            final1 = vals1_norm[-1] if vals1_norm else 1
            ax2.annotate(f'{final0:.2f}x', xy=(steps0[-1], final0),
                         xytext=(5, 0), textcoords='offset points', fontsize=8, color=colors[0])
            ax2.annotate(f'{final1:.2f}x', xy=(steps1[-1], final1),
                         xytext=(5, 0), textcoords='offset points', fontsize=8, color=colors[1])

    # ── Bottom-left: Average Sharpe and return per mode ────────────────────────
    ax3 = axes[1, 0]
    mode_labels = ['MODE=0\nclose > max(close)\n[current prod]', 'MODE=1\nclose > max(high)\n[stricter breakout]']
    avg_sharpes = [stats[0]['avg_sharpe'], stats[1]['avg_sharpe']]
    avg_rets = [stats[0]['avg_ret'], stats[1]['avg_ret']]
    x3 = np.arange(2)
    ax3_twin = ax3.twinx()
    bars3 = ax3.bar(x3, avg_sharpes, 0.4, label='Avg Sharpe', color='#4CAF50', alpha=0.8)
    bars4 = ax3_twin.bar(x3 + 0.4, avg_rets, 0.4, label='Avg Return %', color='#FF9800', alpha=0.8)
    ax3.set_ylabel('Avg Sharpe Ratio', color='#4CAF50')
    ax3_twin.set_ylabel('Avg Return (%)', color='#FF9800')
    ax3.set_xticks(x3 + 0.2)
    ax3.set_xticklabels(mode_labels, fontsize=9)
    ax3.set_title('Average Sharpe Ratio & Return (9 Universes × 6 Windows)')
    ax3.legend(loc='upper left', fontsize=8)
    ax3_twin.legend(loc='upper right', fontsize=8)
    ax3.grid(axis='y', alpha=0.3)

    # Add stat labels on bars
    for bar, val in zip(bars3, avg_sharpes):
        ax3.text(bar.get_x() + bar.get_width()/2., bar.get_height() + 0.05,
                 f'{val:.3f}', ha='center', va='bottom', fontsize=8, fontweight='bold')
    for bar, val in zip(bars4, avg_rets):
        ax3_twin.text(bar.get_x() + bar.get_width()/2., bar.get_height() + 1,
                      f'{val:.1f}%', ha='center', va='bottom', fontsize=8, fontweight='bold')

    # ── Bottom-right: Trade count comparison ───────────────────────────────────
    ax4 = axes[1, 1]
    trade_counts = [stats[0]['total_trades'], stats[1]['total_trades']]
    pass_rates_global = [stats[0]['pass_rate'], stats[1]['pass_rate']]
    pass_counts = [stats[0]['passes'], stats[1]['passes']]
    total_windows = [stats[0]['n'], stats[1]['n']]
    x4 = np.arange(2)
    bars5 = ax4.bar(x4, trade_counts, 0.4, label='Total Trades', color='#9C27B0', alpha=0.8)
    ax4_twin = ax4.twinx()
    bars6 = ax4_twin.bar(x4 + 0.4, pass_rates_global, 0.4, label='Pass Rate %', color='#00BCD4', alpha=0.8)
    ax4.set_ylabel('Total Trades (all windows)', color='#9C27B0')
    ax4_twin.set_ylabel('Global Pass Rate (%)', color='#00BCD4')
    ax4.set_xticks(x4 + 0.2)
    ax4.set_xticklabels(mode_labels, fontsize=9)
    ax4.set_title(f'Trade Count & Pass Rate\n{pass_counts[0]}/{total_windows[0]} windows pass [MODE=0] | {pass_counts[1]}/{total_windows[1]} windows pass [MODE=1]')
    ax4.legend(loc='upper left', fontsize=8)
    ax4_twin.legend(loc='upper right', fontsize=8)
    ax4.grid(axis='y', alpha=0.3)

    for bar, val in zip(bars5, trade_counts):
        ax4.text(bar.get_x() + bar.get_width()/2., bar.get_height() + 5,
                 f'{val}', ha='center', va='bottom', fontsize=8, fontweight='bold')
    for bar, val in zip(bars6, pass_rates_global):
        ax4_twin.text(bar.get_x() + bar.get_width()/2., bar.get_height() + 0.5,
                      f'{val:.1f}%', ha='center', va='bottom', fontsize=8, fontweight='bold')

    plt.tight_layout(rect=[0, 0, 1, 0.96])

    # Caption
    caption = (
        f"Turtle+Chandelier Entry Mode Sweep — EP=21, CHAND(28,2.0), DUAL_EXIT, CAP=3, HM=45, FEE=20bp RT\n"
        f"Walk-forward: 252-bar train / 252-bar test | 9 universes × up to 6 windows | Min 3 trades/window\n"
        f"MODE=0 (close > max_close): avg Sharpe {stats[0]['avg_sharpe']:.3f}, pass {stats[0]['passes']}/{stats[0]['n']} ({stats[0]['pass_rate']:.1f}%) | {stats[0]['total_trades']} trades\n"
        f"MODE=1 (close > max_high): avg Sharpe {stats[1]['avg_sharpe']:.3f}, pass {stats[1]['passes']}/{stats[1]['n']} ({stats[1]['pass_rate']:.1f}%) | {stats[1]['total_trades']} trades"
    )
    fig.text(0.5, 0.01, caption, ha='center', va='bottom', fontsize=8, style='italic',
             bbox=dict(boxstyle='round', facecolor='#f5f5f5', alpha=0.5))

    plt.savefig(OUTFILE, dpi=150, bbox_inches='tight', facecolor='white')
    print(f"Saved: {OUTFILE}")

    # Also save a simple line chart
    fig2, ax = plt.subplots(figsize=(12, 7))
    if base5_key_0 in equity_data and base5_key_1 in equity_data:
        eq0 = compound_equity_for_key(equity_data[base5_key_0])
        eq1 = compound_equity_for_key(equity_data[base5_key_1])
        if eq0 and eq1:
            steps0 = [s for s, _ in eq0]
            steps1 = [s for s, _ in eq1]
            start_eq = eq0[0][1] if eq0 else 1.0
            vals0_norm = [eq / start_eq for _, eq in eq0]
            vals1_norm = [eq / start_eq for _, eq in eq1]

            ax.plot(steps0, vals0_norm, color=colors[0], label=f'close > max(close) [MODE=0] — {vals0_norm[-1]:.2f}x',
                    linewidth=2, alpha=0.9)
            ax.plot(steps1, vals1_norm, color=colors[1], label=f'close > max(high) [MODE=1] — {vals1_norm[-1]:.2f}x',
                    linewidth=2, alpha=0.9)
            ax.set_yscale('log')
            ax.set_ylabel('Normalized Equity (log scale)', fontsize=11)
            ax.set_xlabel('Test Window Step (bar number within window)', fontsize=11)
            ax.set_title('Turtle+Chandelier — Entry Mode Equity Comparison (Base5, 6 Walk-Forward Windows Compounded)',
                         fontsize=12, fontweight='bold')
            ax.legend(fontsize=10)
            ax.grid(True, alpha=0.3)

            # Add summary stats box
            summary_text = (
                f"MODE=0 (close > max_close): Sharpe {stats[0]['avg_sharpe']:.3f}, Pass {stats[0]['pass_rate']:.1f}%, Trades {stats[0]['total_trades']}\n"
                f"MODE=1 (close > max_high): Sharpe {stats[1]['avg_sharpe']:.3f}, Pass {stats[1]['pass_rate']:.1f}%, Trades {stats[1]['total_trades']}"
            )
            ax.text(0.02, 0.98, summary_text, transform=ax.transAxes, fontsize=9,
                    verticalalignment='top', fontfamily='monospace',
                    bbox=dict(boxstyle='round', facecolor='white', alpha=0.7))

    plt.tight_layout()
    OUTFILE2 = "charts/turtle_entry_mode_equity.png"
    plt.savefig(OUTFILE2, dpi=150, bbox_inches='tight', facecolor='white')
    print(f"Saved: {OUTFILE2}")

if __name__ == "__main__":
    main()
