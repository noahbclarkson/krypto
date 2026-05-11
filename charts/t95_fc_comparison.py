#!/usr/bin/env python3
"""
T95 FRESHNESS_COOLDOWN Equity Comparison Chart — updated version.
Plots equity curves for key FC values from the T95 sweep with proper legend.
"""
import csv, os, sys

SNAPSHOT_DIR = "/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots"
CHART_DIR = "/home/ubuntu/.openclaw/workspace-krypto/charts"
CHART_PATH = os.path.join(CHART_DIR, "comparison_chart.png")

def load_equity_csv(path):
    bars, dates, equities = [], [], []
    with open(path) as f:
        reader = csv.DictReader(f)
        for row in reader:
            bars.append(int(row['bar']))
            dates.append(row['date'])
            equities.append(float(row['equity']))
    return bars, dates, equities

def load_summary():
    rows = []
    with open(os.path.join(SNAPSHOT_DIR, "t95_fc_sweep_summary.csv")) as f:
        reader = csv.DictReader(f)
        for row in reader:
            rows.append({
                'fc': int(row['fc']),
                'pass_pct': float(row['pass_pct']),
                'avg_sharpe': float(row['avg_sharpe']),
                'avg_return': float(row['avg_return_pct']),
                'avg_dd': float(row['avg_max_dd_pct']),
                'total_trades': int(row['total_trades']),
                'win_rate': float(row['win_rate_pct']),
            })
    return rows

def main():
    summary = load_summary()

    # Sort by pass rate desc, then Sharpe desc
    by_pass = sorted(summary, key=lambda x: (-x['pass_pct'], -x['avg_sharpe']))
    by_sharpe = sorted(summary, key=lambda x: (-x['avg_sharpe'], -x['pass_pct']))

    # Key FC values for chart:
    # FC=0: baseline (exact-live verified 2.76x, 286 trades)
    # FC=97: PASS WINNER (83.0% pass rate, walk-forward sweep)
    # FC=2: live_bot reported 2.90x vs 2.76x baseline (quick re-entry cost small)
    # FC=55: Sharpe plateau winner (4.40 Sharpe, 75.9% pass)
    key_fcs = [0, 2, 55, 97]

    # Load equity data for available files
    equity_data = {}
    for fc in key_fcs:
        path = os.path.join(SNAPSHOT_DIR, f"t95_fc_equity_{fc}.csv")
        if os.path.exists(path):
            bars, dates, equities = load_equity_csv(path)
            equity_data[fc] = (bars, dates, equities)
            print(f"  FC={fc}: {len(equities)} bars, equity [{min(equities):.4f}, {max(equities):.4f}]x")
        else:
            print(f"  FC={fc}: NOT FOUND")

    colors = {0: '#555555', 2: '#2196F3', 55: '#FF5722', 97: '#4CAF50'}

    labels = {}
    for fc in key_fcs:
        sr_pass = next((s for s in by_pass if s['fc'] == fc), None)
        if sr_pass:
            if fc == 0:
                labels[fc] = f"FC=0 Baseline (52% pass, Sharpe 2.11)"
            elif fc == 97:
                labels[fc] = f"FC=97 WINNER (83% pass, Sharpe 3.97)"
            elif fc == 2:
                labels[fc] = f"FC=2 (live_bot: 2.90x vs 2.76x)"
            else:
                labels[fc] = f"FC={fc} (pass {sr_pass['pass_pct']:.0f}%, Sharpe {sr_pass['avg_sharpe']:.2f})"

    import matplotlib
    matplotlib.use('Agg')
    import matplotlib.pyplot as plt

    fig, (ax_eq, ax_dd) = plt.subplots(2, 1, figsize=(15, 10), gridspec_kw={'height_ratios': [3, 1.2]})

    # Equity curves — log scale, dynamic Y
    for fc in sorted(key_fcs):
        if fc not in equity_data:
            continue
        bars, dates, equities = equity_data[fc]
        x = list(range(len(equities)))
        label = labels.get(fc, f"FC={fc}")
        color = colors.get(fc, '#333333')
        lw = 2.0 if fc in [0, 97] else 1.2
        alpha = 0.95 if fc in [0, 97] else 0.7
        ax_eq.semilogy(x, equities, label=label, color=color, linewidth=lw, alpha=alpha)

    ax_eq.set_title(
        "T95 FRESHNESS_COOLDOWN Hyperopt — Equity Curves (Log Scale)\n"
        "101 values tested: FC ∈ [0..100 step 1] | 9 universes × 6 windows | Exact Live-Bot Path",
        fontsize=13, fontweight='bold'
    )
    ax_eq.set_ylabel("Equity (× initial)", fontsize=11)
    ax_eq.legend(loc='upper left', fontsize=10, framealpha=0.9)
    ax_eq.grid(True, alpha=0.3, linestyle='--')
    ax_eq.set_xlabel("Bar (day index)", fontsize=11)

    # Text box with summary
    box_lines = ["T95 FRESHNESS_COOLDOWN Hyperopt | 9 universes × 6 windows = 54 per FC value", ""]
    for fc in sorted(key_fcs):
        sr = next((s for s in by_pass if s['fc'] == fc), None)
        if not sr:
            sr = next((s for s in by_sharpe if s['fc'] == fc), None)
        if sr:
            marker = " ← BASELINE" if fc == 0 else (" ← PASS WINNER" if sr['pass_pct'] >= 83 else "")
            box_lines.append(
                f"FC={fc:3d}: pass {sr['pass_pct']:5.1f}%  Sharpe {sr['avg_sharpe']:.3f}  "
                f"ret {sr['avg_return']:+.1f}%  DD {sr['avg_dd']:.1f}%{marker}"
            )

    text_box = '\n'.join(box_lines)
    ax_eq.text(0.99, 0.02, text_box, transform=ax_eq.transAxes,
               fontsize=8, verticalalignment='bottom', horizontalalignment='right',
               fontfamily='monospace',
               bbox=dict(boxstyle='round,pad=0.4', facecolor='white', alpha=0.88, edgecolor='#cccccc'))

    # Drawdown panel
    for fc in sorted(key_fcs):
        if fc not in equity_data:
            continue
        bars, dates, equities = equity_data[fc]
        x = list(range(len(equities)))
        peak = equities[0]
        dd_pct = []
        for eq in equities:
            if eq > peak:
                peak = eq
            dd_pct.append((peak - eq) / peak * 100.0)
        color = colors.get(fc, '#333333')
        label = labels.get(fc, f"FC={fc}")
        ax_dd.fill_between(x, 0, dd_pct, color=color, alpha=0.18)
        ax_dd.plot(x, dd_pct, color=color, linewidth=1.0, alpha=0.85, label=label.split('(')[0].strip())

    ax_dd.set_ylabel("Drawdown (%)", fontsize=11)
    ax_dd.set_xlabel("Bar (day index)", fontsize=11)
    ax_dd.legend(loc='upper left', fontsize=9)
    ax_dd.grid(True, alpha=0.3, linestyle='--')
    ax_dd.set_ylim(bottom=0)

    footer = (
        f"FRESHNESS_COOLDOWN ∈ [0..100 step 1] — 101 values swept | "
        f"Chart: {CHART_PATH}"
    )
    ax_dd.text(0.5, -0.30, footer, transform=ax_dd.transAxes,
               fontsize=7.5, ha='center', va='top', color='#555555', fontfamily='monospace')

    plt.tight_layout(rect=[0, 0.07, 1, 1])
    os.makedirs(CHART_DIR, exist_ok=True)
    plt.savefig(CHART_PATH, dpi=150, bbox_inches='tight', facecolor='white')
    print(f"\nChart saved: {CHART_PATH}")

if __name__ == "__main__":
    main()