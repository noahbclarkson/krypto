#!/usr/bin/env python3
"""
Plot TURTLE_ATR_MULT Fine Sweep results.
M ∈ [1.00..5.00] step 0.05 — 81 values, 9 universes, 6 windows each.
"""
import csv
import sys

def main():
    csv_path = "snapshots/turtle_atr_mult_fine_sweep.csv"
    rows = []
    with open(csv_path) as f:
        reader = csv.DictReader(f)
        for row in reader:
            rows.append({
                'mult': float(row['mult']),
                'pass': int(row['pass']),
                'sharpe': float(row['avg_sharpe']),
                'ret': float(row['avg_return_pct']),
                'dd': float(row['worst_dd_pct']),
                'trades': int(row['total_trades']),
            })

    print(f"Loaded {len(rows)} rows")
    for r in rows[:5]:
        print(r)
    print("...")

if __name__ == "__main__":
    main()