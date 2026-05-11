#!/usr/bin/env python3
"""
T95 FRESHNESS_COOLDOWN Equity Comparison Chart
Plots equity curves for key FC values from the T95 sweep.
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
                'trades': int(row['total_trades']),
                'win_rate': float(row['win_rate_pct']),
            })
    return rows

def main():
    summary = load_summary()
    # Sort by pass rate desc, then Sharpe desc
    by_pass = sorted(summary, key=lambda x: (-x['pass_pct'], -x['avg_sharpe']))
    by_sharpe = sorted(summary, key=lambda x: (-x['avg_sharpe'], -x['pass_pct']))

    # Pick: FC=0 (baseline), top-pass-rate winner (FC=96), top-Sharpe (FC=57), runner-up Sharpe (FC=58)
    key_fcs = [0]
    # Add best by pass rate (winner)
    for r in by_pass:
        if r['fc'] not in key_fcs:
            key_fcs.append(r['fc'])
            break
    # Add best by Sharpe
    for r in by_sharpe:
        if r['fc'] not in key_fcs:
            key_fcs.append(r['fc'])
            break
    # Add 2nd Sharpe runner-up
    for r in by_sharpe:
        if r['fc'] not in key_fcs:
            key_fcs.append(r['fc'])
            break

    key_fcs.sort()
    print(f"Key FC values for chart: {key_fcs}")
    print(f"  by pass: {[r['fc'] for r in by_pass[:3]]}")
    print(f"  by sharpe: {[r['fc'] for r in by_sharpe[:3]]}")

    # Load equity data
    equity_data = {}
    for fc in key_fcs:
        path = os.path.join(SNAPSHOT_DIR, f"t95_fc_equity_{fc}.csv")
        if os.path.exists(path):
            bars, dates, equities = load_equity_csv(path)
            equity_data[fc] = (bars, dates, equities)
            eq_min = min(equities)
            eq_max = max(equities)
            print(f"  FC={fc}: {len(equities)} bars, equity [{eq_min:.4f}, {eq_max:.4f}]x")
        else:
            print(f"  FC={fc}: NOT FOUND — will skip")

    if len(equity_data) < 1:
        print("ERROR: no equity data found")
        sys.exit(1)

    colors = {0: '#888888', 96: '#2196F3', 57: '#FF5722', 58: '#4CAF50', 97: '#9C27B0', 98: '#795548'}

    labels = {}
    for fc in key_fcs:
        sr_pass = next((s for s in by_pass if s['fc'] == fc), None)
        sr_sharpe = next((s for s in by_sharpe if s['fc'] == fc), None)
        if sr_pass and sr_sharpe and sr_pass['fc'] == sr_sharpe['fc']:
            labels[fc] = f"FC={fc} (pass={sr_pass['pass_pct']:.0f}%, Sharpe={sr_pass['avg_sharpe']:.2f}) [PASS WINNER]"
        elif fc == 0:
            labels[fc] = f"FC=0 Baseline (pass=52%, Sharpe={next((s for s in summary if s['fc']==0), None)['avg_sharpe']:.2f})"
        elif sr_sharpe:
            labels[fc] = f"FC={fc} (pass={sr_sharpe['pass_pct']:.0f}%, Sharpe={sr_sharpe['avg_sharpe']:.2f})"

    import matplotlib
    matplotlib.use('Agg')
    import matplotlib.pyplot as plt

    fig, (ax_eq, ax_dd) = plt.subplots(2, 1, figsize=(15, 10), gridspec_kw={'height_ratios': [3, 1.2]})

    # Equity curves — log scale, dynamic Y
    for fc in key_fcs:
        if fc not in equity_data:
            continue
        bars, dates, equities = equity_data[fc]
        x = list(range(len(equities)))
        label = labels.get(fc, f"FC={fc}")
        color = colors.get(fc, '#333333')
        ax_eq.semilogy(x, equities, label=label, color=color, linewidth=1.6, alpha=0.9)

    ax_eq.set_title("T95 FRESHNESS_COOLDOWN Hyperopt — Equity Curves (Log Scale)\n"
                    "101 values tested: FC ∈ [0..100 step 1] | Exact Live-Bot Path (Turtle ATR-only exit)",
                    fontsize=13, fontweight='bold')
    ax_eq.set_ylabel("Equity (× initial)", fontsize=11)
    ax_eq.legend(loc='upper left', fontsize=9, framealpha=0.9)
    ax_eq.grid(True, alpha=0.3, linestyle='--')
    ax_eq.set_xlabel("Bar (day index)", fontsize=11)

    # Text box with summary
    box_lines = ["Walk-Forward: 9 universes × 6 windows = 54 per FC value", ""]
    for fc in key_fcs:
        sr = next((s for s in by_pass if s['fc'] == fc), None)
        if not sr:
            sr = next((s for s in by_sharpe if s['fc'] == fc), None)
        if sr:
            box_lines.append(f"FC={fc:3d}: pass {sr['pass_pct']:5.1f}%  Sharpe {sr['avg_sharpe']:.3f}  ret {sr['avg_return']:+.1f}%  DD {sr['avg_dd']:.1f}%")

    text_box = '\n'.join(box_lines)
    ax_eq.text(0.99, 0.02, text_box, transform=ax_eq.transAxes,
               fontsize=7.5, verticalalignment='bottom', horizontalalignment='right',
               fontfamily='monospace',
               bbox=dict(boxstyle='round,pad=0.4', facecolor='white', alpha=0.88, edgecolor='#cccccc'))

    # Drawdown panel
    for fc in key_fcs:
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
    ax_dd.legend(loc='upper left', fontsize=8)
    ax_dd.grid(True, alpha=0.3, linestyle='--')
    ax_dd.set_ylim(bottom=0)

    footer = (f"FRESHNESS_COOLDOWN ∈ [0..100 step 1] — 101 values × 9 universes × 6 windows | "
              f"Chart: {CHART_PATH}")
    ax_dd.text(0.5, -0.30, footer, transform=ax_dd.transAxes,
               fontsize=7.5, ha='center', va='top', color='#555555', fontfamily='monospace')

    plt.tight_layout(rect=[0, 0.07, 1, 1])
    os.makedirs(CHART_DIR, exist_ok=True)
    plt.savefig(CHART_PATH, dpi=150, bbox_inches='tight', facecolor='white')
    print(f"\nChart saved: {CHART_PATH}")

    # Print results table
    print("\n=== T95 Full Sweep Summary (sorted by pass%, then Sharpe) ===")
    print(f"{'FC':>4}  {'Pass%':>6}  {'Sharpe':>7}  {'Return%':>8}  {'MaxDD%':>7}  {'Trades':>6}")
    print("-" * 55)
    for sr in by_pass[:30]:
        marker = " ←" if sr['fc'] in key_fcs else ""
        print(f"{sr['fc']:4d}  {sr['pass_pct']:6.1f}  {sr['avg_sharpe']:7.4f}  {sr['avg_return']:+8.2f}  {sr['avg_dd']:7.2f}  {sr['trades']:6d}{marker}")
    print("...")
    print(f"\nTop Sharpe winners:")
    for sr in by_sharpe[:5]:
        print(f"  FC={sr['fc']:3d}: pass={sr['pass_pct']:.1f}%, Sharpe={sr['avg_sharpe']:.4f}")

if __name__ == "__main__":
    main()