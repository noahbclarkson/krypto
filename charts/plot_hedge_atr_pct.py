#!/usr/bin/env python3
"""
Plot HEDGE_ATR_PCT sweep equity curves.
Generates: /home/ubuntu/.openclaw/workspace-krypto/charts/comparison_chart.png
"""
import csv
import matplotlib.pyplot as plt
import sys

SUMMARY_CSV = "/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/t88_hedge_atr_pct_summary.csv"
EQUITY_CSV = "/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/t88_hedge_atr_pct_equity.csv"
OUTPUT_PNG = "/home/ubuntu/.openclaw/workspace-krypto/charts/comparison_chart.png"

def read_summary():
    """Read summary CSV."""
    rows = []
    with open(SUMMARY_CSV, newline='') as f:
        reader = csv.DictReader(f)
        for row in reader:
            rows.append({
                'pct': float(row['pct']),
                'pass': int(row['pass']),
                'total': int(row['total']),
                'sharpe': float(row['sharpe']),
                'dd': float(row['dd']),
                'ret': float(row['ret']),
            })
    return rows

def read_equity():
    """Read equity CSV by pct."""
    by_pct = {}
    with open(EQUITY_CSV, newline='') as f:
        reader = csv.DictReader(f)
        for row in reader:
            pct = float(row['pct'])
            key = f"{row['universe']}_{row['window']}"
            equity = float(row['equity'])
            by_pct.setdefault(pct, {})[key] = equity
    return by_pct

def main():
    print("Reading summary...")
    summary = read_summary()
    if not summary:
        print(f"ERROR: No summary data at {SUMMARY_CSV}")
        sys.exit(1)

    summary.sort(key=lambda x: x['pct'])
    pcts = [r['pct'] for r in summary]
    passes = [r['pass'] for r in summary]
    sharpes = [r['sharpe'] for r in summary]
    dds = [r['dd'] for r in summary]

    # Find baseline and winners
    baseline = next((r for r in summary if abs(r['pct'] - 0.45) < 0.001), None)
    winner = max(summary, key=lambda r: r['sharpe'])
    runners = sorted(summary, key=lambda r: r['sharpe'], reverse=True)[1:3]

    baseline_sharpe = baseline['sharpe'] if baseline else 0.0

    print(f"\n=== HEDGE_ATR_PCT Summary ===")
    print(f"{'PCT':>6} {'Pass':>6} {'Sharpe':>8} {'DD%':>7}")
    for r in summary:
        marker = ""
        if abs(r['pct'] - winner['pct']) < 0.001:
            marker = " <-- WINNER"
        elif abs(r['pct'] - 0.45) < 0.001:
            marker = " <-- BASELINE"
        print(f"{r['pct']:6.2f} {r['pass']:6}/{r['total']:2} {r['sharpe']:8.3f} {r['dd']:7.2f}{marker}")

    print(f"\nBaseline PCT=0.45: Sharpe {baseline_sharpe:.3f}")
    print(f"Winner PCT={winner['pct']:.2f}: Sharpe {winner['sharpe']:.3f}")

    # Plot
    fig, (ax1, ax2) = plt.subplots(2, 1, figsize=(14, 8), sharex=True)

    # Bar 1: pass rate
    ax1.bar(pcts, passes, width=0.03, color='steelblue', alpha=0.8)
    ax1.axhline(38, color='orange', linestyle='--', linewidth=1.5, label='60% threshold')
    ax1.set_ylabel('Pass Count (of 63)', fontsize=11)
    ax1.set_title('HEDGE_ATR_PCT: Pass Rate + Sharpe by Threshold', fontsize=13)
    ax1.legend(loc='upper right')
    ax1.grid(axis='y', alpha=0.3)
    ax1.set_ylim(0, 70)

    # Bar 2: Sharpe
    ax2.bar(pcts, sharpes, width=0.03, color='darkgreen', alpha=0.8)
    ax2.axvline(0.45, color='red', linestyle=':', linewidth=2, label='Baseline (0.45)')
    
    # Mark winner
    ax2.axvline(winner['pct'], color='gold', linestyle='--', linewidth=2, label=f'Winner ({winner["pct"]:.2f})')
    ax2.set_xlabel('HEDGE_ATR_PCT', fontsize=11)
    ax2.set_ylabel('Avg Sharpe', fontsize=11)
    ax2.grid(axis='y', alpha=0.3)
    ax2.legend(loc='upper right')

    plt.tight_layout()
    fig.savefig(OUTPUT_PNG, dpi=150, bbox_inches='tight')
    print(f"\nSaved: {OUTPUT_PNG}")
    plt.close(fig)

if __name__ == '__main__':
    main()