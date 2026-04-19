#!/usr/bin/env python3
"""
Equity Comparison Chart: Regime ATR Trend Hyperopt
Plots: Baseline (pct=0.90) vs Winner vs Runner-ups
"""

import csv
import sys
import math

def load_equity_csv(path):
    """Load equity CSV: universe,window,step,equity,atr_lookback,atr_trend_pct,label"""
    by_label = {}
    with open(path, 'r') as f:
        reader = csv.DictReader(f)
        for row in reader:
            label = row['label']
            step = int(row['step'])
            equity = float(row['equity'])
            if label not in by_label:
                by_label[label] = []
            # store (step, equity) pairs; aggregate across universes/windows later
            by_label[label].append((step, equity))
    return by_label

def aggregate_by_step(data):
    """Aggregate equity across all runs for each step (mean equity)."""
    by_step = {}
    for (step, eq) in data:
        if step not in by_step:
            by_step[step] = []
        by_step[step].append(eq)
    steps = sorted(by_step.keys())
    mean_equity = []
    for s in steps:
        vals = by_step[s]
        mean_equity.append(sum(vals) / len(vals))
    return steps, mean_equity

def compute_metrics(steps, equity):
    """Compute Sharpe-like and drawdown metrics from equity series."""
    if len(steps) < 2:
        return 0.0, 0.0, 1.0

    # Daily returns
    rets = []
    for i in range(1, len(equity)):
        if equity[i-1] > 0:
            r = (equity[i] / equity[i-1]) - 1.0
            rets.append(r)

    if len(rets) < 10:
        return 0.0, 0.0, equity[-1] if equity else 1.0

    mn = sum(rets) / len(rets)
    sd = math.sqrt(sum((r - mn)**2 for r in rets) / len(rets))
    if sd == 0:
        sharpe = 0.0
    else:
        sharpe = mn * math.sqrt(252) / sd

    # Max drawdown
    peak = equity[0]
    max_dd = 0.0
    for e in equity:
        if e > peak:
            peak = e
        dd = (peak - e) / peak
        if dd > max_dd:
            max_dd = dd

    final = equity[-1]
    return sharpe, max_dd * 100.0, final

def main():
    equity_csv = "snapshots/regime_atr_trend_equity.csv"
    output_png = "charts/comparison_chart.png"

    try:
        by_label = load_equity_csv(equity_csv)
    except FileNotFoundError:
        print(f"ERROR: {equity_csv} not found. Run the hyperopt harness first.")
        sys.exit(1)

    if not by_label:
        print("ERROR: No data in equity CSV")
        sys.exit(1)

    # ── Identify labels ──────────────────────────────────────────────────────
    baseline_label = None
    candidate_labels = []
    for lbl in by_label:
        if 'baseline' in lbl:
            baseline_label = lbl
        else:
            candidate_labels.append(lbl)

    # Sort candidates by mean equity final value
    candidate_final = {}
    for lbl in candidate_labels:
        data = by_label[lbl]
        _, eq = aggregate_by_step(data)
        final = eq[-1] if eq else 1.0
        candidate_final[lbl] = final

    sorted_candidates = sorted(candidate_labels, key=lambda l: candidate_final[l], reverse=True)

    # Select: baseline + top 3 candidates (max 4 lines total)
    selected = []
    if baseline_label:
        selected.append(baseline_label)
    selected.extend(sorted_candidates[:3])

    print(f"\n  Selected configs for chart:")
    metrics = {}
    for lbl in selected:
        steps, eq = aggregate_by_step(by_label[lbl])
        sh, dd, final = compute_metrics(steps, eq)
        metrics[lbl] = (sh, dd, final)
        print(f"    {lbl:40s} final={final:12.3f} sharpe={sh:+.3f} maxDD={dd:.1f}%")

    # ── Plot ───────────────────────────────────────────────────────────────────
    try:
        import matplotlib
        matplotlib.use('Agg')
        import matplotlib.pyplot as plt
        import matplotlib.ticker as mticker
    except ImportError:
        print("ERROR: matplotlib not installed. Run: pip install matplotlib")
        sys.exit(1)

    fig, (ax1, ax2) = plt.subplots(2, 1, figsize=(14, 10), sharex=False,
                                   gridspec_kw={'height_ratios': [3, 1]})

    colors = ['#2196F3', '#FF5722', '#4CAF50', '#9C27B0', '#FF9800']
    linestyles = ['-', '--', '-.', ':', '-']
    col_idx = 0

    for lbl in selected:
        steps, eq = aggregate_by_step(by_label[lbl])
        if not steps:
            continue

        sh, dd, final = metrics[lbl]
        color = colors[col_idx % len(colors)]
        ls = linestyles[col_idx % len(linestyles)]

        # Short label for legend
        if 'baseline' in lbl:
            disp = f"BASELINE (0.90)"
        else:
            # Parse lookback and pct from label
            parts = lbl.replace('lb', '').replace('p', '_').split('_')
            lb = parts[0] if len(parts) > 0 else '?'
            pct = parts[1] if len(parts) > 1 else '?'
            disp = f"lb={lb} pct={pct}"

        ax1.plot(steps, eq, label=disp, color=color, linestyle=ls, linewidth=1.5, alpha=0.9)
        col_idx += 1

    ax1.set_title("Regime ATR Trend Hyperopt: Equity Curves\n(Baseline vs Winner vs Runner-ups)", fontsize=14, fontweight='bold')
    ax1.set_ylabel("Portfolio Equity (log scale)", fontsize=11)
    ax1.set_yscale('log')
    ax1.yaxis.set_major_formatter(mticker.FuncFormatter(lambda x, _: f'{x:.1f}'))
    ax1.grid(True, alpha=0.3, linestyle='--')
    ax1.legend(loc='upper left', fontsize=10, framealpha=0.9)

    # Caption with metrics
    caption_lines = []
    for lbl in selected:
        sh, dd, final = metrics[lbl]
        if 'baseline' in lbl:
            caption_lines.append(f"BASELINE pct=0.90: final={final:.3f} Sharpe={sh:+.3f} MaxDD={dd:.1f}%")
        else:
            parts = lbl.replace('lb', '').replace('p', '_').split('_')
            lb = parts[0] if len(parts) > 0 else '?'
            pct = parts[1] if len(parts) > 1 else '?'
            caption_lines.append(f"lb={lb} pct={pct}: final={final:.3f} Sharpe={sh:+.3f} MaxDD={dd:.1f}%")

    caption = " | ".join(caption_lines)
    ax1.set_xlabel(f"Step (252-bar windows) | {caption}", fontsize=9)
    ax1.title.set_fontsize(12)

    # ── Drawdown panel ──────────────────────────────────────────────────────
    col_idx = 0
    for lbl in selected:
        steps, eq = aggregate_by_step(by_label[lbl])
        if not steps:
            continue

        # Compute drawdown series
        peak = eq[0]
        dd_series = []
        for e in eq:
            if e > peak:
                peak = e
            dd = (peak - e) / peak * 100.0
            dd_series.append(dd)

        color = colors[col_idx % len(colors)]
        ls = linestyles[col_idx % len(linestyles)]

        if 'baseline' in lbl:
            disp = "BASELINE (0.90)"
        else:
            parts = lbl.replace('lb', '').replace('p', '_').split('_')
            lb = parts[0] if len(parts) > 0 else '?'
            pct = parts[1] if len(parts) > 1 else '?'
            disp = f"lb={lb} pct={pct}"

        ax2.plot(steps, dd_series, label=disp, color=color, linestyle=ls, linewidth=1.5, alpha=0.9)
        col_idx += 1

    ax2.set_ylabel("Max Drawdown %", fontsize=11)
    ax2.set_yscale('linear')
    ax2.grid(True, alpha=0.3, linestyle='--')
    ax2.legend(loc='lower left', fontsize=9, framealpha=0.9)
    ax2.set_xlabel("Step (252-bar walk-forward windows)", fontsize=10)

    plt.tight_layout()
    plt.savefig(output_png, dpi=150, bbox_inches='tight', facecolor='white')
    print(f"\n  Chart saved → {output_png}")
    plt.close()

if __name__ == '__main__':
    main()
